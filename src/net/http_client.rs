use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedSender;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::multipart::{Form, Part};
use reqwest::{Method, Proxy, redirect};
use tokio::sync::oneshot;

use crate::models::{
    BodyKind, HttpMethod, HttpResponseData, RequestItem, StreamDirection, StreamEntry,
    StreamStatus,
};
use crate::net::WorkerEvent;
use crate::net::interpolator::{extract_json_path, interpolate};
use crate::net::snippets::{build_resolved_headers, build_resolved_url};

fn to_reqwest_method(method: HttpMethod) -> Method {
    match method {
        HttpMethod::Get => Method::GET,
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
        HttpMethod::Patch => Method::PATCH,
        HttpMethod::Delete => Method::DELETE,
        HttpMethod::Head => Method::HEAD,
        HttpMethod::Options => Method::OPTIONS,
    }
}

fn build_client(req: &RequestItem, vars: &HashMap<String, String>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(req.network.timeout_secs.max(1)))
        .danger_accept_invalid_certs(req.network.accept_invalid_certs)
        .redirect(if req.network.follow_redirects {
            redirect::Policy::limited(10)
        } else {
            redirect::Policy::none()
        });

    let proxy = interpolate(&req.network.proxy_url, vars).trim().to_string();
    if !proxy.is_empty() {
        let p = Proxy::all(&proxy).map_err(|e| format!("Invalid proxy ({proxy}): {e}"))?;
        builder = builder.proxy(p);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

fn build_headers_map(headers: &[(String, String)]) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| format!("Invalid header '{k}': {e}"))?;
        let val = HeaderValue::from_str(v)
            .map_err(|e| format!("Invalid header value for '{k}': {e}"))?;
        map.append(name, val);
    }
    if !map.contains_key(reqwest::header::USER_AGENT) {
        map.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_static("TexReq/0.1"),
        );
    }
    Ok(map)
}

fn detect_language(headers: &[(String, String)], body: &str) -> String {
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();

    if ct.contains("json") || ct.contains("graphql") {
        return "json".to_string();
    }
    if ct.contains("html") || ct.contains("xml") || ct.contains("svg") {
        return "html".to_string();
    }
    if ct.contains("yaml") || ct.contains("yml") {
        return "yaml".to_string();
    }
    if ct.contains("javascript") {
        return "javascript".to_string();
    }

    let trimmed = body.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return "json".to_string();
    }
    if trimmed.starts_with('<') {
        return "html".to_string();
    }

    "text".to_string()
}

