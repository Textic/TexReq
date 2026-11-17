use std::collections::HashMap;

use base64::Engine as _;
use url::Url;

use crate::models::{AuthConfig, BodyKind, RequestItem, RequestKind};
use crate::net::interpolator::interpolate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetLang {
    Curl,
    RustReqwest,
    TsFetch,
    PythonRequests,
}

impl SnippetLang {
    pub const ALL: &'static [SnippetLang] = &[
        Self::Curl,
        Self::RustReqwest,
        Self::TsFetch,
        Self::PythonRequests,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Curl => "cURL",
            Self::RustReqwest => "Rust",
            Self::TsFetch => "Fetch (JS/TS)",
            Self::PythonRequests => "Python",
        }
    }

    pub fn editor_language(&self) -> &'static str {
        match self {
            Self::Curl => "bash",
            Self::RustReqwest => "rust",
            Self::TsFetch => "javascript",
            Self::PythonRequests => "python",
        }
    }
}

pub fn build_resolved_url(req: &RequestItem, vars: &HashMap<String, String>) -> String {
    let mut base = interpolate(&req.url, vars).trim().to_string();
    if base.is_empty() {
        base = "https://example.com".to_string();
    } else if !base.contains("://") {
        let prefix = if req.kind == RequestKind::WebSocket {
            "ws://"
        } else {
            "http://"
        };
        base = format!("{prefix}{base}");
    }

    let mut pairs: Vec<(String, String)> = req
        .params
        .iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
        .map(|r| {
            (
                interpolate(r.key.trim(), vars),
                interpolate(&r.value, vars),
            )
        })
        .collect();

    if let AuthConfig::ApiKey {
        key,
        value,
        in_header: false,
    } = &req.auth
    {
        let k = interpolate(key.trim(), vars);
        if !k.is_empty() {
            pairs.push((k, interpolate(value, vars)));
        }
    }

    if pairs.is_empty() {
        return base;
    }

    if let Ok(mut parsed) = Url::parse(&base) {
        {
            let mut query = parsed.query_pairs_mut();
            for (k, v) in pairs {
                query.append_pair(&k, &v);
            }
        }
        parsed.to_string()
    } else {
        base
    }
}

pub fn build_resolved_headers(
    req: &RequestItem,
    vars: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
        .map(|r| {
            (
                interpolate(r.key.trim(), vars),
                interpolate(&r.value, vars),
            )
        })
        .collect();

    match &req.auth {
        AuthConfig::None => {}
        AuthConfig::Bearer { token } => {
            let tok = interpolate(token, vars);
            if !tok.is_empty() {
                headers.push(("Authorization".to_string(), format!("Bearer {tok}")));
            }
        }
        AuthConfig::Basic { username, password } => {
            let u = interpolate(username, vars);
            let p = interpolate(password, vars);
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
            headers.push(("Authorization".to_string(), format!("Basic {encoded}")));
        }
        AuthConfig::ApiKey {
            key,
            value,
            in_header: true,
        } => {
            let k = interpolate(key.trim(), vars);
            if !k.is_empty() {
                headers.push((k, interpolate(value, vars)));
            }
        }
        AuthConfig::ApiKey {
            in_header: false, ..
        } => {}
    }

    let has_ct = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    if !has_ct {
        match &req.body {
            BodyKind::Json(_) | BodyKind::GraphQl { .. } => {
                headers.push(("Content-Type".to_string(), "application/json".to_string()));
            }
            BodyKind::FormUrlEncoded(_) => {
                headers.push((
                    "Content-Type".to_string(),
                    "application/x-www-form-urlencoded".to_string(),
                ));
            }
            BodyKind::Raw { content_type, .. } if !content_type.trim().is_empty() => {
                headers.push(("Content-Type".to_string(), content_type.trim().to_string()));
            }
            _ => {}
        }
    }

    headers
}

fn resolved_body_string(req: &RequestItem, vars: &HashMap<String, String>) -> Option<String> {
    match &req.body {
        BodyKind::None => None,
        BodyKind::Json(text) => Some(interpolate(text, vars)),
        BodyKind::GraphQl { query, variables } => {
            let q = interpolate(query, vars);
            let v_str = interpolate(variables, vars);
            let vars_json = serde_json::from_str::<serde_json::Value>(&v_str)
                .unwrap_or_else(|_| serde_json::json!({}));
            Some(
                serde_json::to_string_pretty(&serde_json::json!({
                    "query": q,
                    "variables": vars_json
                }))
                .unwrap_or(q),
            )
        }
        BodyKind::FormUrlEncoded(rows) => {
            let pairs: Vec<String> = rows
                .iter()
                .filter(|r| r.enabled && !r.key.trim().is_empty())
                .map(|r| {
                    format!(
                        "{}={}",
                        interpolate(r.key.trim(), vars),
                        interpolate(&r.value, vars)
                    )
                })
                .collect();
            Some(pairs.join("&"))
        }
        BodyKind::Raw { text, .. } => Some(interpolate(text, vars)),
        BodyKind::Multipart(_) | BodyKind::Binary { .. } => None,
    }
}

