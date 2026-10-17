//! Project preferences only: no source contents or Git mutations.
mod icons;
mod writer;
pub use icons::import_project_icon;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub use writer::Flush;
pub use writer::Writer;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProjectIcon {
    Emoji(String),
    Image(PathBuf),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedProject {
    pub path: PathBuf,
    pub color: u32,
    pub staged: bool,
    pub split: bool,
    pub selected: [Option<String>; 2],
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<ProjectIcon>,
}
/// Shared rail appearance and selection, with checkout review settings retained
/// in SavedProject. Optional at schema version 1 for migration from flat projects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedRepositoryProject {
    pub common_dir: PathBuf,
    pub checkouts: Vec<PathBuf>,
    pub active_checkout: Option<PathBuf>,
    pub name: String,
    pub color: u32,
    pub icon: Option<ProjectIcon>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
// Optional fields may be added within a schema version. Retain compatibility
// instead of rejecting all saved projects when an unfamiliar option appears.
pub struct SavedState {
    pub version: u32,
    pub projects: Vec<SavedProject>,
    #[serde(default)]
    pub repositories: Vec<SavedRepositoryProject>,
    pub active: Option<PathBuf>,
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    #[serde(default)]
    pub discovery: crate::discovery::Preferences,
    #[serde(default)]
    pub reader: ReaderSettings,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReaderSettings {
    pub font_size: f32,
    pub wrap: bool,
}
impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            font_size: 13.,
            wrap: true,
        }
    }
}
impl ReaderSettings {
    pub fn normalize(&mut self) {
        self.font_size = if self.font_size.is_finite() {
            self.font_size.clamp(10., 24.)
        } else {
            13.
        };
    }
}
impl Default for SavedState {
    fn default() -> Self {
        Self {
            version: 1,
            projects: Vec::new(),
            repositories: Vec::new(),
            active: None,
            sidebar_width: None,
            discovery: Default::default(),
            reader: Default::default(),
        }
    }
}
pub fn default_path() -> Option<PathBuf> {
    preference_path(
        std::env::var_os("DIFFY_STATE_PATH"),
        std::env::var_os("DIFFER_STATE_PATH"),
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().as_os_str().to_owned()),
    )
}
fn preference_path(
    current: Option<std::ffi::OsString>,
    legacy: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    current.or(legacy).map(PathBuf::from).or_else(|| {
        #[cfg(target_os = "macos")]
        {
            home.map(|home| {
                PathBuf::from(home).join("Library/Application Support/Diffy/state.json")
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = home;
            directories::ProjectDirs::from("com", "hitSlop", "Diffy")
                .map(|dirs| dirs.config_local_dir().join("state.json"))
        }
    })
}
pub fn load(path: &Path) -> Result<SavedState, String> {
    let migration_allowed = cfg!(target_os = "macos")
        && std::env::var_os("DIFFY_STATE_PATH").is_none()
        && std::env::var_os("DIFFER_STATE_PATH").is_none()
        && default_path().as_deref() == Some(path);
    let legacy = migration_allowed
        .then(|| {
            path.parent()
                .and_then(Path::parent)
                .map(|p| p.join("Differ/state.json"))
        })
        .flatten();
    load_with_legacy(path, legacy.as_deref())
}

/// Restore on the UI thread without performing a migration write. The application's
/// background writer creates the current preferences file after restoration.
pub fn load_for_restore(path: &Path) -> Result<SavedState, String> {
    if !cfg!(target_os = "macos")
        || path.exists()
        || std::env::var_os("DIFFY_STATE_PATH").is_some()
        || std::env::var_os("DIFFER_STATE_PATH").is_some()
        || default_path().as_deref() != Some(path)
    {
        return read(path);
    }
    if let Some(parent) = path.parent().and_then(Path::parent) {
        let legacy = parent.join("Differ/state.json");
        if legacy.exists() {
            return read(&legacy);
        }
    }
    read(path)
}
fn load_with_legacy(path: &Path, legacy: Option<&Path>) -> Result<SavedState, String> {
    if !path.exists()
        && let Some(legacy) = legacy
        && legacy.exists()
    {
        // Validate before copying, and never overwrite preferences another process created.
        let state = read(legacy)?;
        save_inner(path, &state, false)?;
    }
    read(path)
}
fn read(path: &Path) -> Result<SavedState, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SavedState::default()),
        Err(e) => return Err(format!("Couldn't read saved projects: {e}")),
    };
    let state: SavedState =
        serde_json::from_slice(&bytes).map_err(|e| format!("Couldn't read saved projects: {e}"))?;
    if state.version != 1 || state.projects.iter().any(|p| !p.path.is_absolute()) {
        return Err("Saved projects have an unsupported version or invalid path.".into());
    }
    Ok(state)
}
pub fn save(path: &Path, state: &SavedState) -> Result<(), String> {
    save_inner(path, state, true)
}
fn save_inner(path: &Path, state: &SavedState, replace: bool) -> Result<(), String> {
    let parent = path.parent().ok_or("Saved-project path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("Couldn't save projects: {e}"))?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!(".state-{}-{suffix}.tmp", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if replace {
            fs::rename(&tmp, path)
        } else {
            let linked = fs::hard_link(&tmp, path);
            fs::remove_file(&tmp)?;
            match linked {
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                result => result,
            }
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result.map_err(|e| format!("Couldn't save projects: {e}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_legacy_and_existing_preferences() {
        let root = std::env::temp_dir().join(format!("diffy-migrate-{}", std::process::id()));
        let legacy = root.join("Differ/state.json");
        let current = root.join("Diffy/state.json");
        let saved = SavedState {
            active: Some(PathBuf::from("/tmp/project")),
            ..Default::default()
        };
        save(&legacy, &saved).unwrap();
        assert_eq!(load_with_legacy(&current, Some(&legacy)).unwrap(), saved);
        assert_eq!(read(&legacy).unwrap(), saved);
        save(&current, &SavedState::default()).unwrap();
        assert_eq!(
            load_with_legacy(&current, Some(&legacy)).unwrap(),
            SavedState::default()
        );
        fs::remove_file(&current).unwrap();
        fs::write(&legacy, "invalid").unwrap();
        assert!(load_with_legacy(&current, Some(&legacy)).is_err());
        assert!(!current.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn overrides_take_precedence_and_bypass_migration() {
        assert_eq!(
            preference_path(
                Some("/new".into()),
                Some("/old".into()),
                Some("/home".into())
            ),
            Some(PathBuf::from("/new"))
        );
        assert_eq!(
            preference_path(None, Some("/old".into()), Some("/home".into())),
            Some(PathBuf::from("/old"))
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            preference_path(None, None, Some("/home".into())),
            Some(PathBuf::from(
                "/home/Library/Application Support/Diffy/state.json"
            ))
        );
        #[cfg(target_os = "macos")]
        assert_eq!(preference_path(None, None, None), None);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            preference_path(None, None, None),
            directories::ProjectDirs::from("com", "hitSlop", "Diffy")
                .map(|dirs| dirs.config_local_dir().join("state.json"))
        );
    }
    #[test]
    fn preferences_round_trip_and_removal_only_updates_state() {
        let root = std::env::temp_dir().join(format!(
            "diffy-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("preferences/state.json");
        assert!(load(&path).unwrap().projects.is_empty());
        fs::write(root.join("source.rs"), "precious bytes").unwrap();
        let mut state = SavedState {
            projects: vec![SavedProject {
                path: root.clone(),
                color: 0xff8175,
                staged: true,
                split: true,
                selected: [Some("a.rs".into()), Some("b.rs".into())],
                name: Some("My project".into()),
                icon: Some(ProjectIcon::Emoji("🪴".into())),
            }],
            active: Some(root.clone()),
            ..Default::default()
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), state);
        state.projects[0].icon = Some(ProjectIcon::Image(root.join("icon.png")));
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), state);
        state.projects.clear();
        state.active = None;
        save(&path, &state).unwrap();
        assert!(load(&path).unwrap().projects.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("source.rs")).unwrap(),
            "precious bytes"
        );
        fs::write(&path, "broken json").unwrap();
        assert!(load(&path).is_err());
        fs::write(&path, r#"{"version":2,"projects":[],"active":null}"#).unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn previous_preferences_accept_optional_project_appearance() {
        let saved: SavedState = serde_json::from_str(r#"{"version":1,"projects":[{"path":"/tmp/example","color":1,"staged":false,"split":true,"selected":[null,null]}],"active":null}"#).unwrap();
        assert_eq!(saved.projects[0].name, None);
        assert_eq!(saved.projects[0].icon, None);
    }
    #[test]
    fn retired_auto_import_flags_preserve_projects_and_exclusions() {
        let saved: SavedState = serde_json::from_value(serde_json::json!({
            "version": 1,
            "projects": [{"path":"/tmp/example","color":1,"staged":false,"split":true,"selected":[null,null]}],
            "active": "/tmp/example",
            "discovery": {
                "automatic": [true, true, true],
                "onboarding_completed": true,
                "excluded": ["/tmp/skipped"],
                "excluded_repositories": ["/tmp/removed/.git"]
            }
        })).unwrap();
        assert_eq!(saved.projects.len(), 1);
        assert_eq!(saved.active, Some(PathBuf::from("/tmp/example")));
        assert!(saved.discovery.onboarding_completed);
        assert!(saved.discovery.excluded.contains(Path::new("/tmp/skipped")));
        assert!(
            saved
                .discovery
                .excluded_repositories
                .contains(Path::new("/tmp/removed/.git"))
        );
        let serialized = serde_json::to_value(&saved).unwrap();
        assert!(serialized["discovery"].get("automatic").is_none());
        assert_eq!(
            serde_json::from_value::<SavedState>(serialized).unwrap(),
            saved
        );
    }
    #[test]
    fn optional_future_fields_do_not_hide_saved_projects() {
        let saved: SavedState = serde_json::from_str(r#"{"version":1,"projects":[{"path":"/tmp/example","color":1,"staged":false,"split":true,"selected":[null,null],"future_option":true}],"active":null,"future_reader_option":{"enabled":true}}"#).unwrap();
        assert_eq!(saved.projects.len(), 1);
        assert_eq!(saved.reader, ReaderSettings::default());
    }
}
