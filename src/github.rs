//! Read-only GitHub API access. Credentials stay inside the backend, never in saved state.
use crate::repository::{self, GroupSnapshot};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}
impl Repository {
    pub fn parse(remote: &str) -> Option<Self> {
        let path = if let Some(path) = remote.strip_prefix("git@github.com:") {
            path.to_owned()
        } else {
            let url = url::Url::parse(remote).ok()?;
            if url.host_str()? != "github.com"
                || !matches!(url.scheme(), "https" | "ssh")
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return None;
            }
            url.path().trim_start_matches('/').to_owned()
        };
        let path = path.trim_end_matches('/').trim_end_matches(".git");
        let (owner, name) = path.split_once('/')?;
        if [owner, name].iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        }) {
            return None;
        }
        Some(Self {
            owner: owner.into(),
            name: name.into(),
        })
    }
    pub fn label(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
    pub fn url(&self) -> String {
        format!("https://github.com/{}", self.label())
    }
    pub fn pr_url(&self, number: u64) -> String {
        format!("{}/pull/{number}", self.url())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub repository: Repository,
}
pub fn remotes(path: &Path) -> Result<Vec<Remote>, String> {
    let bytes = repository::git(path, &["remote", "-v"])?;
    let text = String::from_utf8(bytes).map_err(|_| "Couldn't read repository remotes.")?;
    let mut result = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(name), Some(url), Some("(fetch)")) = (parts.next(), parts.next(), parts.next())
            && let Some(repository) = Repository::parse(url)
        {
            result.push(Remote {
                name: name.into(),
                repository,
            });
        }
    }
    result.sort_by_key(|r| (r.name != "origin", r.name.clone()));
    Ok(result)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub draft: bool,
    pub base: String,
    pub head: String,
    pub base_sha: String,
    pub head_sha: String,
}
impl PullRequest {
    pub fn tooltip(&self) -> String {
        format!(
            "#{} {}{}\n{} · {} → {}",
            self.number,
            self.title,
            if self.draft { " · Draft" } else { "" },
            self.author,
            self.head,
            self.base
        )
    }
    pub fn cache_root(&self, repo: &Repository) -> PathBuf {
        // A cache namespace only; never passed to filesystem or Git operations.
        PathBuf::from(format!(
            "github://{}/{}/{}/{}/{}",
            repo.owner, repo.name, self.number, self.base_sha, self.head_sha
        ))
    }
}
pub struct Page {
    pub items: Vec<PullRequest>,
    pub more: bool,
}
pub struct PullDiff {
    pub pr: PullRequest,
    pub snapshot: GroupSnapshot,
}
pub trait Backend: Send + Sync {
    fn list(&self, repo: &Repository, page: u32) -> Result<Page, String>;
    fn diff(&self, repo: &Repository, number: u64) -> Result<PullDiff, String>;
}
pub struct OctocrabBackend;
fn runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("diffy-github")
                .enable_all()
                .build()
                .map_err(|_| "Couldn't start GitHub networking.".into())
        })
        .as_ref()
        .map_err(Clone::clone)
}
fn gh_program() -> Option<PathBuf> {
    which::which("gh").ok().or_else(|| {
        let mut candidates = vec![
            PathBuf::from("/opt/homebrew/bin/gh"),
            PathBuf::from("/usr/local/bin/gh"),
            PathBuf::from("/usr/bin/gh"),
        ];
        if let Some(home) = std::env::home_dir() {
            candidates.push(home.join(".local/bin/gh"));
        }
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(root).join("GitHub CLI/gh.exe"));
        }
        candidates.into_iter().find(|p| p.is_file())
    })
}
fn credential() -> Result<String, String> {
    let program =
        gh_program().ok_or("Install GitHub CLI, then run gh auth login to view pull requests.")?;
    let mut command = Command::new(program);
    command
        .args(["auth", "token", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Couldn't start GitHub CLI. Run gh auth login, then retry.")?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader
        .join()
        .ok()
        .and_then(Result::ok)
        .ok_or("Couldn't read GitHub credentials. Run gh auth login, then retry.")?;
    if !status.is_some_and(|s| s.success()) || bytes.len() > 64 * 1024 {
        return Err(
            "GitHub login unavailable. Run gh auth login --hostname github.com, then retry.".into(),
        );
    }
    String::from_utf8(bytes)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Run gh auth login --hostname github.com, then retry.".into())
}
fn client(token: String) -> Result<octocrab::Octocrab, String> {
    octocrab::Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(|_| "Couldn't initialize GitHub access.".into())
}
fn error(error: octocrab::Error) -> String {
    match error {
        octocrab::Error::GitHub { source, .. } => match source.status_code.as_u16() {
            401 => "GitHub login expired. Run gh auth login --hostname github.com, then retry.".into(),
            403 => "GitHub denied access or reached a rate limit. Check gh auth status and your repository permissions, then retry.".into(),
            429 => "GitHub's rate limit was reached. Wait for it to reset, then retry.".into(),
            404 => "Repository or pull request unavailable. Check the selected remote and your GitHub access.".into(),
            406 | 422 => "GitHub couldn't provide this diff. Open the pull request in GitHub to review it.".into(),
            _ => "GitHub couldn't complete the request. Retry or open the pull request in GitHub.".into(),
        },
        _ => "Couldn't reach GitHub. Check your connection and retry.".into(),
    }
}
fn pull(pr: octocrab::models::pulls::PullRequest) -> PullRequest {
    PullRequest {
        number: pr.number,
        title: pr.title.unwrap_or_default(),
        author: pr.user.map(|u| u.login).unwrap_or_default(),
        draft: pr.draft.unwrap_or(false),
        base: pr.base.ref_field,
        head: pr.head.ref_field,
        base_sha: pr.base.sha,
        head_sha: pr.head.sha,
    }
}
impl Backend for OctocrabBackend {
    fn list(&self, repo: &Repository, page: u32) -> Result<Page, String> {
        let runtime = runtime()?;
        let _entered = runtime.enter();
        let client = client(credential()?)?;
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                let page = client
                    .pulls(&repo.owner, &repo.name)
                    .list()
                    .state(octocrab::params::State::Open)
                    .sort(octocrab::params::pulls::Sort::Updated)
                    .direction(octocrab::params::Direction::Descending)
                    .per_page(100)
                    .page(page)
                    .send()
                    .await
                    .map_err(error)?;
                Ok(Page {
                    more: page.next.is_some(),
                    items: page.items.into_iter().map(pull).collect(),
                })
            })
            .await
            .map_err(|_| {
                "GitHub request timed out. Retry when your connection is available.".to_owned()
            })?
        })
    }
    fn diff(&self, repo: &Repository, number: u64) -> Result<PullDiff, String> {
        let runtime = runtime()?;
        let _entered = runtime.enter();
        let client = client(credential()?)?;
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                let handler = client.pulls(&repo.owner, &repo.name);
                let pr = handler.get(number).await.map_err(error)?;
                let patch = handler.get_diff(number).await.map_err(error)?;
                if patch.len() > 16 * 1024 * 1024 {
                    return Err(
                        "This PR is too large for the desktop preview. Open it in GitHub.".into(),
                    );
                }
                // Metadata and the diff are separate requests. Don't cache a newer patch
                // under old commit IDs if a force-push or base update happened in between.
                let latest = handler.get(number).await.map_err(error)?;
                if latest.base.sha != pr.base.sha || latest.head.sha != pr.head.sha {
                    return Err(
                        "This pull request changed while loading. Refresh to get the latest diff."
                            .into(),
                    );
                }
                let snapshot = GroupSnapshot::parse(patch)?;
                if snapshot
                    .files
                    .iter()
                    .map(|f| f.estimated_size_bytes())
                    .sum::<usize>()
                    > 32 * 1024 * 1024
                {
                    return Err(
                        "This PR is too large for the desktop preview. Open it in GitHub.".into(),
                    );
                }
                Ok(PullDiff {
                    pr: pull(pr),
                    snapshot,
                })
            })
            .await
            .map_err(|_| {
                "GitHub request timed out. Retry when your connection is available.".to_owned()
            })?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_client_initializes_on_a_background_thread() {
        std::thread::spawn(|| {
            let runtime = runtime().unwrap();
            let _entered = runtime.enter();
            // Building Octocrab starts a Tokio task even before the first request.
            assert!(client("fixture-token".into()).is_ok());
        })
        .join()
        .unwrap();
    }
    #[test]
    fn github_remotes_are_unambiguous_and_host_scoped() {
        for url in [
            "git@github.com:hitSlop/diffy.git",
            "https://github.com/hitSlop/diffy.git",
            "ssh://git@github.com/hitSlop/diffy",
            "https://github.com/hitSlop/diffy/",
        ] {
            assert_eq!(Repository::parse(url).unwrap().label(), "hitSlop/diffy");
        }
        for url in [
            "https://example.com/hitSlop/diffy",
            "https://github.com.evil.test/a/b",
            "git@other:org/repo.git",
            "https://github.com/a/b/more",
            "https://github.com/a/..",
            "https://github.com/a/b?token=secret",
        ] {
            assert!(Repository::parse(url).is_none(), "{url}");
        }
    }
}
