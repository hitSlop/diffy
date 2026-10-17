//! Read-only project discovery. No agent execution, terminal reads, or credentials.
use agent_sessions::{Agent, DiscoverFilter, Event, EventKinds, ReadOptions, Roots, TailMode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Herdr,
    Codex,
    Claude,
}
impl Source {
    pub const ALL: [Self; 3] = [Self::Herdr, Self::Codex, Self::Claude];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Herdr => "Herdr",
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub onboarding_completed: bool,
    pub excluded: BTreeSet<PathBuf>,
    pub excluded_repositories: BTreeSet<PathBuf>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: PathBuf,
    pub repository: Option<PathBuf>,
    pub sources: BTreeSet<Source>,
    pub error: Option<String>,
    pub updated_at: Option<SystemTime>,
}
impl Candidate {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub candidates: Vec<Candidate>,
    pub errors: [Option<String>; 3],
}
impl Snapshot {
    pub(crate) fn sort(&mut self) {
        self.candidates.sort_by_cached_key(|candidate| {
            (
                std::cmp::Reverse(candidate.updated_at),
                candidate.name().to_lowercase(),
                candidate.path.clone(),
            )
        });
    }
}
#[derive(Clone)]
pub struct Locations {
    pub herdr: Vec<PathBuf>,
    pub socket: Option<PathBuf>,
    pub codex: Option<PathBuf>,
    pub claude: Option<PathBuf>,
    pub errors: [Option<String>; 3],
}
impl Locations {
    pub fn from_env() -> Self {
        #[cfg(not(windows))]
        let home = std::env::home_dir();
        let mut errors = [None, None, None];
        let mut root = |agent, source: Source| match Roots::from_env_for(agent) {
            Ok(roots) => {
                if source == Source::Codex {
                    roots.codex
                } else {
                    roots.claude
                }
            }
            Err(_) => {
                errors[source.index()] = Some("Configuration directory override is empty.".into());
                None
            }
        };
        let codex = root(Agent::Codex, Source::Codex);
        let claude = root(Agent::ClaudeCode, Source::Claude);
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                #[cfg(windows)]
                {
                    std::env::var_os("APPDATA").map(PathBuf::from)
                }
                #[cfg(not(windows))]
                {
                    home.as_ref().map(|h| h.join(".config"))
                }
            });
        Self {
            herdr: config
                .map(|p| vec![p.join("herdr"), p.join("herdr-dev")])
                .unwrap_or_default(),
            socket: std::env::var_os("HERDR_SOCKET_PATH")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            codex,
            claude,
            errors,
        }
    }
}

type Stamp = (u64, Option<SystemTime>);

struct TemporaryRoots(Vec<PathBuf>);
impl Default for TemporaryRoots {
    fn default() -> Self {
        let roots = std::iter::once(std::env::temp_dir());
        #[cfg(unix)]
        let roots = roots.chain(["/tmp", "/var/tmp"].map(PathBuf::from));
        Self::new(roots)
    }
}
impl TemporaryRoots {
    fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut paths = BTreeSet::new();
        for root in roots {
            if root.is_absolute() {
                if let Ok(canonical) = fs::canonicalize(&root) {
                    paths.insert(canonical);
                }
                paths.insert(root);
            }
        }
        Self(paths.into_iter().collect())
    }
    fn contains(&self, checkout: &Path) -> bool {
        self.0.iter().any(|root| checkout.starts_with(root))
    }
}

