use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Instant, sleep_until};

use crate::models::{
    AppSettings, AuthConfig, BodyKind, CollectionFolder, Environment, HttpMethod, HttpResponseData,
    KeyValueRow, RequestItem, RequestKind, StreamDirection, StreamStatus,
};
use crate::net::WorkerEvent;
use crate::net::http_client::execute_http;
use crate::net::runtime::spawn_tokio;
use crate::net::snippets::{SnippetLang, generate_snippet};
use crate::net::ws_client::{WsCommand, run_ws_session};
use crate::storage::LocalStore;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const DEFAULT_MAX_BODY_BYTES: usize = 65_536;
const MAX_ALLOWED_BODY_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub enum McpStoreSource {
    Default,
    CustomDir(PathBuf),
}

impl McpStoreSource {
    fn open(&self) -> (LocalStore, AppSettings) {
        match self {
            Self::Default => LocalStore::from_settings(),
            Self::CustomDir(dir) => LocalStore::from_dir(dir.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RequestSummary {
    id: String,
    name: String,
    kind: String,
    method: String,
    url: String,
    folder_path: String,
}

#[derive(Debug, Serialize)]
struct FolderSummary {
    id: String,
    name: String,
    path: String,
    request_count: usize,
    subfolder_count: usize,
}

/// Runs TexReq's headless MCP server over standard input/output using newline-delimited JSON-RPC 2.0.
pub fn run_stdio_server() -> Result<(), String> {
    let source = resolve_cli_store_source();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read MCP stdin: {e}"))?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = handle_rpc_line(trimmed, &source) {
            let encoded = serde_json::to_string(&response)
                .map_err(|e| format!("Failed to encode MCP response: {e}"))?;
            writer
                .write_all(encoded.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
                .and_then(|_| writer.flush())
                .map_err(|e| format!("Failed to write MCP stdout: {e}"))?;
        }
    }

    Ok(())
}

fn resolve_cli_store_source() -> McpStoreSource {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(dir) = arg.strip_prefix("--config-dir=") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return McpStoreSource::CustomDir(PathBuf::from(trimmed));
            }
        } else if arg == "--config-dir"
            && let Some(dir) = args.next()
        {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return McpStoreSource::CustomDir(PathBuf::from(trimmed));
            }
        }
    }
    McpStoreSource::Default
}

/// Processes one JSON-RPC line and returns `Some(response)` for requests (never for notifications).
pub fn handle_rpc_line(line: &str, source: &McpStoreSource) -> Option<Value> {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(parsed) => parsed,
        Err(err) => {
            return Some(json_rpc_error(
                Value::Null,
                -32700,
                format!("Parse error: {err}"),
            ));
        }
    };

    if req.jsonrpc.as_deref().is_some_and(|v| v != "2.0") {
        let id = req.id.unwrap_or(Value::Null);
        return Some(json_rpc_error(id, -32600, "Unsupported jsonrpc version"));
    }

    // Notifications omit `id` (or set it to null for non-request notifications).
    let Some(id) = req.id.filter(|v| !v.is_null()) else {
        return None;
    };

    match req.method.as_str() {
        "initialize" => Some(json_rpc_ok(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": {
                    "name": "texreq-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )),
        "ping" => Some(json_rpc_ok(id, json!({}))),
        "tools/list" => Some(json_rpc_ok(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => {
            let Some(name) = req.params.get("name").and_then(Value::as_str) else {
                return Some(json_rpc_error(id, -32602, "Missing required tool name"));
            };
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            match call_tool(name, &args, source) {
                Ok(payload) => {
                    let text =
                        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
                    Some(json_rpc_ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        }),
                    ))
                }
                Err(ToolFailure::InvalidTool(msg)) => Some(json_rpc_error(id, -32601, msg)),
                Err(ToolFailure::InvalidParams(msg)) => Some(json_rpc_error(id, -32602, msg)),
                Err(ToolFailure::Execution(msg)) => Some(json_rpc_ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": msg }],
                        "isError": true
                    }),
                )),
            }
        }
        "resources/list" => Some(json_rpc_ok(id, json!({ "resources": resource_definitions() }))),
        "resources/read" => {
            let Some(uri) = req.params.get("uri").and_then(Value::as_str) else {
                return Some(json_rpc_error(id, -32602, "Missing resource uri"));
            };
            match read_resource(uri, source) {
                Ok(text) => Some(json_rpc_ok(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text
                        }]
                    }),
                )),
                Err(msg) => Some(json_rpc_error(id, -32602, msg)),
            }
        }
        other => Some(json_rpc_error(
            id,
            -32601,
            format!("Method not found: {other}"),
        )),
    }
}

enum ToolFailure {
    InvalidTool(String),
    InvalidParams(String),
    Execution(String),
}

