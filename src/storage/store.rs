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

        let _ = fs::remove_dir_all(temp_dir);
    }
}
