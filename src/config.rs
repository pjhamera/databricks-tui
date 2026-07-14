use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Preferences remembered across sessions; written whenever the user
/// changes one, best-effort.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Theme id in the same kebab-case form the --theme flag takes.
    pub theme: Option<String>,
    /// Chosen SQL warehouse per profile: profile → (id, name).
    #[serde(default)]
    pub warehouses: HashMap<String, (String, String)>,
    /// Pane ids in display order (missing ones append in default order).
    #[serde(default)]
    pub pane_order: Vec<String>,
    /// Pane ids the user has hidden.
    #[serde(default)]
    pub hidden_panes: Vec<String>,
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("databricks-tui")
            .join("config.json"),
    )
}

impl Config {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else {
            return;
        };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
            restrict(dir, 0o700);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&p, json);
            restrict(&p, 0o600);
        }
    }
}

/// Owner-only permissions on files the app writes (no-op off Unix).
pub fn restrict(path: &std::path::Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}
