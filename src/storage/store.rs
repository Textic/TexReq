use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::models::{
    AppSettings, CollectionFolder, Environment, HistoryEntry, default_collections,
    default_environments,
};

#[derive(Debug, Serialize, Deserialize)]
struct EnvironmentsFile {
    environments: Vec<Environment>,
}

pub struct LocalStore {
    config_dir: PathBuf,
    workspace_dir: PathBuf,
}

impl LocalStore {
    pub fn default_config_dir() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join("TexReq"))
            .unwrap_or_else(|| PathBuf::from("./data"))
    }

    pub fn from_settings() -> (Self, AppSettings) {
        Self::from_dir(Self::default_config_dir())
    }

    pub fn from_dir(config_dir: PathBuf) -> (Self, AppSettings) {
        let _ = fs::create_dir_all(&config_dir);

        let settings = load_settings_from(&config_dir);
        let workspace_dir = settings
            .workspace_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir.clone());
        let _ = fs::create_dir_all(&workspace_dir);

        (
            Self {
                config_dir,
                workspace_dir,
            },
            settings,
        )
    }

    #[cfg(test)]
    pub fn at_dir(base_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&base_dir);
        Self {
            config_dir: base_dir.clone(),
            workspace_dir: base_dir,
        }
    }

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    pub fn set_workspace_dir(&mut self, custom_dir: Option<PathBuf>) {
        self.workspace_dir = custom_dir.unwrap_or_else(|| self.config_dir.clone());
        let _ = fs::create_dir_all(&self.workspace_dir);
    }

    pub fn save_settings(&self, settings: &AppSettings) {
        let path = self.config_dir.join("settings.json");
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = write_atomic(&path, json.as_bytes());
        }
    }

    pub fn load_collections(&self) -> Vec<CollectionFolder> {
        let path = self.workspace_dir.join("collections.json");
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<Vec<CollectionFolder>>(&raw)
        {
            return parsed;
        }
        let defaults = default_collections();
        self.save_collections(&defaults);
        defaults
    }

    pub fn save_collections(&self, collections: &[CollectionFolder]) {
        let path = self.workspace_dir.join("collections.json");
        if let Ok(json) = serde_json::to_string_pretty(collections) {
            let _ = write_atomic(&path, json.as_bytes());
        }
    }

    pub fn load_linked_folder(&self, dir: &Path) -> Result<CollectionFolder, String> {
        if !dir.exists() {
            return Err(format!("Linked folder does not exist on disk: {}", dir.display()));
        }
        let col_file = dir.join("collection.json");
        if col_file.exists() {
            let raw = fs::read_to_string(&col_file)
                .map_err(|e| format!("Failed to read {}: {e}", col_file.display()))?;
            let mut folder = serde_json::from_str::<CollectionFolder>(&raw)
                .map_err(|e| format!("Invalid collection.json in {}: {e}", dir.display()))?;
            folder.disk_path = Some(dir.to_path_buf());
            return Ok(folder);
        }

        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Linked Folder");
        let mut folder = CollectionFolder::new(name);
        folder.disk_path = Some(dir.to_path_buf());
        self.save_linked_folder(&folder)?;
        Ok(folder)
    }

    pub fn save_linked_folder(&self, folder: &CollectionFolder) -> Result<(), String> {
        let Some(dir) = folder.disk_path.as_ref() else {
            return Ok(());
        };
        if !dir.exists() {
            return Err(format!(
                "Linked folder '{}' no longer exists on disk",
                dir.display()
            ));
        }
        let path = dir.join("collection.json");
        let json = serde_json::to_string_pretty(folder)
            .map_err(|e| format!("Failed to serialize folder: {e}"))?;
        write_atomic(&path, json.as_bytes())
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }

    pub fn load_all_collections(&self, linked_dirs: &[String]) -> Vec<CollectionFolder> {
        let mut all = self.load_collections();
        for f in &mut all {
            f.disk_path = None;
        }
        let mut seen = Vec::<PathBuf>::new();
        for raw in linked_dirs {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let dir = PathBuf::from(trimmed);
            let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            if seen.iter().any(|p| p == &canonical) {
                continue;
            }
            seen.push(canonical);
            if let Ok(folder) = self.load_linked_folder(&dir) {
                all.push(folder);
            }
        }
        all
    }

    pub fn save_all_collections(&self, collections: &[CollectionFolder]) {
        let personal: Vec<CollectionFolder> = collections
            .iter()
            .filter(|f| f.disk_path.is_none())
            .cloned()
            .collect();
        self.save_collections(&personal);
        for folder in collections.iter().filter(|f| f.disk_path.is_some()) {
            let _ = self.save_linked_folder(folder);
        }
    }

    pub fn load_history(&self) -> Vec<HistoryEntry> {
        let path = self.config_dir.join("history.json");
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<Vec<HistoryEntry>>(&raw)
        {
            return parsed;
        }
        Vec::new()
    }

    pub fn save_history(&self, history: &[HistoryEntry]) {
        let path = self.config_dir.join("history.json");
        let capped: Vec<&HistoryEntry> = history.iter().take(100).collect();
        if let Ok(json) = serde_json::to_string_pretty(&capped) {
            let _ = write_atomic(&path, json.as_bytes());
        }
    }

    pub fn load_environments(&self) -> Vec<Environment> {
        let path = self.workspace_dir.join("environments.toml");
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = toml::from_str::<EnvironmentsFile>(&raw)
        {
            return parsed.environments;
        }
        let defaults = default_environments();
        self.save_environments(&defaults);
        defaults
    }

    pub fn save_environments(&self, environments: &[Environment]) {
        let path = self.workspace_dir.join("environments.toml");
        let wrapper = EnvironmentsFile {
            environments: environments.to_vec(),
        };
        if let Ok(serialized) = toml::to_string_pretty(&wrapper) {
            let _ = write_atomic(&path, serialized.as_bytes());
        }
    }
}