fn json_rpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn json_rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_collections",
            "description": "List saved TexReq folders and API requests, with optional keyword filtering.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Optional substring filter over folder path, request name, or URL." }
                }
            }
        }),
        json!({
            "name": "get_request",
            "description": "Retrieve full details for a saved request by ID or name (credentials masked when enabled in settings).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Saved request UUID." },
                    "name": { "type": "string", "description": "Request name if ID is omitted." }
                }
            }
        }),
        json!({
            "name": "list_environments",
            "description": "List configured TexReq environments and show which one is currently active.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "get_environment_variables",
            "description": "Inspect variables in an environment (sensitive values masked by default).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "environment": { "type": "string", "description": "Environment ID or name (defaults to active environment)." }
                }
            }
        }),
        json!({
            "name": "list_history",
            "description": "List recent request execution history entries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Maximum entries to return (1-100, default 20)." }
                }
            }
        }),
        json!({
            "name": "run_saved_request",
            "description": "Execute a saved HTTP/SSE request using TexReq's HTTP engine and environment interpolation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Saved request UUID." },
                    "name": { "type": "string", "description": "Saved request name if ID is omitted." },
                    "environment": { "type": "string", "description": "Optional environment ID or name override." },
                    "include_headers": { "type": "boolean", "description": "Include request and response headers (default true)." },
                    "max_body_bytes": { "type": "integer", "description": "Maximum response body bytes to return (default 65536)." }
                }
            }
        }),
        json!({
            "name": "send_http_request",
            "description": "Execute an ad-hoc HTTP request without modifying saved collections.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "method": { "type": "string", "description": "HTTP method: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS." },
                    "url": { "type": "string", "description": "Target URL (supports {{VAR}} placeholders)." },
                    "headers": { "type": "object", "description": "Key-value map of HTTP headers." },
                    "params": { "type": "object", "description": "Key-value map of URL query parameters." },
                    "body": { "type": "string", "description": "Request body payload." },
                    "body_type": { "type": "string", "description": "none, json, or raw." },
                    "content_type": { "type": "string", "description": "Content-Type when body_type is raw." },
                    "environment": { "type": "string", "description": "Optional environment ID or name for {{VAR}} interpolation." },
                    "timeout_secs": { "type": "integer", "description": "Request timeout in seconds." },
                    "accept_invalid_certs": { "type": "boolean", "description": "Allow self-signed or invalid TLS certificates." },
                    "follow_redirects": { "type": "boolean", "description": "Follow HTTP redirects." },
                    "max_body_bytes": { "type": "integer", "description": "Maximum response body bytes to return (default 65536)." }
                },
                "required": ["method", "url"]
            }
        }),
        json!({
            "name": "test_websocket_once",
            "description": "Connect to a WebSocket endpoint, optionally send a message, and collect frames until timeout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "WebSocket URL (ws:// or wss://)." },
                    "request_id": { "type": "string", "description": "Optional saved WebSocket request ID instead of url." },
                    "headers": { "type": "object", "description": "Optional handshake headers." },
                    "send_message": { "type": "string", "description": "Optional initial text frame to send once connected." },
                    "timeout_secs": { "type": "integer", "description": "Wait window in seconds (1-30, default 5)." },
                    "max_messages": { "type": "integer", "description": "Maximum incoming frames to capture (1-50, default 5)." },
                    "environment": { "type": "string", "description": "Optional environment ID or name." }
                }
            }
        }),
        json!({
            "name": "save_request",
            "description": "Create or update a saved request in the workspace collection (disabled when read-only MCP mode is enabled).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional existing request ID to update in place." },
                    "name": { "type": "string", "description": "Display name for the request." },
                    "method": { "type": "string", "description": "HTTP method (default GET)." },
                    "kind": { "type": "string", "description": "http, websocket, or sse (default http)." },
                    "url": { "type": "string", "description": "Request URL." },
                    "folder_id": { "type": "string", "description": "Target folder ID." },
                    "folder_name": { "type": "string", "description": "Target folder name (creates if missing when folder_id is omitted)." },
                    "headers": { "type": "object", "description": "Optional headers object." },
                    "params": { "type": "object", "description": "Optional query params object." },
                    "body": { "type": "string", "description": "Optional request body text." },
                    "body_type": { "type": "string", "description": "none, json, or raw." },
                    "content_type": { "type": "string", "description": "Content-Type header for raw body." }
                },
                "required": ["name", "url"]
            }
        }),
        json!({
            "name": "create_folder",
            "description": "Create a collection folder at root or inside a parent folder (disabled when read-only MCP mode is enabled).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "New folder name." },
                    "parent_folder_id": { "type": "string", "description": "Optional parent folder UUID." }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "set_environment_variable",
            "description": "Set or update a variable in a TexReq environment (disabled when read-only MCP mode is enabled).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "environment": { "type": "string", "description": "Environment ID or name (defaults to active environment)." },
                    "key": { "type": "string", "description": "Variable key." },
                    "value": { "type": "string", "description": "Variable value." },
                    "enabled": { "type": "boolean", "description": "Whether the variable row is enabled (default true)." }
                },
                "required": ["key", "value"]
            }
        }),
        json!({
            "name": "generate_snippet",
            "description": "Generate a cURL, Rust reqwest, JS/TS fetch, or Python requests snippet from a saved or ad-hoc request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "language": { "type": "string", "description": "curl, rust, fetch, javascript, typescript, or python." },
                    "request_id": { "type": "string", "description": "Optional saved request ID." },
                    "method": { "type": "string", "description": "HTTP method when request_id is omitted (default GET)." },
                    "url": { "type": "string", "description": "URL when request_id is omitted." },
                    "headers": { "type": "object", "description": "Headers when request_id is omitted." },
                    "params": { "type": "object", "description": "Params when request_id is omitted." },
                    "body": { "type": "string", "description": "Body text when request_id is omitted." },
                    "body_type": { "type": "string", "description": "none, json, or raw." },
                    "content_type": { "type": "string", "description": "Content-Type for raw body." },
                    "environment": { "type": "string", "description": "Optional environment ID/name for non-secret variable resolution." }
                },
                "required": ["language"]
            }
        }),
    ]
}

fn resource_definitions() -> Vec<Value> {
    vec![
        json!({
            "uri": "texreq://collections",
            "name": "TexReq Collections",
            "description": "Tree summary of saved folders and requests in the active workspace.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "texreq://environments",
            "name": "TexReq Environments",
            "description": "Configured environments and variables (secrets masked per settings).",
            "mimeType": "application/json"
        }),
    ]
}

