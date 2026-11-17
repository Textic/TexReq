use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponseData {
    pub status: u16,
    pub status_text: String,
    pub elapsed_ms: u128,
    pub ttfb_ms: u128,
    pub download_ms: u128,
    pub size_bytes: usize,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<(String, String)>,
    pub raw_body: String,
    pub pretty_body: String,
    pub detected_language: String,
    pub sent_headers: Vec<(String, String)>,
    pub final_url: String,
    pub tls_info: String,
    pub captured_vars: Vec<(String, String)>,
}

pub fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