#[derive(Default)]
pub struct Scanner {
    metadata: HashMap<PathBuf, (Stamp, Result<Vec<PathBuf>, ()>)>,
    roots: HashMap<PathBuf, (Instant, Result<PathBuf, String>)>,
    pub metadata_reads: usize,
    updated: HashMap<PathBuf, SystemTime>,
    temporary_roots: TemporaryRoots,
}
impl Scanner {
    fn cached(
        &mut self,
        path: &Path,
        read: impl FnOnce() -> Result<Vec<PathBuf>, ()>,
    ) -> Result<Vec<PathBuf>, ()> {
        let meta = fs::symlink_metadata(path).map_err(|_| ())?;
        if !meta.is_file() {
            return Err(());
        }
        let stamp = (meta.len(), meta.modified().ok());
        let result = if let Some((old, result)) = self.metadata.get(path)
            && *old == stamp
        {
            result.clone()
        } else {
            self.metadata_reads += 1;
            let result = read();
            self.metadata
                .insert(path.to_owned(), (stamp, result.clone()));
            result
        };
        if let Ok(paths) = &result {
            self.record_activity(paths, stamp.1);
        }
        result
    }
    fn record_activity(&mut self, paths: &[PathBuf], updated: Option<SystemTime>) {
        if let Some(updated) = updated {
            for path in paths {
                self.updated
                    .entry(path.clone())
                    .and_modify(|time| *time = (*time).max(updated))
                    .or_insert(updated);
            }
        }
    }
    pub fn scan(
        &mut self,
        locations: &Locations,
        enabled: [bool; 3],
        cancelled: &AtomicBool,
    ) -> Snapshot {
        let _span = tracing::info_span!("project_discovery").entered();
        // Rebuild from the files visited this scan, including cache hits. Removed
        // history must not leave an old maximum timestamp attached to a project.
        self.updated.clear();
        let mut raw = Vec::new();
        let mut snapshot = Snapshot::default();
        for source in Source::ALL {
            if !enabled[source.index()] || cancelled.load(Ordering::Relaxed) {
                continue;
            }
            if let Some(error) = &locations.errors[source.index()] {
                snapshot.errors[source.index()] = Some(error.clone());
                continue;
            }
            let (paths, failures) = match source {
                Source::Herdr => self.herdr(locations, cancelled),
                Source::Codex => self.sessions(locations.codex.as_deref(), Agent::Codex, cancelled),
                Source::Claude => {
                    self.sessions(locations.claude.as_deref(), Agent::ClaudeCode, cancelled)
                }
            };
            raw.extend(paths.into_iter().map(|path| (source, path)));
            if failures > 0 {
                snapshot.errors[source.index()] = Some(format!(
                    "{failures} metadata sources could not be read completely. Other results are available."
                ));
            }
        }
        let mut merged: BTreeMap<PathBuf, Candidate> = BTreeMap::new();
        for (source, path) in raw {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            if !path.is_absolute() {
                continue;
            }
            let updated_at = self.updated.get(&path).copied();
            let path = fs::canonicalize(&path).unwrap_or(path);
            // Old agent history frequently points at deleted checkouts. Do not clutter the picker.
            if !path.is_dir() {
                continue;
            }
            let result = match self.roots.get(&path) {
                Some((time, result)) if time.elapsed() < Duration::from_secs(30) => result.clone(),
                _ => {
                    let result = if path.is_dir() {
                        crate::repository::resolve_root(&path)
                    } else {
                        Err("Directory is unavailable on this machine.".into())
                    };
                    let result = result.map_err(|_| {
                        if path.is_dir() {
                            "Not an accessible Git repository.".into()
                        } else {
                            "Directory is unavailable on this machine.".into()
                        }
                    });
                    self.roots
                        .insert(path.clone(), (Instant::now(), result.clone()));
                    result
                }
            };
            let (path, error) = match result {
                Ok(root) => (root, None),
                Err(_) => continue,
            };
            // Filter the resolved checkout, including cache hits: agent histories
            // also contain disposable evaluation repos and temporary worktrees.
            if self.temporary_roots.contains(&path) {
                continue;
            }
            let candidate = merged.entry(path.clone()).or_insert_with(|| Candidate {
                repository: crate::repository::common_directory(&path).ok(),
                path,
                sources: BTreeSet::new(),
                error,
                updated_at,
            });
            candidate.updated_at = candidate.updated_at.max(updated_at);
            candidate.sources.insert(source);
        }
        snapshot.candidates = merged.into_values().collect();
        snapshot.sort();
        if !cancelled.load(Ordering::Relaxed) {
            self.metadata.retain(|path, _| path.is_file());
            self.roots.retain(|path, _| path.is_dir());
        }
        snapshot
    }
    fn sessions(
        &mut self,
        root: Option<&Path>,
        agent: Agent,
        cancelled: &AtomicBool,
    ) -> (Vec<PathBuf>, usize) {
        let Some(root) = root else {
            return (Vec::new(), 0);
        };
        let roots = match agent {
            Agent::Codex => Roots {
                codex: Some(root.to_owned()),
                claude: None,
            },
            Agent::ClaudeCode => Roots {
                claude: Some(root.to_owned()),
                codex: None,
            },
            _ => return (Vec::new(), 0),
        };
        let discovered = agent_sessions::discover(
            &roots,
            &DiscoverFilter {
                agents: vec![agent],
                ..Default::default()
            },
        );
        let mut failures = discovered.errors.len();
        let mut paths = BTreeSet::new();
        if agent == Agent::ClaudeCode
            && let Ok(dirs) = fs::read_dir(root.join("projects"))
        {
            for dir in dirs.flatten() {
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }
                if !dir.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let index = dir.path().join("sessions-index.json");
                if index.exists() {
                    match self.cached(&index, || read_claude_index(&index)) {
                        Ok(items) => paths.extend(items),
                        Err(()) => failures += 1,
                    }
                }
            }
        }
        for file in discovered.files {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            match self.cached(&file.path, || read_session(&file.path, agent)) {
                Ok(items) => paths.extend(items),
                Err(()) => failures += 1,
            }
        }
        (paths.into_iter().collect(), failures)
    }
    fn herdr(&mut self, locations: &Locations, cancelled: &AtomicBool) -> (Vec<PathBuf>, usize) {
        let mut sessions: BTreeSet<PathBuf> = locations.herdr.iter().cloned().collect();
        for root in &locations.herdr {
            if let Ok(dirs) = fs::read_dir(root.join("sessions")) {
                sessions.extend(
                    dirs.flatten()
                        .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
                        .map(|d| d.path()),
                );
            }
        }
        let mut endpoints: BTreeMap<PathBuf, Option<PathBuf>> = sessions
            .into_iter()
            .map(|p| (p.join("herdr.sock"), Some(p.join("session.json"))))
            .collect();
        if let Some(socket) = &locations.socket {
            endpoints.insert(
                socket.clone(),
                socket.parent().map(|p| p.join("session.json")),
            );
        }
        let mut paths = BTreeSet::new();
        let mut failures = 0;
        let deadline = Instant::now() + Duration::from_secs(6);
        for (socket, saved) in endpoints {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            // Windows named pipes do not appear in the filesystem.
            let named_pipe = cfg!(windows)
                && (socket.to_string_lossy().starts_with(r"\\.\pipe\")
                    || locations.socket.as_ref() == Some(&socket));
            let live = (named_pipe || socket.exists()) && Instant::now() < deadline;
            let result = if live {
                read_herdr_socket(
                    &socket,
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                    cancelled,
                )
                .ok()
            } else {
                None
            };
            if let Some(items) = result {
                // A successful poll is not new project activity. Use the saved
                // session's timestamp when available, never the scan time.
                let updated = saved
                    .as_ref()
                    .and_then(|p| fs::metadata(p).ok())
                    .and_then(|m| m.modified().ok());
                self.record_activity(&items, updated);
                paths.extend(items);
                continue;
            }
            if let Some(saved) = saved
                && saved.exists()
            {
                match self.cached(&saved, || read_herdr_saved(&saved)) {
                    Ok(items) => paths.extend(items),
                    Err(()) => failures += 1,
                }
            } else if live {
                failures += 1;
            }
        }
        (paths.into_iter().collect(), failures)
    }
}

