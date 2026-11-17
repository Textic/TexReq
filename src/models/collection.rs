use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{BodyKind, CaptureRule, HttpMethod, KeyValueRow, RequestItem, RequestKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionFolder {
    pub id: String,
    pub name: String,
    pub expanded: bool,
    pub requests: Vec<RequestItem>,
    #[serde(default)]
    pub subfolders: Vec<CollectionFolder>,
}

impl CollectionFolder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            expanded: true,
            requests: Vec::new(),
            subfolders: Vec::new(),
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

pub fn default_demo_collections() -> Vec<CollectionFolder> {
    let mut rest_folder = CollectionFolder::new("REST Demo (JSONPlaceholder)");

    let mut get_users = RequestItem::default();
    get_users.name = "List Users".to_string();
    get_users.kind = RequestKind::Http(HttpMethod::Get);
    get_users.url = "{{api_base}}/users".to_string();
    get_users.params = vec![KeyValueRow::new("_limit", "5")];
    get_users.captures = vec![CaptureRule::new("first_user_id", "$[0].id")];

    let mut post_item = RequestItem::default();
    post_item.name = "Create Post".to_string();
    post_item.kind = RequestKind::Http(HttpMethod::Post);
    post_item.url = "{{api_base}}/posts".to_string();
    post_item.headers = vec![KeyValueRow::new("Content-Type", "application/json")];
    post_item.body = BodyKind::Json(
        "{\n  \"title\": \"Hello from TexReq\",\n  \"body\": \"Fast flat-dark API client\",\n  \"userId\": 1\n}"
            .to_string(),
    );

    let mut patch_item = RequestItem::default();
    patch_item.name = "Partial Update (PATCH)".to_string();
    patch_item.kind = RequestKind::Http(HttpMethod::Patch);
    patch_item.url = "{{api_base}}/posts/1".to_string();
    patch_item.headers = vec![KeyValueRow::new("Content-Type", "application/json")];
    patch_item.body =
        BodyKind::Json("{\n  \"title\": \"Updated title via PATCH\"\n}".to_string());

    rest_folder.requests = vec![get_users, post_item, patch_item];

    let mut realtime_folder = CollectionFolder::new("Realtime & GraphQL");

    let mut ws_echo = RequestItem::default();
    ws_echo.name = "WebSocket Echo".to_string();
    ws_echo.kind = RequestKind::WebSocket;
    ws_echo.url = "wss://echo.websocket.events".to_string();
    ws_echo.ws_templates = vec![
        "{\"type\":\"ping\",\"source\":\"TexReq\"}".to_string(),
        "Hello WebSocket!".to_string(),
    ];

    let mut sse_stream = RequestItem::default();
    sse_stream.name = "SSE Stream Demo".to_string();
    sse_stream.kind = RequestKind::Sse;
    sse_stream.url = "https://sse.dev/test".to_string();
    sse_stream.headers = vec![KeyValueRow::new("Accept", "text/event-stream")];

    let mut gql_req = RequestItem::default();
    gql_req.name = "GraphQL Countries".to_string();
    gql_req.kind = RequestKind::Http(HttpMethod::Post);
    gql_req.url = "https://countries.trevorblades.com/graphql".to_string();
    gql_req.body = BodyKind::GraphQl {
        query: "query Country($code: ID!) {\n  country(code: $code) {\n    name\n    capital\n    currency\n  }\n}".to_string(),
        variables: "{\n  \"code\": \"CL\"\n}".to_string(),
    };

    realtime_folder.requests = vec![ws_echo, sse_stream, gql_req];

    vec![rest_folder, realtime_folder]
}
