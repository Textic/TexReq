use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpClient {
    ClaudeDesktop,
    ClaudeCode,
    Antigravity,
    OpenAiCodex,
}

impl McpClient {
    pub const ALL: [Self; 4] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Antigravity,
        Self::OpenAiCodex,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop (Anthropic)",
            Self::ClaudeCode => "Claude Code CLI (Anthropic)",
            Self::Antigravity => "Antigravity (Google)",
            Self::OpenAiCodex => "Codex CLI (OpenAI)",
        }
    }

    pub fn format_label(self) -> &'static str {
        match self {
            Self::OpenAiCodex => "TOML",
            _ => "JSON",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpClientStatus {
    pub client: McpClient,
    pub config_path: Option<PathBuf>,
    pub installed: bool,
    pub matches_command: bool,
}

/// Resolves the active TexReq executable path cleanly (stripping Windows verbatim prefix if present).
pub fn resolve_texreq_executable() -> String {
    match std::env::current_exe() {
        Ok(path) => clean_exe_path(&path),
        Err(_) => "texreq".to_string(),
    }
}

fn clean_exe_path(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        return stripped.to_string();
    }
    raw
}

pub fn client_config_path(client: McpClient) -> Option<PathBuf> {
    match client {
        McpClient::ClaudeDesktop => dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json")),
        McpClient::ClaudeCode => dirs::home_dir().map(|h| h.join(".claude.json")),
        McpClient::Antigravity => {
            dirs::home_dir().map(|h| h.join(".gemini").join("config").join("mcp_config.json"))
        }
        McpClient::OpenAiCodex => {
            if let Some(codex_home) = std::env::var_os("CODEX_HOME")
                && !codex_home.is_empty()
            {
                return Some(PathBuf::from(codex_home).join("config.toml"));
            }
            dirs::home_dir().map(|h| h.join(".codex").join("config.toml"))
        }
    }
}

pub fn generic_mcp_json(command: &str) -> String {
    let doc = serde_json::json!({
        "mcpServers": {
            "texreq": {
                "command": command,
                "args": ["--mcp"]
            }
        }
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

pub fn inspect_all_clients(command: &str) -> Vec<McpClientStatus> {
    McpClient::ALL
        .into_iter()
        .map(|c| inspect_client(c, command))
        .collect()
}

pub fn inspect_client(client: McpClient, command: &str) -> McpClientStatus {
    let path = client_config_path(client);
    let Some(ref cfg_path) = path else {
        return McpClientStatus {
            client,
            config_path: None,
            installed: false,
            matches_command: false,
        };
    };

    inspect_client_at(client, cfg_path, command)
}

fn inspect_client_at(client: McpClient, cfg_path: &Path, command: &str) -> McpClientStatus {
    let Ok(raw) = fs::read_to_string(cfg_path) else {
        return McpClientStatus {
            client,
            config_path: Some(cfg_path.to_path_buf()),
            installed: false,
            matches_command: false,
        };
    };

    let (installed, matches_command) = match client {
        McpClient::OpenAiCodex => inspect_toml_entry(&raw, command),
        _ => inspect_json_entry(&raw, command),
    };

    McpClientStatus {
        client,
        config_path: Some(cfg_path.to_path_buf()),
        installed,
        matches_command,
    }
}

fn inspect_json_entry(raw: &str, command: &str) -> (bool, bool) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (false, false);
    };
    let Some(entry) = val.get("mcpServers").and_then(|m| m.get("texreq")) else {
        return (false, false);
    };
    let cmd_ok = entry.get("command").and_then(|v| v.as_str()) == Some(command);
    let args_ok = entry
        .get("args")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| arr.len() == 1 && arr[0].as_str() == Some("--mcp"));
    (true, cmd_ok && args_ok)
}

fn inspect_toml_entry(raw: &str, command: &str) -> (bool, bool) {
    let Ok(tbl) = toml::from_str::<toml::Table>(raw) else {
        return (false, false);
    };
    let Some(entry) = tbl
        .get("mcp_servers")
        .and_then(|v| v.as_table())
        .and_then(|m| m.get("texreq"))
        .and_then(|v| v.as_table())
    else {
        return (false, false);
    };
    let cmd_ok = entry.get("command").and_then(|v| v.as_str()) == Some(command);
    let args_ok = entry
        .get("args")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| arr.len() == 1 && arr[0].as_str() == Some("--mcp"));
    (true, cmd_ok && args_ok)
}

pub fn install_client(client: McpClient, command: &str) -> Result<String, String> {
    let path = client_config_path(client)
        .ok_or_else(|| format!("Could not resolve config directory for {}", client.display_name()))?;
    install_client_at(client, &path, command)?;
    Ok(format!(
        "Installed TexReq MCP in {} ({})",
        client.display_name(),
        path.to_string_lossy()
    ))
}

pub fn uninstall_client(client: McpClient) -> Result<String, String> {
    let path = client_config_path(client)
        .ok_or_else(|| format!("Could not resolve config directory for {}", client.display_name()))?;
    uninstall_client_at(client, &path)?;
    Ok(format!(
        "Removed TexReq MCP from {}",
        client.display_name()
    ))
}