fn read_session(path: &Path, agent: Agent) -> Result<Vec<PathBuf>, ()> {
    // Deliberately a metadata prefix, not a complete transcript snapshot.
    let opts = ReadOptions {
        include: EventKinds::META,
        // Claude may put a large file-history snapshot before its first cwd record.
        max_file_bytes: Some(if agent == Agent::ClaudeCode {
            4 * 1024 * 1024
        } else {
            1024 * 1024
        }),
        max_line_bytes: Some(if agent == Agent::ClaudeCode {
            2 * 1024 * 1024
        } else {
            256 * 1024
        }),
        tail: TailMode::AllowIncomplete,
        ..Default::default()
    };
    let mut reader = agent_sessions::read_from(
        agent,
        BufReader::new(File::open(path).map_err(|_| ())?),
        &opts,
    )
    .map_err(|_| ())?;
    for event in reader.by_ref() {
        match event {
            Ok(event) => {
                if let Event::Meta(meta) = event.value
                    && let Some(cwd) = meta.cwd
                {
                    let _prefix_summary = reader.finish();
                    return Ok(vec![PathBuf::from(cwd)]);
                }
            }
            Err(error) => {
                let _ = reader.finish();
                if agent == Agent::Codex
                    && matches!(error, agent_sessions::StreamError::Line { line_no: 1, .. })
                {
                    return legacy_codex_header(path);
                }
                return Err(());
            }
        }
    }
    let summary = reader.finish();
    if summary.is_complete() {
        Ok(Vec::new())
    } else {
        Err(())
    }
}
// Early Codex headers predate the typed envelope. Most contain no cwd at all;
// skip them normally instead of treating missing project metadata as corruption.
fn legacy_codex_header(path: &Path) -> Result<Vec<PathBuf>, ()> {
    use std::io::BufRead;
    #[derive(Deserialize)]
    struct Header {
        id: Option<String>,
        timestamp: Option<serde::de::IgnoredAny>,
        cwd: Option<PathBuf>,
        #[serde(rename = "type")]
        kind: Option<String>,
    }
    let mut bytes = Vec::new();
    BufReader::new(File::open(path).map_err(|_| ())?)
        .take(256 * 1024 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 256 * 1024 {
        return Err(());
    }
    let header: Header = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if header.kind.is_none() && header.id.is_some() && header.timestamp.is_some() {
        Ok(header.cwd.into_iter().collect())
    } else {
        Err(())
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ()> {
    let file = File::open(path).map_err(|_| ())?;
    if file.metadata().map_err(|_| ())?.len() > 8 * 1024 * 1024 {
        return Err(());
    }
    serde_json::from_reader(BufReader::new(file.take(8 * 1024 * 1024 + 1))).map_err(|_| ())
}
#[derive(Deserialize)]
struct ClaudeIndex {
    #[serde(default)]
    entries: Vec<ClaudeEntry>,
}
#[derive(Deserialize)]
struct ClaudeEntry {
    #[serde(rename = "projectPath")]
    path: Option<PathBuf>,
}
fn read_claude_index(path: &Path) -> Result<Vec<PathBuf>, ()> {
    Ok(read_json::<ClaudeIndex>(path)?
        .entries
        .into_iter()
        .filter_map(|e| e.path)
        .collect())
}
#[derive(Deserialize, Default)]
struct LiveSnapshot {
    #[serde(default)]
    workspaces: Vec<LiveWorkspace>,
    #[serde(default)]
    panes: Vec<LivePane>,
}
#[derive(Deserialize)]
struct LiveWorkspace {
    worktree: Option<LiveWorktree>,
}
#[derive(Deserialize)]
struct LiveWorktree {
    checkout_path: PathBuf,
}
#[derive(Deserialize)]
struct LivePane {
    cwd: Option<PathBuf>,
    foreground_cwd: Option<PathBuf>,
}
#[derive(Deserialize)]
struct Envelope {
    id: String,
    result: Option<LiveResult>,
}
#[derive(Deserialize)]
struct LiveResult {
    snapshot: LiveSnapshot,
}
#[derive(Deserialize)]
struct SavedSession {
    version: Option<u32>,
    #[serde(default)]
    workspaces: Vec<SavedWorkspace>,
}
#[derive(Deserialize)]
struct SavedWorkspace {
    identity_cwd: Option<PathBuf>,
    #[serde(default)]
    tabs: Vec<SavedTab>,
    #[serde(default)]
    panes: BTreeMap<String, SavedPane>,
}
#[derive(Deserialize)]
struct SavedTab {
    #[serde(default)]
    panes: BTreeMap<String, SavedPane>,
}
#[derive(Deserialize)]
struct SavedPane {
    cwd: Option<PathBuf>,
}
fn read_herdr_saved(path: &Path) -> Result<Vec<PathBuf>, ()> {
    let saved: SavedSession = read_json(path)?;
    if saved.version.is_some_and(|v| v > 3) {
        return Err(());
    }
    Ok(saved
        .workspaces
        .into_iter()
        .flat_map(|w| {
            w.identity_cwd
                .into_iter()
                .chain(w.panes.into_values().filter_map(|p| p.cwd))
                .chain(
                    w.tabs
                        .into_iter()
                        .flat_map(|t| t.panes.into_values().filter_map(|p| p.cwd)),
                )
                .collect::<Vec<_>>()
        })
        .collect())
}

#[cfg(unix)]
fn connect(path: &Path, _: Instant) -> io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}
#[cfg(windows)]
fn connect(path: &Path, deadline: Instant) -> io::Result<interprocess::local_socket::Stream> {
    use interprocess::{
        ConnectWaitMode,
        local_socket::{ConnectOptions, GenericNamespaced, prelude::*},
    };
    let stream = ConnectOptions::new()
        .name(
            path.to_string_lossy()
                .into_owned()
                .to_ns_name::<GenericNamespaced>()?,
        )
        .wait_mode(ConnectWaitMode::Timeout(
            deadline.saturating_duration_since(Instant::now()),
        ))
        .connect_sync()?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}
fn read_herdr_socket(
    path: &Path,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> io::Result<Vec<PathBuf>> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let mut stream = connect(path, deadline)?;
    let request = b"{\"id\":\"diffy-discover\",\"method\":\"session.snapshot\",\"params\":{}}\n";
    let mut sent = 0;
    let mut reply = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if sent < request.len() {
            match stream.write(&request[sent..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        } else {
            let mut chunk = [0; 8192];
            match stream.read(&mut chunk) {
                Ok(0) if cfg!(windows) => {}
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    reply.extend_from_slice(&chunk[..n]);
                    if reply.len() > 2 * 1024 * 1024 {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    if let Some(end) = reply.iter().position(|b| *b == b'\n') {
                        let envelope: Envelope = serde_json::from_slice(&reply[..end])
                            .map_err(|_| io::ErrorKind::InvalidData)?;
                        if envelope.id != "diffy-discover" {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                        let snapshot = envelope.result.ok_or(io::ErrorKind::InvalidData)?.snapshot;
                        return Ok(snapshot
                            .workspaces
                            .into_iter()
                            .filter_map(|w| w.worktree.map(|w| w.checkout_path))
                            .chain(
                                snapshot
                                    .panes
                                    .into_iter()
                                    .filter_map(|p| p.cwd.or(p.foreground_cwd)),
                            )
                            .collect());
                    }
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "diffy-discovery-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn write(&self, path: &str, value: &impl Serialize) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
            path
        }
        fn repo(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            assert!(
                Command::new("git")
                    .args(["init", "-q"])
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
            fs::canonicalize(path).unwrap()
        }
        fn locations(&self) -> Locations {
            Locations {
                herdr: vec![self.0.join("herdr")],
                socket: None,
                codex: Some(self.0.join("codex")),
                claude: Some(self.0.join("claude")),
                errors: [None, None, None],
            }
        }
        fn scanner(&self) -> Scanner {
            // Fixtures live in the host's temp directory. Give them their own
            // temporary root so ordinary fixture projects remain discoverable.
            Scanner {
                temporary_roots: TemporaryRoots::new([self.0.join("system-temp")]),
                ..Scanner::default()
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn codex(path: &Path) -> serde_json::Value {
        serde_json::json!({"type":"session_meta","payload":{"id":"fixture","cwd":path,"source":"cli"}})
    }
    #[test]
    fn default_temporary_roots_hide_platform_temp_and_unix_aliases() {
        let f = Fixture::new();
        let root = f.repo("openloop-eval/runs/app");
        f.write("codex/sessions/temp.jsonl", &codex(&root));
        assert!(
            Scanner::default()
                .scan(&f.locations(), [true; 3], &AtomicBool::new(false))
                .candidates
                .is_empty()
        );
        #[cfg(unix)]
        for alias in ["/tmp", "/var/tmp"] {
            let roots = TemporaryRoots::default();
            assert!(roots.contains(&Path::new(alias).join("evaluation/app")));
            if let Ok(canonical) = fs::canonicalize(alias) {
                assert!(roots.contains(&canonical.join("evaluation/app")));
            }
        }
    }
    #[test]
    fn temporary_checkouts_are_hidden_across_sources_and_cached_scans() {
        let f = Fixture::new();
        let temporary = f.repo("system-temp/openloop-eval/runs/app");
        let nested = temporary.join("src");
        fs::create_dir(&nested).unwrap();
        let ordinary = f.repo("projects/app");
        let similar = f.repo("system-temp-project/app");
        let named_tmp = f.repo("projects/tmp/app");
        f.write("herdr/session.json", &serde_json::json!({"version":3,"workspaces":[{"identity_cwd":nested,"tabs":[]},{"identity_cwd":ordinary,"tabs":[]}]}));
        for (name, path) in [
            ("temporary", &nested),
            ("ordinary", &ordinary),
            ("similar", &similar),
            ("named-tmp", &named_tmp),
        ] {
            f.write(&format!("codex/sessions/{name}.jsonl"), &codex(path));
            f.write(&format!("claude/projects/example/{name}.jsonl"), &serde_json::json!({"type":"user","cwd":path,"sessionId":name,"message":{"content":"ignored"}}));
        }
        let mut scanner = f.scanner();
        let cancel = AtomicBool::new(false);
        let first = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(
            first
                .candidates
                .iter()
                .map(|c| c.path.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([ordinary.clone(), similar, named_tmp])
        );
        assert_eq!(
            first
                .candidates
                .iter()
                .find(|c| c.path == ordinary)
                .unwrap()
                .sources,
            BTreeSet::from(Source::ALL)
        );
        assert!(
            scanner.roots.contains_key(&nested),
            "The excluded checkout's resolved root is cached too"
        );
        let reads = scanner.metadata_reads;
        assert_eq!(scanner.scan(&f.locations(), [true; 3], &cancel), first);
        assert_eq!(scanner.metadata_reads, reads);
    }
    #[cfg(unix)]
    #[test]
    fn temporary_filter_follows_checkout_and_temp_root_symlinks() {
        let f = Fixture::new();
        let temporary = f.repo("system-temp/app");
        let ordinary = f.repo("projects/app");
        let temp_alias = f.0.join("temp-alias");
        let checkout_alias = f.0.join("project-alias");
        let ordinary_alias = f.0.join("system-temp/ordinary-alias");
        std::os::unix::fs::symlink(f.0.join("system-temp"), &temp_alias).unwrap();
        std::os::unix::fs::symlink(&temporary, &checkout_alias).unwrap();
        std::os::unix::fs::symlink(&ordinary, &ordinary_alias).unwrap();
        f.write("codex/sessions/temp.jsonl", &codex(&checkout_alias));
        f.write("codex/sessions/ordinary.jsonl", &codex(&ordinary_alias));
        let mut scanner = Scanner {
            temporary_roots: TemporaryRoots::new([temp_alias]),
            ..Scanner::default()
        };
        let snapshot = scanner.scan(&f.locations(), [true; 3], &AtomicBool::new(false));
        assert_eq!(snapshot.candidates.len(), 1);
        assert_eq!(snapshot.candidates[0].path, ordinary);
    }
    #[test]
    fn discovery_merges_sources_and_nested_paths_and_reuses_metadata() {
        let f = Fixture::new();
        let root = f.repo("项目 with spaces");
        let nested = root.join("src");
        fs::create_dir(&nested).unwrap();
        f.write(
            "herdr/session.json",
            &serde_json::json!({"version":3,"workspaces":[{"identity_cwd":nested,"tabs":[]}]}),
        );
        f.write("codex/sessions/2026/rollout.jsonl", &codex(&root));
        f.write(
            "claude/projects/project/sessions-index.json",
            &serde_json::json!({"entries":[{"projectPath":root,"firstPrompt":"ignored"}]}),
        );
        let mut scanner = f.scanner();
        let cancel = AtomicBool::new(false);
        let snapshot = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(snapshot.candidates.len(), 1);
        assert_eq!(snapshot.candidates[0].path, root);
        assert_eq!(
            snapshot.candidates[0].sources,
            Source::ALL.into_iter().collect()
        );
        assert!(snapshot.errors.iter().all(Option::is_none));
        let reads = scanner.metadata_reads;
        assert_eq!(scanner.scan(&f.locations(), [true; 3], &cancel), snapshot);
        assert_eq!(
            scanner.metadata_reads, reads,
            "Unchanged files must not be parsed again"
        );
    }
    #[test]
    fn metadata_prefix_ignores_large_transcript_tail_and_unrelated_payload_errors() {
        let f = Fixture::new();
        let root = f.repo("repo");
        let path = f.write("codex/sessions/header.jsonl", &codex(&root));
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"\n").unwrap();
        file.write_all(&vec![b'x'; 2 * 1024 * 1024]).unwrap();
        assert_eq!(read_session(&path, Agent::Codex), Ok(vec![root.clone()]));
        let claude=f.write("claude/projects/example/session.jsonl", &serde_json::json!({"type":"user","cwd":root,"sessionId":"fixture","message":{"content":"ignored"}}));
        assert_eq!(read_session(&claude, Agent::ClaudeCode), Ok(vec![root]));
    }
    #[test]
    fn claude_cwd_after_large_file_history_snapshot_is_recovered() {
        let f = Fixture::new();
        let root = f.repo("repo");
        let path = f.write("claude/projects/example/session.jsonl", &serde_json::json!({"type":"file-history-snapshot", "snapshot":{"padding":"x".repeat(1500 * 1024)}}));
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "\n{}", serde_json::json!({"type":"user", "cwd":root,"sessionId":"fixture", "message":{"content":"ignored"}})).unwrap();
        assert_eq!(read_session(&path, Agent::ClaudeCode), Ok(vec![root]));
    }
    #[test]
    fn incomplete_sources_do_not_hide_valid_results_and_missing_roots_are_normal() {
        let f = Fixture::new();
        let root = f.repo("repo");
        f.write("codex/archived_sessions/good.jsonl", &codex(&root));
        let bad = f.0.join("codex/archived_sessions/bad.jsonl");
        fs::write(bad, b"{broken\n").unwrap();
        let snapshot = f
            .scanner()
            .scan(&f.locations(), [true; 3], &AtomicBool::new(false));
        assert_eq!(snapshot.candidates.len(), 1);
        assert!(snapshot.errors[1].is_some());
        assert!(snapshot.errors[0].is_none());
        assert!(snapshot.errors[2].is_none());
    }
    #[test]
    fn unavailable_and_non_git_paths_are_hidden() {
        let f = Fixture::new();
        let root = f.repo("repo");
        f.write("codex/sessions/a.jsonl", &codex(&root));
        fs::create_dir(f.0.join("not-git")).unwrap();
        f.write("codex/sessions/b.jsonl", &codex(&f.0.join("not-git")));
        f.write("codex/sessions/gone.jsonl", &codex(&f.0.join("gone")));
        let snapshot = f.scanner().scan(
            &f.locations(),
            [false, true, false],
            &AtomicBool::new(false),
        );
        assert_eq!(snapshot.candidates.len(), 1);
        assert_eq!(snapshot.candidates[0].path, root);
        assert!(snapshot.candidates[0].error.is_none());
    }
    #[test]
    fn linked_worktrees_remain_distinct() {
        let f = Fixture::new();
        let root = f.repo("repo");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "fixture"
                ])
                .status()
                .unwrap()
                .success()
        );
        let checkout = f.0.join("checkout");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["worktree", "add", "-qb", "fixture"])
                .arg(&checkout)
                .status()
                .unwrap()
                .success()
        );
        f.write("codex/sessions/a.jsonl", &codex(&root));
        f.write("codex/sessions/b.jsonl", &codex(&checkout));
        let snapshot = f.scanner().scan(
            &f.locations(),
            [false, true, false],
            &AtomicBool::new(false),
        );
        assert_eq!(snapshot.candidates.len(), 2);
        assert!(snapshot.candidates.iter().all(|c| c.error.is_none()));
    }
    #[cfg(unix)]
    #[test]
    fn live_herdr_reads_only_snapshot_and_ignores_unknown_fields() {
        use std::io::BufRead;
        use std::os::unix::net::UnixListener;
        let f = Fixture::new();
        let root = f.repo("repo");
        let socket = f.0.join("live.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let checkout = root.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "session.snapshot");
            writeln!(stream,"{}",serde_json::json!({"id":"diffy-discover","result":{"type":"session_snapshot","snapshot":{"version":"0.9.4","protocol":22,"agents":[{"ignored":"data"}],"workspaces":[{"worktree":{"checkout_path":checkout}}],"panes":[]}}})).unwrap();
        });
        let paths = read_herdr_socket(
            &socket,
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(paths, vec![root]);
        server.join().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn stalled_herdr_is_bounded_and_cancelled_reads_stop() {
        use std::os::unix::net::UnixListener;
        let f = Fixture::new();
        let socket = f.0.join("stall.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(150));
        });
        let start = Instant::now();
        let error = read_herdr_socket(
            &socket,
            start + Duration::from_millis(50),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_millis(140));
        server.join().unwrap();
        assert_eq!(
            read_herdr_socket(
                &socket,
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(true)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[test]
    fn unchanged_thousand_session_scan_is_cached() {
        let f = Fixture::new();
        let root = f.repo("repo");
        for n in 0..1000 {
            f.write(&format!("codex/sessions/{n}.jsonl"), &codex(&root));
        }
        let mut scanner = f.scanner();
        let cancel = AtomicBool::new(false);
        let first = scanner.scan(&f.locations(), [false, true, false], &cancel);
        assert_eq!(first.candidates.len(), 1);
        let reads = scanner.metadata_reads;
        let start = Instant::now();
        assert_eq!(
            scanner.scan(&f.locations(), [false, true, false], &cancel),
            first
        );
        assert_eq!(scanner.metadata_reads, reads);
        eprintln!("Cached 1000-session discovery: {:?}", start.elapsed());
    }
    fn modified(path: &Path, seconds: u64) -> SystemTime {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(time))
            .unwrap();
        time
    }
    #[test]
    fn activity_orders_across_sources_and_updates_cached_and_removed_metadata() {
        let f = Fixture::new();
        let old = f.repo("a-old");
        let new = f.repo("z-new");
        let old_session = f.write("codex/sessions/old.jsonl", &codex(&old));
        let new_session = f.write(
            "herdr/session.json",
            &serde_json::json!({"version":3,"workspaces":[{"identity_cwd":new,"tabs":[]}]}),
        );
        let newest_session = f.write("claude/projects/old/session.jsonl", &serde_json::json!({"type":"user","cwd":old,"sessionId":"fixture","message":{"content":"ignored"}}));
        modified(&old_session, 10);
        modified(&new_session, 20);
        let latest = modified(&newest_session, 30);
        let mut scanner = f.scanner();
        let cancel = AtomicBool::new(false);
        let scan = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(
            scan.candidates.iter().map(|c| &c.path).collect::<Vec<_>>(),
            vec![&old, &new]
        );
        assert_eq!(scan.candidates[0].updated_at, Some(latest));
        assert_eq!(
            scan.candidates[0].sources,
            BTreeSet::from([Source::Codex, Source::Claude])
        );
        let reads = scanner.metadata_reads;
        assert_eq!(scanner.scan(&f.locations(), [true; 3], &cancel), scan);
        assert_eq!(scanner.metadata_reads, reads);

        // Touch an existing session without changing its creation time or cwd.
        modified(&new_session, 40);
        let scan = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(scan.candidates[0].path, new);
        assert_eq!(scanner.metadata_reads, reads + 1);

        fs::remove_file(newest_session).unwrap();
        let scan = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(scan.candidates[1].path, old);
        assert_eq!(
            scan.candidates[1].updated_at,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10))
        );
        assert_eq!(scan.candidates[1].sources, BTreeSet::from([Source::Codex]));
        assert_eq!(
            scanner.metadata_reads,
            reads + 1,
            "Remaining metadata stays cached"
        );
    }
    #[test]
    fn equal_and_unknown_activity_use_name_then_path() {
        let candidate = |path: &str, updated_at| Candidate {
            path: path.into(),
            repository: None,
            error: None,
            sources: BTreeSet::new(),
            updated_at,
        };
        let known = Some(SystemTime::UNIX_EPOCH);
        let mut snapshot = Snapshot {
            candidates: vec![
                candidate("/a/unknown", None),
                candidate("/z/same", known),
                candidate("/a/same", known),
                candidate("/a/Alpha", known),
            ],
            errors: Default::default(),
        };
        snapshot.sort();
        assert_eq!(
            snapshot
                .candidates
                .iter()
                .map(|c| c.path.to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["/a/Alpha", "/a/same", "/z/same", "/a/unknown"]
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_and_nested_paths_merge_into_one_checkout() {
        let f = Fixture::new();
        let root = f.repo("repo");
        let nested = root.join("src");
        fs::create_dir(&nested).unwrap();
        let link = f.0.join("alias");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        f.write("codex/sessions/nested.jsonl", &codex(&nested));
        f.write("codex/sessions/alias.jsonl", &codex(&link));
        let snapshot = f
            .scanner()
            .scan(&f.locations(), [true; 3], &AtomicBool::new(false));
        assert_eq!(snapshot.candidates.len(), 1);
        assert_eq!(snapshot.candidates[0].path, root);
    }
    #[cfg(unix)]
    #[test]
    fn live_herdr_activity_uses_saved_metadata_and_never_poll_time() {
        use std::io::BufRead;
        use std::os::unix::net::UnixListener;
        let f = Fixture::new();
        let root = f.repo("repo");
        let saved = f.write(
            "herdr/session.json",
            &serde_json::json!({"version":3,"workspaces":[]}),
        );
        let time = modified(&saved, 10);
        let listener = UnixListener::bind(f.0.join("herdr/herdr.sock")).unwrap();
        let checkout = root.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                writeln!(stream, "{}", serde_json::json!({"id":"diffy-discover","result":{"snapshot":{"workspaces":[{"worktree":{"checkout_path":checkout}}]}}})).unwrap();
            }
        });
        let mut scanner = f.scanner();
        let cancel = AtomicBool::new(false);
        let snapshot = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(snapshot.candidates[0].path, root);
        assert_eq!(snapshot.candidates[0].updated_at, Some(time));
        assert_eq!(scanner.scan(&f.locations(), [true; 3], &cancel), snapshot);
        fs::remove_file(saved).unwrap();
        let snapshot = scanner.scan(&f.locations(), [true; 3], &cancel);
        assert_eq!(snapshot.candidates[0].updated_at, None);
        server.join().unwrap();
    }
    #[test]
    fn legacy_codex_headers_without_cwd_are_normal_skips() {
        let f = Fixture::new();
        let root = f.repo("repo");
        let no_path = f.write("legacy.jsonl", &serde_json::json!({"id":"old-session","timestamp":"2025-01-01","instructions":"ignored"}));
        assert_eq!(read_session(&no_path, Agent::Codex), Ok(Vec::new()));
        let with_path = f.write(
            "legacy-path.jsonl",
            &serde_json::json!({"id":"old-session","timestamp":"2025-01-01","cwd":root}),
        );
        assert_eq!(read_session(&with_path, Agent::Codex), Ok(vec![root]));
    }
}