fn read_resource(uri: &str, source: &McpStoreSource) -> Result<String, String> {
    let (store, settings) = source.open();
    match uri {
        "texreq://collections" => {
            let collections = store.load_collections();
            let mut folders = Vec::new();
            let mut requests = Vec::new();
            collect_collection_tree(&collections, "", None, &mut folders, &mut requests);
            serde_json::to_string_pretty(&json!({
                "workspace_dir": store.workspace_dir().to_string_lossy(),
                "folders": folders,
                "requests": requests,
            }))
            .map_err(|e| format!("Serialization failed: {e}"))
        }
        "texreq://environments" => {
            let envs = store.load_environments();
            let masked_envs: Vec<Value> = envs
                .into_iter()
                .map(|env| serialize_environment(&env, settings.mcp_mask_secrets))
                .collect();
            serde_json::to_string_pretty(&json!({
                "mask_secrets": settings.mcp_mask_secrets,
                "environments": masked_envs,
            }))
            .map_err(|e| format!("Serialization failed: {e}"))
        }
        _ => Err(format!("Unknown resource URI: {uri}")),
    }
}

fn call_tool(name: &str, args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    match name {
        "list_collections" => tool_list_collections(args, source),
        "get_request" => tool_get_request(args, source),
        "list_environments" => tool_list_environments(source),
        "get_environment_variables" => tool_get_environment_variables(args, source),
        "list_history" => tool_list_history(args, source),
        "run_saved_request" => tool_run_saved_request(args, source),
        "send_http_request" => tool_send_http_request(args, source),
        "test_websocket_once" => tool_test_websocket_once(args, source),
        "save_request" => tool_save_request(args, source),
        "create_folder" => tool_create_folder(args, source),
        "set_environment_variable" => tool_set_environment_variable(args, source),
        "generate_snippet" => tool_generate_snippet(args, source),
        other => Err(ToolFailure::InvalidTool(format!("Unknown tool: {other}"))),
    }
}

fn tool_list_collections(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (store, _) = source.open();
    let collections = store.load_collections();
    let mut folders = Vec::new();
    let mut requests = Vec::new();
    collect_collection_tree(&collections, "", query, &mut folders, &mut requests);
    Ok(json!({
        "folders": folders,
        "requests": requests,
    }))
}

fn tool_get_request(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let id = args.get("id").and_then(Value::as_str).map(str::trim);
    let name = args.get("name").and_then(Value::as_str).map(str::trim);
    if id.is_none_or(str::is_empty) && name.is_none_or(str::is_empty) {
        return Err(ToolFailure::InvalidParams(
            "Provide either `id` or `name` to look up a saved request.".to_string(),
        ));
    }

    let (store, settings) = source.open();
    let collections = store.load_collections();
    let (folder_path, req) = find_saved_request(&collections, id, name)
        .ok_or_else(|| ToolFailure::Execution("Saved request not found.".to_string()))?;

    let mut item = req.clone();
    if settings.mcp_mask_secrets {
        mask_request_secrets(&mut item);
    }

    Ok(json!({
        "folder_path": folder_path,
        "request": item,
        "secrets_masked": settings.mcp_mask_secrets,
    }))
}

fn tool_list_environments(source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let (store, _) = source.open();
    let envs = store.load_environments();
    let list: Vec<Value> = envs
        .iter()
        .map(|env| {
            json!({
                "id": env.id,
                "name": env.name,
                "variable_count": env.variables.len(),
            })
        })
        .collect();

    Ok(json!({
        "environments": list,
    }))
}

fn tool_get_environment_variables(
    args: &Value,
    source: &McpStoreSource,
) -> Result<Value, ToolFailure> {
    let (store, settings) = source.open();
    let envs = store.load_environments();
    let selector = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let env = resolve_environment(&envs, selector).map_err(ToolFailure::Execution)?;

    Ok(serialize_environment(env, settings.mcp_mask_secrets))
}

fn tool_list_history(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;
    let (store, _) = source.open();
    let history = store.load_history();
    let entries: Vec<Value> = history
        .into_iter()
        .take(limit)
        .map(|h| {
            json!({
                "id": h.id,
                "timestamp": h.timestamp,
                "status": h.status,
                "elapsed_ms": h.elapsed_ms,
                "name": h.request.name,
                "kind": request_kind_label(h.request.kind),
                "method": request_method_label(h.request.kind),
                "url": h.request.url,
            })
        })
        .collect();
    Ok(json!({ "entries": entries }))
}

fn tool_run_saved_request(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let id = args.get("id").and_then(Value::as_str).map(str::trim);
    let name = args.get("name").and_then(Value::as_str).map(str::trim);
    if id.is_none_or(str::is_empty) && name.is_none_or(str::is_empty) {
        return Err(ToolFailure::InvalidParams(
            "Provide either `id` or `name` to execute a saved request.".to_string(),
        ));
    }

    let env_selector = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let include_headers = args
        .get("include_headers")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let max_body_bytes = parse_max_body_bytes(args);

    let (store, settings) = source.open();
    let collections = store.load_collections();
    let (_, req) = find_saved_request(&collections, id, name)
        .ok_or_else(|| ToolFailure::Execution("Saved request not found.".to_string()))?;

    if req.kind == RequestKind::WebSocket {
        return Err(ToolFailure::Execution(
            "This saved request uses WebSocket. Call `test_websocket_once` with `request_id` instead."
                .to_string(),
        ));
    }

    let envs = store.load_environments();
    let vars = resolve_env_vars_for_execution(&envs, env_selector).map_err(ToolFailure::Execution)?;

    let req_clone = req.clone();
    let resp = run_async_blocking(async move { execute_http(req_clone, vars).await })
        .map_err(ToolFailure::Execution)?;

    Ok(format_http_response(
        &resp,
        include_headers,
        max_body_bytes,
        settings.mcp_mask_secrets,
    ))
}