pub async fn execute_http(
    req: RequestItem,
    vars: HashMap<String, String>,
) -> Result<HttpResponseData, String> {
    let client = build_client(&req, &vars)?;
    let url = build_resolved_url(&req, &vars);
    let mut sent_headers = build_resolved_headers(&req, &vars);
    if !sent_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
    {
        sent_headers.push(("User-Agent".to_string(), "TexReq/0.1".to_string()));
    }

    let method = match req.kind {
        crate::models::RequestKind::Http(m) => to_reqwest_method(m),
        crate::models::RequestKind::Sse | crate::models::RequestKind::WebSocket => Method::GET,
    };

    let header_map = build_headers_map(&sent_headers)?;
    let mut builder = client.request(method, &url).headers(header_map);

    match &req.body {
        BodyKind::None => {}
        BodyKind::Json(text) => {
            builder = builder.body(interpolate(text, &vars));
        }
        BodyKind::GraphQl { query, variables } => {
            let q = interpolate(query, &vars);
            let v_raw = interpolate(variables, &vars);
            let vars_val = serde_json::from_str::<serde_json::Value>(&v_raw)
                .unwrap_or_else(|_| serde_json::json!({}));
            let payload = serde_json::json!({
                "query": q,
                "variables": vars_val,
            });
            builder = builder.json(&payload);
        }
        BodyKind::FormUrlEncoded(rows) => {
            let pairs: Vec<(String, String)> = rows
                .iter()
                .filter(|r| r.enabled && !r.key.trim().is_empty())
                .map(|r| {
                    (
                        interpolate(r.key.trim(), &vars),
                        interpolate(&r.value, &vars),
                    )
                })
                .collect();
            builder = builder.form(&pairs);
        }
        BodyKind::Multipart(rows) => {
            let mut form = Form::new();
            for row in rows.iter().filter(|r| r.enabled && !r.key.trim().is_empty()) {
                let k = interpolate(row.key.trim(), &vars);
                let v = interpolate(&row.value, &vars);
                if row.is_file {
                    let bytes = tokio::fs::read(&v)
                        .await
                        .map_err(|e| format!("Failed to read file '{v}': {e}"))?;
                    let filename = Path::new(&v)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file.bin")
                        .to_string();
                    let mime = mime_guess::from_path(&v)
                        .first_or_octet_stream()
                        .to_string();
                    let part = Part::bytes(bytes)
                        .file_name(filename)
                        .mime_str(&mime)
                        .map_err(|e| format!("Invalid MIME type: {e}"))?;
                    form = form.part(k, part);
                } else {
                    form = form.text(k, v);
                }
            }
            builder = builder.multipart(form);
        }
        BodyKind::Raw { text, .. } => {
            builder = builder.body(interpolate(text, &vars));
        }
        BodyKind::Binary { file_path } => {
            let path = interpolate(file_path, &vars);
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("Failed to read binary file '{path}': {e}"))?;
            builder = builder.body(bytes);
        }
    }

    let start = Instant::now();
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let ttfb_ms = start.elapsed().as_millis();

    let status = resp.status().as_u16();
    let status_text = resp
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let version = format!("{:?}", resp.version());
    let final_url = resp.url().to_string();

    let mut resp_headers = Vec::new();
    let mut cookies = Vec::new();
    for (k, v) in resp.headers() {
        let key_str = k.to_string();
        let val_str = v.to_str().unwrap_or("<binary>").to_string();
        if key_str.eq_ignore_ascii_case("set-cookie")
            && let Some((cookie_pair, _)) = val_str.split_once(';').or(Some((val_str.as_str(), "")))
            && let Some((ck, cv)) = cookie_pair.split_once('=')
        {
            cookies.push((ck.trim().to_string(), cv.trim().to_string()));
        }
        resp_headers.push((key_str, val_str));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Error reading response body: {e}"))?;
    let elapsed_ms = start.elapsed().as_millis();
    let download_ms = elapsed_ms.saturating_sub(ttfb_ms);
    let size_bytes = bytes.len();

    let raw_body = String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| format!("<Binary content: {size_bytes} bytes>"));

    let detected_language = detect_language(&resp_headers, &raw_body);
    let parsed_json = serde_json::from_str::<serde_json::Value>(&raw_body).ok();
    let pretty_body = if let Some(ref val) = parsed_json {
        serde_json::to_string_pretty(val).unwrap_or_else(|_| raw_body.clone())
    } else {
        raw_body.clone()
    };

    let mut captured_vars = Vec::new();
    if let Some(ref json_val) = parsed_json {
        for rule in req.captures.iter().filter(|r| r.enabled) {
            let var_name = rule.var_name.trim();
            if var_name.is_empty() {
                continue;
            }
            if let Some(extracted) = extract_json_path(json_val, &rule.json_path) {
                captured_vars.push((var_name.to_string(), extracted));
            }
        }
    }

    let scheme = if final_url.starts_with("https://") {
        if req.network.accept_invalid_certs {
            "TLS (certificate verification skipped)"
        } else {
            "TLS verified (OS trust store)"
        }
    } else {
        "Plain HTTP (no TLS)"
    };
    let proxy_desc = if req.network.proxy_url.trim().is_empty() {
        "Direct".to_string()
    } else {
        format!("Proxy: {}", interpolate(&req.network.proxy_url, &vars))
    };
    let tls_info = format!("{version} · {scheme} · {proxy_desc}");

    Ok(HttpResponseData {
        status,
        status_text,
        elapsed_ms,
        ttfb_ms,
        download_ms,
        size_bytes,
        headers: resp_headers,
        cookies,
        raw_body,
        pretty_body,
        detected_language,
        sent_headers,
        final_url,
        tls_info,
        captured_vars,
    })
}