pub fn generate_snippet(
    req: &RequestItem,
    vars: &HashMap<String, String>,
    lang: SnippetLang,
) -> String {
    let url = build_resolved_url(req, vars);
    let headers = build_resolved_headers(req, vars);
    let body = resolved_body_string(req, vars);
    let method = match req.kind {
        RequestKind::Http(m) => m.as_str(),
        RequestKind::Sse => "GET",
        RequestKind::WebSocket => "GET",
    };

    if req.kind == RequestKind::WebSocket {
        return match lang {
            SnippetLang::Curl => format!("websocat '{}'", url.replace('\'', "'\\''")),
            SnippetLang::RustReqwest => format!(
                "let (ws_stream, _) = tokio_tungstenite::connect_async(\"{url}\").await?;"
            ),
            SnippetLang::TsFetch => format!(
                "const ws = new WebSocket(\"{url}\");\nws.onmessage = (ev) => console.log(ev.data);"
            ),
            SnippetLang::PythonRequests => format!(
                "import websockets, asyncio\n\nasync def run():\n    async with websockets.connect(\"{url}\") as ws:\n        print(await ws.recv())"
            ),
        };
    }

    match lang {
        SnippetLang::Curl => {
            let mut lines = vec![format!("curl -X {method} '{}'", url.replace('\'', "'\\''"))];
            for (k, v) in &headers {
                lines.push(format!("  -H '{}: {}'", k, v.replace('\'', "'\\''")));
            }
            if let Some(b) = body {
                lines.push(format!("  --data-raw '{}'", b.replace('\'', "'\\''")));
            }
            if !req.network.proxy_url.trim().is_empty() {
                lines.push(format!("  -x '{}'", req.network.proxy_url.trim()));
            }
            if req.network.accept_invalid_certs {
                lines.push("  -k".to_string());
            }
            lines.join(" \\\n")
        }
        SnippetLang::RustReqwest => {
            let mut out = String::from("let client = reqwest::Client::new();\nlet resp = client\n");
            out.push_str(&format!(
                "    .request(reqwest::Method::{}, \"{url}\")\n",
                method.to_ascii_uppercase()
            ));
            for (k, v) in &headers {
                out.push_str(&format!(
                    "    .header(\"{k}\", \"{}\")\n",
                    v.replace('"', "\\\"")
                ));
            }
            if let Some(b) = body {
                out.push_str(&format!("    .body(r#\"{b}\"#)\n"));
            }
            out.push_str("    .send()\n    .await?;\nprintln!(\"{}\", resp.text().await?);");
            out
        }
        SnippetLang::TsFetch => {
            let mut out = format!("const res = await fetch(\"{url}\", {{\n  method: \"{method}\",\n");
            if !headers.is_empty() {
                out.push_str("  headers: {\n");
                for (k, v) in &headers {
                    out.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
                }
                out.push_str("  },\n");
            }
            if let Some(b) = body {
                out.push_str(&format!("  body: {},\n", serde_json::to_string(&b).unwrap_or_else(|_| "\"\"".to_string())));
            }
            out.push_str("});\nconsole.log(await res.text());");
            out
        }
        SnippetLang::PythonRequests => {
            let mut out = String::from("import requests\n\n");
            out.push_str(&format!("url = \"{url}\"\n"));
            if !headers.is_empty() {
                out.push_str("headers = {\n");
                for (k, v) in &headers {
                    out.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
                }
                out.push_str("}\n");
            }
            if let Some(b) = body {
                out.push_str(&format!(
                    "payload = {}\nresp = requests.request(\"{method}\", url{}, data=payload)\n",
                    serde_json::to_string(&b).unwrap_or_else(|_| "\"\"".to_string()),
                    if headers.is_empty() { "" } else { ", headers=headers" }
                ));
            } else {
                out.push_str(&format!(
                    "resp = requests.request(\"{method}\", url{})\n",
                    if headers.is_empty() { "" } else { ", headers=headers" }
                ));
            }
            out.push_str("print(resp.status_code, resp.text)");
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HttpMethod, KeyValueRow};

    #[test]
    fn generates_snippets_across_all_languages() {
        let mut req = RequestItem::default();
        req.kind = RequestKind::Http(HttpMethod::Patch);
        req.url = "https://api.test/items/1".to_string();
        req.headers = vec![KeyValueRow::new("X-Test", "yes")];
        req.body = BodyKind::Json("{\"ok\":true}".to_string());
        let vars = HashMap::new();

        let curl = generate_snippet(&req, &vars, SnippetLang::Curl);
        assert!(curl.contains("curl -X PATCH 'https://api.test/items/1'"));
        assert!(curl.contains("X-Test: yes"));

        let rust = generate_snippet(&req, &vars, SnippetLang::RustReqwest);
        assert!(rust.contains("reqwest::Method::PATCH"));

        let ts = generate_snippet(&req, &vars, SnippetLang::TsFetch);
        assert!(ts.contains("method: \"PATCH\""));

        let py = generate_snippet(&req, &vars, SnippetLang::PythonRequests);
        assert!(py.contains("requests.request(\"PATCH\""));
    }
}
