use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    pub apps: Vec<AppEntry>,
    #[serde(skip)]
    path: PathBuf,
}

/// Which "category" instance this process is, derived purely from the exe's
/// own filename. The stock `onyx-launcher.exe` has no category (the
/// original shared app list). Any other filename - e.g. `Games.exe`, a copy
/// produced by the category-maker tool - is treated as its own independent
/// category with its own app list, config folder, and single-instance port.
pub fn category_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().to_string();
    if stem.eq_ignore_ascii_case("onyx-launcher") {
        None
    } else {
        Some(stem)
    }
}

pub fn config_dir(category: Option<&str>) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is not set");
    let root = PathBuf::from(base).join("OnyxLauncher");
    match category {
        Some(name) => root.join("categories").join(name),
        None => root,
    }
}

pub fn config_path(category: Option<&str>) -> PathBuf {
    config_dir(category).join("apps.json")
}

impl Config {
    pub fn load(category: Option<&str>) -> Self {
        let path = config_path(category);
        let mut config: Config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        config.path = path;
        config
    }

    pub fn save(&self) {
        if let Some(dir) = self.path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    pub fn add_app(&mut self, name: String, path: String) {
        if self.apps.iter().any(|a| a.path == path) {
            return;
        }
        self.apps.push(AppEntry { name, path });
        self.save();
    }

    pub fn remove_app(&mut self, path: &str) {
        self.apps.retain(|a| a.path != path);
        self.save();
    }
}