fn tool_send_http_request(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let method_str = args
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `method`.".to_string()))?;
    let method = HttpMethod::parse(method_str).ok_or_else(|| {
        ToolFailure::InvalidParams(format!("Unsupported HTTP method: {method_str}"))
    })?;
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `url`.".to_string()))?
        .to_string();

    let (store, settings) = source.open();
    let mut req = RequestItem::with_defaults(&settings);
    req.name = "MCP Ad-hoc Request".to_string();
    req.kind = RequestKind::Http(method);
    req.url = url;
    req.headers = parse_kv_object(args.get("headers"))?;
    req.params = parse_kv_object(args.get("params"))?;
    req.body = parse_body_from_args(args)?;

    if let Some(timeout) = args.get("timeout_secs").and_then(Value::as_u64) {
        req.network.timeout_secs = timeout.max(1);
    }
    if let Some(insecure) = args.get("accept_invalid_certs").and_then(Value::as_bool) {
        req.network.accept_invalid_certs = insecure;
    }
    if let Some(redirects) = args.get("follow_redirects").and_then(Value::as_bool) {
        req.network.follow_redirects = redirects;
    }

    let env_selector = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let envs = store.load_environments();
    let vars = resolve_env_vars_for_execution(&envs, env_selector).map_err(ToolFailure::Execution)?;

    let max_body_bytes = parse_max_body_bytes(args);
    let resp = run_async_blocking(async move { execute_http(req, vars).await })
        .map_err(ToolFailure::Execution)?;

    Ok(format_http_response(
        &resp,
        true,
        max_body_bytes,
        settings.mcp_mask_secrets,
    ))
}

fn tool_test_websocket_once(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let (store, settings) = source.open();
    let request_id = args
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut req = if let Some(id) = request_id {
        let collections = store.load_collections();
        let (_, saved) = find_saved_request(&collections, Some(id), None)
            .ok_or_else(|| ToolFailure::Execution("Saved WebSocket request not found.".to_string()))?;
        let mut cloned = saved.clone();
        cloned.kind = RequestKind::WebSocket;
        cloned
    } else {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolFailure::InvalidParams(
                    "Provide either `url` or `request_id` for `test_websocket_once`.".to_string(),
                )
            })?
            .to_string();
        let mut item = RequestItem::with_defaults(&settings);
        item.kind = RequestKind::WebSocket;
        item.url = url;
        item
    };

    let extra_headers = parse_kv_object(args.get("headers"))?;
    req.headers.extend(extra_headers);

    let send_message = args
        .get("send_message")
        .and_then(Value::as_str)
        .map(str::to_string);
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 30);
    let max_messages = args
        .get("max_messages")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 50) as usize;

    let env_selector = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let envs = store.load_environments();
    let vars = resolve_env_vars_for_execution(&envs, env_selector).map_err(ToolFailure::Execution)?;

    let summary = run_async_blocking(async move {
        run_ws_probe(req, vars, send_message, timeout_secs, max_messages).await
    });
    Ok(summary)
}