fn install_client_at(client: McpClient, path: &Path, command: &str) -> Result<(), String> {
    match client {
        McpClient::OpenAiCodex => update_toml_file(path, Some(command)),
        _ => update_json_file(path, Some(command)),
    }
}

fn uninstall_client_at(client: McpClient, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    match client {
        McpClient::OpenAiCodex => update_toml_file(path, None),
        _ => update_json_file(path, None),
    }
}

fn update_json_file(path: &Path, command: Option<&str>) -> Result<(), String> {
    let mut root: serde_json::Value = if path.exists() {
        let raw = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("Expected JSON object at root of {}", path.display()))?;

    match command {
        Some(cmd) => {
            if !obj.contains_key("mcpServers") || !obj["mcpServers"].is_object() {
                obj.insert("mcpServers".to_string(), serde_json::json!({}));
            }
            if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                servers.insert(
                    "texreq".to_string(),
                    serde_json::json!({
                        "command": cmd,
                        "args": ["--mcp"]
                    }),
                );
            }
        }
        None => {
            if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                servers.remove("texreq");
            }
        }
    }

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize JSON: {e}"))?;
    write_atomic(path, format!("{serialized}\n").as_bytes())
}

fn update_toml_file(path: &Path, command: Option<&str>) -> Result<(), String> {
    let mut root: toml::Table = if path.exists() {
        let raw = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        if raw.trim().is_empty() {
            toml::Table::new()
        } else {
            toml::from_str(&raw).map_err(|e| format!("Invalid TOML in {}: {e}", path.display()))?
        }
    } else {
        toml::Table::new()
    };

    match command {
        Some(cmd) => {
            if !root.get("mcp_servers").is_some_and(|v| v.is_table()) {
                root.insert("mcp_servers".to_string(), toml::Value::Table(toml::Table::new()));
            }
            if let Some(servers) = root.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
                let mut entry = toml::Table::new();
                entry.insert("command".to_string(), toml::Value::String(cmd.to_string()));
                entry.insert(
                    "args".to_string(),
                    toml::Value::Array(vec![toml::Value::String("--mcp".to_string())]),
                );
                servers.insert("texreq".to_string(), toml::Value::Table(entry));
            }
        }
        None => {
            if let Some(servers) = root.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
                servers.remove("texreq");
            }
        }
    }

    let serialized =
        toml::to_string_pretty(&root).map_err(|e| format!("Failed to serialize TOML: {e}"))?;
    write_atomic(path, serialized.as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    let tmp = path.with_extension("texreq.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("Failed to write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("Failed to save {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_merges_json_without_clobbering_other_servers() {
        let dir = std::env::temp_dir().join(format!("texreq_mcp_json_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("mcp_config.json");
        fs::write(
            &cfg,
            r#"{"mcpServers":{"other":{"command":"node","args":["x.js"]}},"theme":"dark"}"#,
        )
        .unwrap();

        install_client_at(McpClient::Antigravity, &cfg, r"C:\Apps\texreq.exe").unwrap();
        let status = inspect_client_at(McpClient::Antigravity, &cfg, r"C:\Apps\texreq.exe");
        assert!(status.installed);
        assert!(status.matches_command);

        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dark");
        assert_eq!(saved["mcpServers"]["other"]["command"], "node");
        assert_eq!(saved["mcpServers"]["texreq"]["args"][0], "--mcp");

        uninstall_client_at(McpClient::Antigravity, &cfg).unwrap();
        let after = inspect_client_at(McpClient::Antigravity, &cfg, r"C:\Apps\texreq.exe");
        assert!(!after.installed);
        let saved_after: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(saved_after["mcpServers"]["other"]["command"], "node");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn installs_and_merges_toml_for_codex_cleanly() {
        let dir = std::env::temp_dir().join(format!("texreq_mcp_toml_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(
            &cfg,
            "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"uvx\"\n",
        )
        .unwrap();

        install_client_at(McpClient::OpenAiCodex, &cfg, "/usr/bin/texreq").unwrap();
        let status = inspect_client_at(McpClient::OpenAiCodex, &cfg, "/usr/bin/texreq");
        assert!(status.installed);
        assert!(status.matches_command);

        let raw = fs::read_to_string(&cfg).unwrap();
        assert!(raw.contains("model = \"gpt-5\""));
        assert!(raw.contains("[mcp_servers.other]"));

        uninstall_client_at(McpClient::OpenAiCodex, &cfg).unwrap();
        let after = inspect_client_at(McpClient::OpenAiCodex, &cfg, "/usr/bin/texreq");
        assert!(!after.installed);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn generates_valid_generic_json() {
        let txt = generic_mcp_json(r"C:\TexReq\texreq.exe");
        let parsed: serde_json::Value = serde_json::from_str(&txt).unwrap();
        assert_eq!(parsed["mcpServers"]["texreq"]["command"], r"C:\TexReq\texreq.exe");
        assert_eq!(parsed["mcpServers"]["texreq"]["args"][0], "--mcp");
    }
}
