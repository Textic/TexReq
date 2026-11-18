use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub workspace_dir: Option<String>,
    #[serde(default = "default_true")]
    pub auto_save: bool,
    #[serde(default)]
    pub default_accept_invalid_certs: bool,
    #[serde(default = "default_true")]
    pub default_follow_redirects: bool,
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,
    #[serde(default)]
    pub default_proxy_url: String,
    #[serde(default = "default_true")]
    pub auto_format_json: bool,
    #[serde(default = "default_true")]
    pub mcp_mask_secrets: bool,
    #[serde(default)]
    pub mcp_read_only: bool,
    #[serde(default)]
    pub linked_folders: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_timeout_secs() -> u64 {
    30
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            workspace_dir: None,
            auto_save: true,
            default_accept_invalid_certs: false,
            default_follow_redirects: true,
            default_timeout_secs: 30,
            default_proxy_url: String::new(),
            auto_format_json: true,
            mcp_mask_secrets: true,
            mcp_read_only: false,
            linked_folders: Vec::new(),
        }
    }
}