async fn run_ws_probe(
    req: RequestItem,
    vars: HashMap<String, String>,
    send_message: Option<String>,
    timeout_secs: u64,
    max_messages: usize,
) -> Value {
    use futures_util::StreamExt as _;

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (evt_tx, mut evt_rx) = futures_channel::mpsc::unbounded();

    let handle = spawn_tokio(async move {
        run_ws_session(req, vars, cmd_rx, evt_tx).await;
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut connected = false;
    let mut pending_send = send_message;
    let mut sent_payload: Option<String> = None;
    let mut status_events = Vec::new();
    let mut messages = Vec::new();
    let mut errors = Vec::new();
    let mut incoming_count = 0usize;

    loop {
        tokio::select! {
            _ = sleep_until(deadline) => break,
            evt = evt_rx.next() => {
                let Some(event) = evt else { break; };
                match event {
                    WorkerEvent::StreamStatus(status) => {
                        status_events.push(status.label().to_string());
                        if status == StreamStatus::Connected {
                            connected = true;
                            if let Some(msg) = pending_send.take() {
                                sent_payload = Some(msg.clone());
                                let _ = cmd_tx.send(WsCommand::SendText(msg));
                            }
                        } else if status == StreamStatus::Disconnected && connected {
                            break;
                        }
                    }
                    WorkerEvent::StreamMessage(entry) => {
                        let dir = match entry.direction {
                            StreamDirection::Incoming => {
                                incoming_count += 1;
                                "incoming"
                            }
                            StreamDirection::Outgoing => "outgoing",
                            StreamDirection::System => "system",
                        };
                        messages.push(json!({
                            "timestamp": entry.timestamp,
                            "direction": dir,
                            "event_name": entry.event_name,
                            "payload": entry.payload,
                        }));
                        if incoming_count >= max_messages {
                            break;
                        }
                    }
                    WorkerEvent::StreamError(err) => {
                        errors.push(err);
                    }
                    WorkerEvent::HttpFinished(_) => {}
                }
            }
        }
    }

    let _ = cmd_tx.send(WsCommand::Disconnect);
    let _ = tokio::time::timeout(Duration::from_millis(500), handle).await;

    json!({
        "connected": connected,
        "status_events": status_events,
        "sent_message": sent_payload,
        "messages": messages,
        "errors": errors,
    })
}

fn tool_save_request(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let (store, settings) = source.open();
    ensure_writable(&settings)?;

    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `name`.".to_string()))?
        .to_string();
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `url`.".to_string()))?
        .to_string();

    let kind = parse_request_kind(
        args.get("kind").and_then(Value::as_str),
        args.get("method").and_then(Value::as_str),
    )?;
    let headers = parse_kv_object(args.get("headers"))?;
    let params = parse_kv_object(args.get("params"))?;
    let body = parse_body_from_args(args)?;

    let mut collections = store.load_collections();
    if let Some(existing_id) = args
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some((folder_id, folder_name, req)) =
            find_request_mut_in_folders(&mut collections, existing_id)
        {
            req.name = name;
            req.kind = kind;
            req.url = url;
            if args.get("headers").is_some() {
                req.headers = headers;
            }
            if args.get("params").is_some() {
                req.params = params;
            }
            if args.get("body").is_some() || args.get("body_type").is_some() {
                req.body = body;
            }
            let saved_id = req.id.clone();
            store.save_collections(&collections);
            return Ok(json!({
                "saved": true,
                "created": false,
                "id": saved_id,
                "folder_id": folder_id,
                "folder_name": folder_name,
            }));
        }
        return Err(ToolFailure::Execution(format!(
            "No saved request exists with id '{existing_id}'."
        )));
    }

    let mut new_req = RequestItem::with_defaults(&settings);
    new_req.name = name;
    new_req.kind = kind;
    new_req.url = url;
    new_req.headers = headers;
    new_req.params = params;
    new_req.body = body;
    let saved_id = new_req.id.clone();

    let target_folder_id = args
        .get("folder_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let target_folder_name = args
        .get("folder_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let (folder_id, folder_name) = if let Some(fid) = target_folder_id {
        let folder = find_folder_by_id_mut(&mut collections, fid).ok_or_else(|| {
            ToolFailure::Execution(format!("Target folder_id '{fid}' was not found."))
        })?;
        folder.requests.push(new_req);
        (folder.id.clone(), folder.name.clone())
    } else {
        let desired_name = target_folder_name.unwrap_or("MCP Requests");
        if let Some(folder) = find_folder_by_name_mut(&mut collections, desired_name) {
            folder.requests.push(new_req);
            (folder.id.clone(), folder.name.clone())
        } else {
            let mut folder = CollectionFolder::new(desired_name);
            folder.requests.push(new_req);
            let res = (folder.id.clone(), folder.name.clone());
            collections.push(folder);
            res
        }
    };

    store.save_collections(&collections);
    Ok(json!({
        "saved": true,
        "created": true,
        "id": saved_id,
        "folder_id": folder_id,
        "folder_name": folder_name,
    }))
}

fn tool_create_folder(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let (store, settings) = source.open();
    ensure_writable(&settings)?;

    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `name`.".to_string()))?
        .to_string();
    let parent_id = args
        .get("parent_folder_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let folder = CollectionFolder::new(name);
    let created_id = folder.id.clone();
    let created_name = folder.name.clone();
    let mut collections = store.load_collections();

    if let Some(pid) = parent_id.as_deref() {
        let parent = find_folder_by_id_mut(&mut collections, pid).ok_or_else(|| {
            ToolFailure::Execution(format!("Parent folder '{pid}' was not found."))
        })?;
        parent.expanded = true;
        parent.subfolders.push(folder);
    } else {
        collections.push(folder);
    }

    store.save_collections(&collections);
    Ok(json!({
        "id": created_id,
        "name": created_name,
        "parent_folder_id": parent_id,
    }))
}

fn tool_set_environment_variable(
    args: &Value,
    source: &McpStoreSource,
) -> Result<Value, ToolFailure> {
    let (store, settings) = source.open();
    ensure_writable(&settings)?;

    let key = args
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `key`.".to_string()))?
        .to_string();
    let value = args
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `value`.".to_string()))?
        .to_string();
    let enabled = args.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let env_selector = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut envs = store.load_environments();
    let env_id = {
        let env = resolve_environment(&envs, env_selector).map_err(ToolFailure::Execution)?;
        env.id.clone()
    };

    let env = envs
        .iter_mut()
        .find(|e| e.id == env_id)
        .ok_or_else(|| ToolFailure::Execution("Target environment vanished.".to_string()))?;

    let updated = if let Some(row) = env.variables.iter_mut().find(|r| r.key.trim() == key) {
        row.value = value;
        row.enabled = enabled;
        true
    } else {
        let mut row = KeyValueRow::new(key.clone(), value);
        row.enabled = enabled;
        env.variables.push(row);
        false
    };

    let env_name = env.name.clone();
    store.save_environments(&envs);

    Ok(json!({
        "environment_id": env_id,
        "environment_name": env_name,
        "key": key,
        "updated": updated,
        "enabled": enabled,
        "sensitive": is_sensitive_key(&key),
    }))
}

fn tool_generate_snippet(args: &Value, source: &McpStoreSource) -> Result<Value, ToolFailure> {
    let lang_str = args
        .get("language")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolFailure::InvalidParams("Missing required `language`.".to_string()))?;
    let lang = parse_snippet_lang(lang_str)?;

    let (store, settings) = source.open();
    let req = if let Some(req_id) = args
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let collections = store.load_collections();
        let (_, saved) = find_saved_request(&collections, Some(req_id), None)
            .ok_or_else(|| ToolFailure::Execution("Saved request not found.".to_string()))?;
        let mut cloned = saved.clone();
        if settings.mcp_mask_secrets {
            mask_request_secrets(&mut cloned);
        }
        cloned
    } else {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolFailure::InvalidParams(
                    "Provide either `request_id` or `url` to generate a snippet.".to_string(),
                )
            })?
            .to_string();
        let kind = parse_request_kind(
            None,
            Some(args.get("method").and_then(Value::as_str).unwrap_or("GET")),
        )?;
        let mut item = RequestItem::with_defaults(&settings);
        item.kind = kind;
        item.url = url;
        item.headers = parse_kv_object(args.get("headers"))?;
        item.params = parse_kv_object(args.get("params"))?;
        item.body = parse_body_from_args(args)?;
        item
    };

    let vars = if let Some(env_sel) = args
        .get("environment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let envs = store.load_environments();
        let env = resolve_environment(&envs, Some(env_sel)).map_err(ToolFailure::Execution)?;
        build_env_map(env, settings.mcp_mask_secrets)
    } else {
        HashMap::new()
    };

    let code = generate_snippet(&req, &vars, lang);
    Ok(json!({
        "language": lang.label(),
        "editor_language": lang.editor_language(),
        "snippet": code,
    }))
}

fn ensure_writable(settings: &AppSettings) -> Result<(), ToolFailure> {
    if settings.mcp_read_only {
        Err(ToolFailure::Execution(
            "Read-only MCP mode is enabled in TexReq Settings. Disable 'Read-only mode' to allow workspace writes."
                .to_string(),
        ))
    } else {
        Ok(())
    }
}