pub async fn run_sse_stream(
    req: RequestItem,
    vars: HashMap<String, String>,
    tx: UnboundedSender<WorkerEvent>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Connecting));
    let client = match build_client(&req, &vars) {
        Ok(c) => c,
        Err(err) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(err));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    let url = build_resolved_url(&req, &vars);
    let mut headers = build_resolved_headers(&req, &vars);
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
    {
        headers.push(("Accept".to_string(), "text/event-stream".to_string()));
    }
    let header_map = match build_headers_map(&headers) {
        Ok(h) => h,
        Err(err) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(err));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    let resp = tokio::select! {
        _ = &mut stop_rx => {
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
        res = client.get(&url).headers(header_map).send() => res,
    };

    let mut resp = match resp {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!(
                "SSE rejected with HTTP {}",
                r.status()
            )));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
        Err(err) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!("SSE error: {err}")));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Connected));
    let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
        StreamDirection::System,
        None,
        format!("SSE connected to {url}"),
    )));

    let mut line_buf = String::new();
    let mut current_event: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                    StreamDirection::System,
                    None,
                    "SSE stream stopped by user",
                )));
                break;
            }
            chunk_res = resp.chunk() => {
                match chunk_res {
                    Ok(Some(chunk)) => {
                        let text = String::from_utf8_lossy(chunk.as_ref());
                        line_buf.push_str(&text);
                        while let Some(pos) = line_buf.find('\n') {
                            let mut line = line_buf[..pos].to_string();
                            line_buf.drain(..=pos);
                            if line.ends_with('\r') {
                                line.pop();
                            }
                            if line.is_empty() {
                                if !data_lines.is_empty() {
                                    let payload = data_lines.join("\n");
                                    let _ = tx.unbounded_send(WorkerEvent::StreamMessage(
                                        StreamEntry::now(
                                            StreamDirection::Incoming,
                                            current_event.take(),
                                            payload,
                                        ),
                                    ));
                                    data_lines.clear();
                                } else {
                                    current_event = None;
                                }
                            } else if line.starts_with(':') {
                                continue;
                            } else if let Some(ev) = line.strip_prefix("event:") {
                                current_event = Some(ev.trim().to_string());
                            } else if let Some(data) = line.strip_prefix("data:") {
                                data_lines.push(data.trim_start().to_string());
                            }
                        }
                    }
                    Ok(None) => {
                        if !data_lines.is_empty() {
                            let _ = tx.unbounded_send(WorkerEvent::StreamMessage(
                                StreamEntry::now(
                                    StreamDirection::Incoming,
                                    current_event.take(),
                                    data_lines.join("\n"),
                                ),
                            ));
                        }
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::System,
                            None,
                            "SSE stream closed by server",
                        )));
                        break;
                    }
                    Err(err) => {
                        let _ = tx.unbounded_send(WorkerEvent::StreamError(format!("Error reading SSE stream: {err}")));
                        break;
                    }
                }
            }
        }
    }

    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthConfig, CaptureRule, KeyValueRow, RequestKind};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn executes_real_local_http_and_captures_json_var() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local loopback listener");
        let addr = listener.local_addr().unwrap();

        let server_task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept client");
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read request");
            let raw_req = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(raw_req.starts_with("PATCH /auth/login?mode=fast HTTP/1.1"));
            assert!(raw_req.to_ascii_lowercase().contains("authorization: bearer secret_init"));

            let body = r#"{"access_token":"tok_final_42","ok":true}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: sid=xyz; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.expect("write response");
        });

        let mut vars = HashMap::new();
        vars.insert("host".to_string(), format!("http://{addr}"));
        vars.insert("init_tok".to_string(), "secret_init".to_string());

        let mut req = RequestItem::default();
        req.kind = RequestKind::Http(HttpMethod::Patch);
        req.url = "{{host}}/auth/login".to_string();
        req.params = vec![KeyValueRow::new("mode", "fast")];
        req.auth = AuthConfig::Bearer {
            token: "{{init_tok}}".to_string(),
        };
        req.body = BodyKind::Json(r#"{"step":1}"#.to_string());
        req.captures = vec![CaptureRule::new("token", "$.access_token")];

        let res = execute_http(req, vars)
            .await
            .expect("loopback http request should succeed");
        server_task.await.expect("server task finished cleanly");

        assert_eq!(res.status, 200);
        assert_eq!(res.detected_language, "json");
        assert!(res.pretty_body.contains("\"access_token\": \"tok_final_42\""));
        assert_eq!(res.cookies, vec![("sid".to_string(), "xyz".to_string())]);
        assert_eq!(
            res.captured_vars,
            vec![("token".to_string(), "tok_final_42".to_string())]
        );
    }
}
