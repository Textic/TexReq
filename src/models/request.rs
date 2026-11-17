use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const ALL: &'static [HttpMethod] = &[
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
        Self::Options,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "PATCH" => Some(Self::Patch),
            "DELETE" => Some(Self::Delete),
            "HEAD" => Some(Self::Head),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestKind {
    Http(HttpMethod),
    WebSocket,
    Sse,
}

impl Default for RequestKind {
    fn default() -> Self {
        Self::Http(HttpMethod::Get)
    }
}

impl RequestKind {
    pub fn is_realtime(&self) -> bool {
        matches!(self, Self::WebSocket | Self::Sse)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KeyValueRow {
    pub id: String,
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub is_file: bool,
}

impl KeyValueRow {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            enabled: true,
            key: key.into(),
            value: value.into(),
            is_file: false,
        }
    }

    pub fn file(key: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            enabled: true,
            key: key.into(),
            value: path.into(),
            is_file: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthConfig {
    #[default]
    None,
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    ApiKey {
        key: String,
        value: String,
        in_header: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BodyKind {
    #[default]
    None,
    Json(String),
    GraphQl {
        query: String,
        variables: String,
    },
    FormUrlEncoded(Vec<KeyValueRow>),
    Multipart(Vec<KeyValueRow>),
    Raw {
        content_type: String,
        text: String,
    },
    Binary {
        file_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRule {
    pub id: String,
    pub enabled: bool,
    pub var_name: String,
    pub json_path: String,
}

impl CaptureRule {
    pub fn new(var_name: impl Into<String>, json_path: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            enabled: true,
            var_name: var_name.into(),
            json_path: json_path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkOptions {
    pub proxy_url: String,
    pub accept_invalid_certs: bool,
    pub follow_redirects: bool,
    pub timeout_secs: u64,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            proxy_url: String::new(),
            accept_invalid_certs: false,
            follow_redirects: true,
            timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestItem {
    pub id: String,
    pub name: String,
    pub kind: RequestKind,
    pub url: String,
    pub params: Vec<KeyValueRow>,
    pub headers: Vec<KeyValueRow>,
    pub auth: AuthConfig,
    pub body: BodyKind,
    #[serde(default)]
    pub captures: Vec<CaptureRule>,
    #[serde(default)]
    pub network: NetworkOptions,
    #[serde(default)]
    pub ws_templates: Vec<String>,
}

impl Default for RequestItem {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New Request".to_string(),
            kind: RequestKind::Http(HttpMethod::Get),
            url: String::new(),
            params: Vec::new(),
            headers: Vec::new(),
            auth: AuthConfig::None,
            body: BodyKind::None,
            captures: Vec::new(),
            network: NetworkOptions::default(),
            ws_templates: Vec::new(),
        }
    }
}

impl RequestItem {
    pub fn with_defaults(settings: &super::AppSettings) -> Self {
        let mut item = Self::default();
        item.network.accept_invalid_certs = settings.default_accept_invalid_certs;
        item.network.follow_redirects = settings.default_follow_redirects;
        item.network.timeout_secs = settings.default_timeout_secs.max(1);
        item.network.proxy_url = settings.default_proxy_url.clone();
        item
    }
}