fn parse_snippet_lang(raw: &str) -> Result<SnippetLang, ToolFailure> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "curl" | "bash" | "sh" => Ok(SnippetLang::Curl),
        "rust" | "reqwest" => Ok(SnippetLang::RustReqwest),
        "fetch" | "js" | "javascript" | "ts" | "typescript" => Ok(SnippetLang::TsFetch),
        "python" | "requests" | "py" => Ok(SnippetLang::PythonRequests),
        other => Err(ToolFailure::InvalidParams(format!(
            "Unsupported snippet language '{other}'. Expected curl, rust, fetch, or python."
        ))),
    }
}

fn parse_request_kind(kind: Option<&str>, method: Option<&str>) -> Result<RequestKind, ToolFailure> {
    match kind.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("websocket") | Some("ws") => Ok(RequestKind::WebSocket),
        Some("sse") => Ok(RequestKind::Sse),
        Some("http") | None => {
            let m_str = method.unwrap_or("GET");
            let parsed = HttpMethod::parse(m_str).ok_or_else(|| {
                ToolFailure::InvalidParams(format!("Unsupported HTTP method: {m_str}"))
            })?;
            Ok(RequestKind::Http(parsed))
        }
        Some(other) => Err(ToolFailure::InvalidParams(format!(
            "Unsupported request kind '{other}'. Expected http, websocket, or sse."
        ))),
    }
}

fn parse_kv_object(raw: Option<&Value>) -> Result<Vec<KeyValueRow>, ToolFailure> {
    let Some(val) = raw else {
        return Ok(Vec::new());
    };
    let Some(map) = val.as_object() else {
        return Err(ToolFailure::InvalidParams(
            "Expected a JSON object for key-value pairs.".to_string(),
        ));
    };
    Ok(map
        .iter()
        .map(|(k, v)| {
            let text = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            KeyValueRow::new(k, text)
        })
        .collect())
}

fn parse_body_from_args(args: &Value) -> Result<BodyKind, ToolFailure> {
    let body_type = args
        .get("body_type")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase());
    let body_text = args.get("body").and_then(Value::as_str);

    match body_type.as_deref() {
        Some("none") => Ok(BodyKind::None),
        Some("json") => Ok(BodyKind::Json(body_text.unwrap_or_default().to_string())),
        Some("raw") => Ok(BodyKind::Raw {
            content_type: args
                .get("content_type")
                .and_then(Value::as_str)
                .unwrap_or("text/plain")
                .to_string(),
            text: body_text.unwrap_or_default().to_string(),
        }),
        None => match body_text {
            Some(text) if !text.is_empty() => Ok(BodyKind::Json(text.to_string())),
            _ => Ok(BodyKind::None),
        },
        Some(other) => Err(ToolFailure::InvalidParams(format!(
            "Unsupported body_type '{other}'. Expected none, json, or raw."
        ))),
    }
}

fn parse_max_body_bytes(args: &Value) -> usize {
    args.get("max_body_bytes")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(512, MAX_ALLOWED_BODY_BYTES))
        .unwrap_or(DEFAULT_MAX_BODY_BYTES)
}

fn run_async_blocking<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    spawn_tokio(async move {
        let out = future.await;
        let _ = tx.send(out);
    });
    rx.recv()
        .expect("TexReq Tokio worker exited before completing MCP operation")
}

fn collect_collection_tree(
    folders: &[CollectionFolder],
    parent_path: &str,
    query: Option<&str>,
    out_folders: &mut Vec<FolderSummary>,
    out_requests: &mut Vec<RequestSummary>,
) {
    let q = query.map(|s| s.to_ascii_lowercase());
    for folder in folders {
        let path = if parent_path.is_empty() {
            folder.name.clone()
        } else {
            format!("{parent_path}/{}", folder.name)
        };
        out_folders.push(FolderSummary {
            id: folder.id.clone(),
            name: folder.name.clone(),
            path: path.clone(),
            request_count: folder.requests.len(),
            subfolder_count: folder.subfolders.len(),
        });

        for req in &folder.requests {
            let matches = q.as_ref().is_none_or(|needle| {
                req.name.to_ascii_lowercase().contains(needle)
                    || req.url.to_ascii_lowercase().contains(needle)
                    || path.to_ascii_lowercase().contains(needle)
            });
            if matches {
                out_requests.push(RequestSummary {
                    id: req.id.clone(),
                    name: req.name.clone(),
                    kind: request_kind_label(req.kind).to_string(),
                    method: request_method_label(req.kind).to_string(),
                    url: req.url.clone(),
                    folder_path: path.clone(),
                });
            }
        }

        collect_collection_tree(&folder.subfolders, &path, query, out_folders, out_requests);
    }
}

fn find_saved_request<'a>(
    folders: &'a [CollectionFolder],
    id: Option<&str>,
    name: Option<&str>,
) -> Option<(String, &'a RequestItem)> {
    if let Some(target_id) = id.filter(|s| !s.is_empty())
        && let Some(found) = find_request_recursive(folders, "", &|r| r.id == target_id)
    {
        return Some(found);
    }
    if let Some(target_name) = name.filter(|s| !s.is_empty()) {
        if let Some(found) = find_request_recursive(folders, "", &|r| r.name == target_name) {
            return Some(found);
        }
        return find_request_recursive(folders, "", &|r| {
            r.name.eq_ignore_ascii_case(target_name)
        });
    }
    None
}

fn find_request_recursive<'a, P>(
    folders: &'a [CollectionFolder],
    parent_path: &str,
    pred: &P,
) -> Option<(String, &'a RequestItem)>
where
    P: Fn(&RequestItem) -> bool,
{
    for folder in folders {
        let path = if parent_path.is_empty() {
            folder.name.clone()
        } else {
            format!("{parent_path}/{}", folder.name)
        };
        if let Some(req) = folder.requests.iter().find(|r| pred(r)) {
            return Some((path, req));
        }
        if let Some(found) = find_request_recursive(&folder.subfolders, &path, pred) {
            return Some(found);
        }
    }
    None
}

fn find_request_mut_in_folders<'a>(
    folders: &'a mut [CollectionFolder],
    id: &str,
) -> Option<(String, String, &'a mut RequestItem)> {
    for folder in folders {
        let fid = folder.id.clone();
        let fname = folder.name.clone();
        if let Some(req) = folder.requests.iter_mut().find(|r| r.id == id) {
            return Some((fid, fname, req));
        }
        if let Some(found) = find_request_mut_in_folders(&mut folder.subfolders, id) {
            return Some(found);
        }
    }
    None
}

