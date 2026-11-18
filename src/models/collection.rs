use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::RequestItem;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionFolder {
    pub id: String,
    pub name: String,
    pub expanded: bool,
    pub requests: Vec<RequestItem>,
    #[serde(default)]
    pub subfolders: Vec<CollectionFolder>,
    #[serde(skip)]
    pub disk_path: Option<PathBuf>,
}

impl CollectionFolder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            expanded: true,
            requests: Vec::new(),
            subfolders: Vec::new(),
            disk_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    pub request: RequestItem,
}

pub fn default_collections() -> Vec<CollectionFolder> {
    Vec::new()
}