fn load_settings_from(config_dir: &Path) -> AppSettings {
    let path = config_dir.join("settings.json");
    if let Ok(raw) = fs::read_to_string(&path)
        && let Ok(parsed) = serde_json::from_str::<AppSettings>(&raw)
    {
        return parsed;
    }
    let defaults = AppSettings::default();
    if let Ok(json) = serde_json::to_string_pretty(&defaults) {
        let _ = write_atomic(&path, json.as_bytes());
    }
    defaults
}

fn write_atomic(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_collections_history_and_environments() {
        let temp_dir = std::env::temp_dir().join(format!("texreq_test_{}", uuid::Uuid::new_v4()));
        let mut store = LocalStore::at_dir(temp_dir.clone());

        let cols = store.load_collections();
        assert!(cols.is_empty());
        let saved_cols = vec![CollectionFolder::new("API")];
        store.save_collections(&saved_cols);
        assert_eq!(store.load_collections().len(), 1);

        let envs = store.load_environments();
        assert!(envs.is_empty());
        let saved_envs = vec![Environment::new("Prod", Vec::new())];
        store.save_environments(&saved_envs);
        assert_eq!(store.load_environments().len(), 1);

        let hist = store.load_history();
        assert!(hist.is_empty());
        store.save_history(&hist);

        let settings = AppSettings {
            auto_save: false,
            default_accept_invalid_certs: true,
            ..AppSettings::default()
        };
        store.save_settings(&settings);
        let reloaded = load_settings_from(&temp_dir);
        assert!(!reloaded.auto_save);
        assert!(reloaded.default_accept_invalid_certs);

        store.set_workspace_dir(Some(temp_dir.join("ws")));
        assert!(store.workspace_dir().ends_with("ws"));

        let linked_dir = temp_dir.join("repo-api");
        fs::create_dir_all(&linked_dir).unwrap();
        let mut linked = store.load_linked_folder(&linked_dir).unwrap();
        assert_eq!(linked.name, "repo-api");
        linked.requests.push(crate::models::RequestItem::default());
        store.save_all_collections(&[CollectionFolder::new("Personal"), linked]);

        let linked_setting = vec![linked_dir.to_string_lossy().to_string()];
        let all = store.load_all_collections(&linked_setting);
        assert_eq!(all.len(), 2);
        assert!(all[0].disk_path.is_none());
        assert!(all[1].disk_path.is_some());
        assert_eq!(all[1].requests.len(), 1);
        assert_eq!(store.load_collections().len(), 1);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn does_not_recreate_deleted_linked_folder_on_load_or_save() {
        let temp_dir =
            std::env::temp_dir().join(format!("texreq_deleted_dir_{}", uuid::Uuid::new_v4()));
        let store = LocalStore::at_dir(temp_dir.clone());
        let linked_dir = temp_dir.join("vanished");
        fs::create_dir_all(&linked_dir).unwrap();

        let linked = store.load_linked_folder(&linked_dir).unwrap();
        assert!(linked_dir.join("collection.json").exists());

        fs::remove_dir_all(&linked_dir).unwrap();
        assert!(!linked_dir.exists());

        let reloaded = store.load_all_collections(&[linked_dir.to_string_lossy().to_string()]);
        assert!(reloaded.is_empty());
        assert!(!linked_dir.exists());

        let save_res = store.save_linked_folder(&linked);
        assert!(save_res.is_err());
        assert!(!linked_dir.exists());

        let _ = fs::remove_dir_all(temp_dir);
    }
}