fn find_folder_by_id_mut<'a>(
    folders: &'a mut [CollectionFolder],
    id: &str,
) -> Option<&'a mut CollectionFolder> {
    for folder in folders {
        if folder.id == id {
            return Some(folder);
        }
        if let Some(found) = find_folder_by_id_mut(&mut folder.subfolders, id) {
            return Some(found);
        }
    }
    None
}

fn find_folder_by_name_mut<'a>(
    folders: &'a mut [CollectionFolder],
    name: &str,
) -> Option<&'a mut CollectionFolder> {
    for folder in folders {
        if folder.name.eq_ignore_ascii_case(name) {
            return Some(folder);
        }
        if let Some(found) = find_folder_by_name_mut(&mut folder.subfolders, name) {
            return Some(found);
        }
    }
    None
}

fn resolve_environment<'a>(
    envs: &'a [Environment],
    selector: Option<&str>,
) -> Result<&'a Environment, String> {
    if let Some(target) = selector {
        return envs
            .iter()
            .find(|e| e.id == target)
            .or_else(|| envs.iter().find(|e| e.name.eq_ignore_ascii_case(target)))
            .ok_or_else(|| format!("Environment '{target}' not found."));
    }
    match envs {
        [] => Err("No environments are configured.".to_string()),
        [single] => Ok(single),
        _ => Err(
            "Multiple environments exist; specify `environment` (ID or name).".to_string(),
        ),
    }
}

fn resolve_env_vars_for_execution(
    envs: &[Environment],
    selector: Option<&str>,
) -> Result<HashMap<String, String>, String> {
    if selector.is_none() {
        return Ok(match envs {
            [single] => build_env_map(single, false),
            _ => HashMap::new(),
        });
    }
    resolve_environment(envs, selector).map(|env| build_env_map(env, false))
}

fn build_env_map(env: &Environment, mask_secrets: bool) -> HashMap<String, String> {
    env.variables
        .iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
        .map(|r| {
            let key = r.key.trim().to_string();
            let val = if mask_secrets && is_sensitive_key(&key) {
                mask_secret(&r.value)
            } else {
                r.value.clone()
            };
            (key, val)
        })
        .collect()
}

fn serialize_environment(env: &Environment, mask_secrets: bool) -> Value {
    let vars: Vec<Value> = env
        .variables
        .iter()
        .map(|row| {
            let sensitive = mask_secrets && is_sensitive_key(&row.key);
            let value = if sensitive {
                mask_secret(&row.value)
            } else {
                row.value.clone()
            };
            json!({
                "key": row.key,
                "value": value,
                "enabled": row.enabled,
                "masked": sensitive,
            })
        })
        .collect();

    json!({
        "id": env.id,
        "name": env.name,
        "variables": vars,
    })
}

fn mask_request_secrets(req: &mut RequestItem) {
    match &mut req.auth {
        AuthConfig::None => {}
        AuthConfig::Bearer { token } => *token = mask_secret(token),
        AuthConfig::Basic { password, .. } => *password = mask_secret(password),
        AuthConfig::ApiKey { value, .. } => *value = mask_secret(value),
    }
    for row in &mut req.headers {
        if is_sensitive_key(&row.key) {
            row.value = mask_secret(&row.value);
        }
    }
}

fn format_http_response(
    resp: &HttpResponseData,
    include_headers: bool,
    max_body_bytes: usize,
    mask_secrets: bool,
) -> Value {
    let source_body = if resp.pretty_body.is_empty() {
        &resp.raw_body
    } else {
        &resp.pretty_body
    };
    let (body, body_truncated) = truncate_utf8(source_body, max_body_bytes);

    let mut out = json!({
        "status": resp.status,
        "status_text": resp.status_text,
        "final_url": resp.final_url,
        "elapsed_ms": resp.elapsed_ms,
        "ttfb_ms": resp.ttfb_ms,
        "download_ms": resp.download_ms,
        "size_bytes": resp.size_bytes,
        "detected_language": resp.detected_language,
        "tls_info": resp.tls_info,
        "captured_vars": resp.captured_vars,
        "body": body,
        "body_truncated": body_truncated,
    });

    if include_headers {
        let resp_headers = mask_header_pairs(&resp.headers, mask_secrets);
        let sent_headers = mask_header_pairs(&resp.sent_headers, mask_secrets);
        let cookies = mask_header_pairs(&resp.cookies, mask_secrets);
        if let Some(obj) = out.as_object_mut() {
            obj.insert("headers".to_string(), json!(resp_headers));
            obj.insert("sent_headers".to_string(), json!(sent_headers));
            obj.insert("cookies".to_string(), json!(cookies));
        }
    }

    out
}

fn mask_header_pairs(pairs: &[(String, String)], mask_secrets: bool) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| {
            if mask_secrets && is_sensitive_key(k) {
                (k.clone(), mask_secret(v))
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    const PATTERNS: [&str; 13] = [
        "token",
        "secret",
        "password",
        "passwd",
        "pwd",
        "api_key",
        "apikey",
        "x-api-key",
        "auth",
        "bearer",
        "cookie",
        "credential",
        "private",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

pub fn mask_secret(val: &str) -> String {
    if val.is_empty() {
        return String::new();
    }
    let count = val.chars().count();
    if count <= 6 {
        return "***".to_string();
    }
    let prefix: String = val.chars().take(2).collect();
    let suffix: String = val.chars().skip(count - 2).collect();
    format!("{prefix}***{suffix}")
}

fn truncate_utf8(input: &str, max_bytes: usize) -> (String, bool) {
    if input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_string(), true)
}

fn request_kind_label(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Http(_) => "http",
        RequestKind::WebSocket => "websocket",
        RequestKind::Sse => "sse",
    }
}

fn request_method_label(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Http(m) => m.as_str(),
        RequestKind::WebSocket => "WS",
        RequestKind::Sse => "SSE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("texreq_mcp_srv_{name}_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn seed_workspace(dir: &PathBuf, settings: &AppSettings) -> (CollectionFolder, Environment) {
        fs::write(
            dir.join("settings.json"),
            serde_json::to_string_pretty(settings).unwrap(),
        )
        .unwrap();
        let (store, _) = LocalStore::from_dir(dir.clone());

        let mut req = RequestItem::with_defaults(settings);
        req.name = "Get Secret Profile".to_string();
        req.url = "{{BASE_URL}}/profile".to_string();
        req.auth = AuthConfig::Bearer {
            token: "secret-auth-token-9876".to_string(),
        };
        req.headers
            .push(KeyValueRow::new("X-Api-Key", "super-secret-api-key"));

        let mut folder = CollectionFolder::new("Auth");
        folder.requests.push(req);
        store.save_collections(std::slice::from_ref(&folder));

        let env = Environment::new(
            "Local",
            vec![
                KeyValueRow::new("BASE_URL", "http://127.0.0.1:8080"),
                KeyValueRow::new("API_TOKEN", "live_secret_value_99"),
            ],
        );
        store.save_environments(std::slice::from_ref(&env));
        (folder, env)
    }

    #[test]
    fn handles_initialize_ping_and_tools_list() {
        let dir = temp_dir("init");
        let source = McpStoreSource::CustomDir(dir.clone());

        let init = handle_rpc_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &source,
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "texreq-mcp");

        // Notification yields no output frame
        assert!(
            handle_rpc_line(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &source
            )
            .is_none()
        );

        let list =
            handle_rpc_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &source).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 12);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn masks_secrets_in_env_and_saved_requests() {
        let dir = temp_dir("mask");
        let mut settings = AppSettings::default();
        settings.mcp_mask_secrets = true;
        let (folder, env) = seed_workspace(&dir, &settings);
        let req_id = &folder.requests[0].id;
        let source = McpStoreSource::CustomDir(dir.clone());

        let call = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "get_environment_variables",
                "arguments": { "environment": env.name }
            }
        });
        let resp = handle_rpc_line(&call.to_string(), &source).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        let vars = parsed["variables"].as_array().unwrap();
        assert_eq!(vars[0]["key"], "BASE_URL");
        assert_eq!(vars[0]["value"], "http://127.0.0.1:8080");
        assert_eq!(vars[1]["key"], "API_TOKEN");
        assert_ne!(vars[1]["value"], "live_secret_value_99");
        assert_eq!(vars[1]["masked"], true);

        let get_req = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "get_request",
                "arguments": { "id": req_id }
            }
        });
        let resp_req = handle_rpc_line(&get_req.to_string(), &source).unwrap();
        let text_req = resp_req["result"]["content"][0]["text"].as_str().unwrap();
        assert!(!text_req.contains("secret-auth-token-9876"));
        assert!(!text_req.contains("super-secret-api-key"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocks_write_tools_when_read_only_enabled() {
        let dir = temp_dir("readonly");
        let mut settings = AppSettings::default();
        settings.mcp_read_only = true;
        let (_, _) = seed_workspace(&dir, &settings);
        let source = McpStoreSource::CustomDir(dir.clone());

        let write_call = json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "create_folder",
                "arguments": { "name": "Blocked" }
            }
        });
        let resp = handle_rpc_line(&write_call.to_string(), &source).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        let msg = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains("Read-only MCP mode"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn executes_http_and_saved_requests_against_loopback() {
        let addr = run_async_blocking(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let bound = listener.local_addr().unwrap();
            spawn_tokio(async move {
                for _ in 0..2 {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf).await;
                        let body = r#"{"ok":true,"source":"loopback"}"#;
                        let reply = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(reply.as_bytes()).await;
                    }
                }
            });
            bound
        });

        let dir = temp_dir("http_loopback");
        let settings = AppSettings::default();
        let (store, _) = LocalStore::from_dir(dir.clone());
        let env = Environment::new(
            "TestEnv",
            vec![KeyValueRow::new("BASE_URL", format!("http://{addr}"))],
        );
        store.save_environments(&[env]);

        let mut saved_req = RequestItem::with_defaults(&settings);
        saved_req.name = "Health".to_string();
        saved_req.url = "{{BASE_URL}}/health".to_string();
        let saved_id = saved_req.id.clone();
        let mut folder = CollectionFolder::new("System");
        folder.requests.push(saved_req);
        store.save_collections(&[folder]);

        let source = McpStoreSource::CustomDir(dir.clone());
        let saved_call = json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "tools/call",
            "params": {
                "name": "run_saved_request",
                "arguments": { "id": saved_id, "environment": "TestEnv" }
            }
        });
        let saved_resp = handle_rpc_line(&saved_call.to_string(), &source).unwrap();
        assert_eq!(saved_resp["result"]["isError"], false);
        let body_text = saved_resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(body_text).unwrap();
        assert_eq!(parsed["status"], 200);
        assert!(parsed["body"].as_str().unwrap().contains("loopback"));

        let adhoc_call = json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "tools/call",
            "params": {
                "name": "send_http_request",
                "arguments": {
                    "method": "POST",
                    "url": format!("http://{addr}/submit"),
                    "body_type": "json",
                    "body": "{\"ping\":1}"
                }
            }
        });
        let adhoc_resp = handle_rpc_line(&adhoc_call.to_string(), &source).unwrap();
        assert_eq!(adhoc_resp["result"]["isError"], false);
        let _ = fs::remove_dir_all(&dir);
    }
}
