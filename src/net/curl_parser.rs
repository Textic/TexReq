use url::Url;

use crate::models::{AuthConfig, BodyKind, HttpMethod, KeyValueRow, RequestItem, RequestKind};

pub fn looks_like_curl(input: &str) -> bool {
    let trimmed = input.trim_start();
    trimmed.starts_with("curl ")
        || trimmed.starts_with("curl\t")
        || trimmed.starts_with("curl\n")
        || trimmed.starts_with("curl\r")
        || trimmed.starts_with("curl.exe ")
}

/// Parses a copied Bash / Chrome DevTools / Windows CMD cURL command into a `RequestItem`.
pub fn parse_curl(input: &str) -> Result<RequestItem, String> {
    let tokens = tokenize_shell(input);
    if tokens.is_empty() {
        return Err("Empty cURL command".to_string());
    }

    let start_idx = if tokens[0].eq_ignore_ascii_case("curl")
        || tokens[0].eq_ignore_ascii_case("curl.exe")
    {
        1
    } else {
        0
    };

    let mut req = RequestItem::default();
    let mut explicit_method: Option<HttpMethod> = None;
    let mut raw_url: Option<String> = None;
    let mut data_parts: Vec<String> = Vec::new();
    let mut urlencoded_parts: Vec<String> = Vec::new();
    let mut multipart_rows: Vec<KeyValueRow> = Vec::new();
    let mut is_head = false;
    let mut force_get = false;

    let mut i = start_idx;
    while i < tokens.len() {
        let tok = &tokens[i];
        match tok.as_str() {
            "-X" | "--request" => {
                if let Some(m) = tokens.get(i + 1) {
                    explicit_method = HttpMethod::parse(m);
                    i += 2;
                    continue;
                }
            }
            "--url" => {
                if let Some(u) = tokens.get(i + 1) {
                    raw_url = Some(u.clone());
                    i += 2;
                    continue;
                }
            }
            "-H" | "--header" => {
                if let Some(header_line) = tokens.get(i + 1) {
                    if let Some((k, v)) = header_line.split_once(':') {
                        let key = k.trim();
                        let val = v.trim();
                        if key.eq_ignore_ascii_case("authorization") {
                            if let Some(token) = val
                                .strip_prefix("Bearer ")
                                .or_else(|| val.strip_prefix("bearer "))
                            {
                                req.auth = AuthConfig::Bearer {
                                    token: token.trim().to_string(),
                                };
                            } else {
                                req.headers.push(KeyValueRow::new(key, val));
                            }
                        } else {
                            req.headers.push(KeyValueRow::new(key, val));
                        }
                    }
                    i += 2;
                    continue;
                }
            }
            "-b" | "--cookie" => {
                if let Some(cookie) = tokens.get(i + 1) {
                    req.headers.push(KeyValueRow::new("Cookie", cookie));
                    i += 2;
                    continue;
                }
            }
            "-A" | "--user-agent" => {
                if let Some(ua) = tokens.get(i + 1) {
                    req.headers.push(KeyValueRow::new("User-Agent", ua));
                    i += 2;
                    continue;
                }
            }
            "-u" | "--user" => {
                if let Some(creds) = tokens.get(i + 1) {
                    let (user, pass) = creds.split_once(':').unwrap_or((creds.as_str(), ""));
                    req.auth = AuthConfig::Basic {
                        username: user.to_string(),
                        password: pass.to_string(),
                    };
                    i += 2;
                    continue;
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-binary" => {
                if let Some(d) = tokens.get(i + 1) {
                    data_parts.push(d.clone());
                    i += 2;
                    continue;
                }
            }
            "--data-urlencode" => {
                if let Some(d) = tokens.get(i + 1) {
                    urlencoded_parts.push(d.clone());
                    i += 2;
                    continue;
                }
            }
            "-F" | "--form" => {
                if let Some(part) = tokens.get(i + 1) {
                    if let Some((k, v)) = part.split_once('=') {
                        if let Some(path) = v.strip_prefix('@') {
                            multipart_rows.push(KeyValueRow::file(k.trim(), path.trim()));
                        } else {
                            multipart_rows.push(KeyValueRow::new(k.trim(), v.trim()));
                        }
                    }
                    i += 2;
                    continue;
                }
            }
            "-x" | "--proxy" => {
                if let Some(p) = tokens.get(i + 1) {
                    req.network.proxy_url = p.clone();
                    i += 2;
                    continue;
                }
            }
            "-k" | "--insecure" => {
                req.network.accept_invalid_certs = true;
            }
            "-I" | "--head" => {
                is_head = true;
            }
            "-G" | "--get" => {
                force_get = true;
            }
            "-L" | "--location" => {
                req.network.follow_redirects = true;
            }
            "--compressed" | "-s" | "--silent" | "-v" | "--verbose" | "-i" | "--include" => {}
            _ => {
                if let Some(rest) = tok.strip_prefix("-X") {
                    if !rest.is_empty() {
                        explicit_method = HttpMethod::parse(rest);
                    }
                } else if !tok.starts_with('-') && raw_url.is_none() {
                    raw_url = Some(tok.clone());
                }
            }
        }
        i += 1;
    }

    let url_str = raw_url.ok_or_else(|| "No URL found in cURL command".to_string())?;
    let trimmed_url = url_str.trim();

    if let Ok(mut parsed) = Url::parse(trimmed_url) {
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if !pairs.is_empty() {
            for (k, v) in pairs {
                req.params.push(KeyValueRow::new(k, v));
            }
            parsed.set_query(None);
            req.url = parsed.to_string();
        } else {
            req.url = trimmed_url.to_string();
        }
    } else {
        req.url = trimmed_url.to_string();
    }

    let has_sse_accept = req.headers.iter().any(|h| {
        h.key.eq_ignore_ascii_case("accept") && h.value.to_ascii_lowercase().contains("event-stream")
    });

    if req.url.starts_with("ws://") || req.url.starts_with("wss://") {
        req.kind = RequestKind::WebSocket;
    } else if has_sse_accept {
        req.kind = RequestKind::Sse;
    } else if force_get {
        req.kind = RequestKind::Http(HttpMethod::Get);
    } else if is_head {
        req.kind = RequestKind::Http(HttpMethod::Head);
    } else if let Some(m) = explicit_method {
        req.kind = RequestKind::Http(m);
    } else if !data_parts.is_empty() || !urlencoded_parts.is_empty() || !multipart_rows.is_empty() {
        req.kind = RequestKind::Http(HttpMethod::Post);
    } else {
        req.kind = RequestKind::Http(HttpMethod::Get);
    }

    if !multipart_rows.is_empty() {
        req.body = BodyKind::Multipart(multipart_rows);
    } else if !urlencoded_parts.is_empty() {
        let rows = urlencoded_parts
            .into_iter()
            .filter_map(|p| p.split_once('=').map(|(k, v)| KeyValueRow::new(k, v)))
            .collect();
        req.body = BodyKind::FormUrlEncoded(rows);
    } else if !data_parts.is_empty() {
        let joined = data_parts.join("&");
        let is_form = req.headers.iter().any(|h| {
            h.key.eq_ignore_ascii_case("content-type")
                && h.value
                    .to_ascii_lowercase()
                    .contains("application/x-www-form-urlencoded")
        });
        let looks_json = joined.trim_start().starts_with('{') || joined.trim_start().starts_with('[');
        if is_form {
            let rows = joined
                .split('&')
                .filter(|s| !s.is_empty())
                .map(|pair| {
                    let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                    KeyValueRow::new(k, v)
                })
                .collect();
            req.body = BodyKind::FormUrlEncoded(rows);
        } else if looks_json || serde_json::from_str::<serde_json::Value>(&joined).is_ok() {
            let formatted = serde_json::from_str::<serde_json::Value>(&joined)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or(joined);
            req.body = BodyKind::Json(formatted);
        } else {
            req.body = BodyKind::Raw {
                content_type: "text/plain".to_string(),
                text: joined,
            };
        }
    }

    if let Ok(u) = Url::parse(&req.url)
        && let Some(host) = u.host_str()
    {
        let path_tail = u
            .path_segments()
            .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
            .unwrap_or("");
        req.name = if path_tail.is_empty() {
            host.to_string()
        } else {
            format!("{host}/{path_tail}")
        };
    }

    Ok(req)
}

fn tokenize_shell(input: &str) -> Vec<String> {
    // Normalize line continuations from Bash (`\`), CMD (`^`), and PowerShell (`` ` ``).
    let normalized = input
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .replace("^\r\n", " ")
        .replace("^\n", " ")
        .replace("`\r\n", " ")
        .replace("`\n", " ");

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_cmd_quote = false;
    let mut in_token = false;
    let mut chars = normalized.chars().peekable();

    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                current.push(c);
            }
            continue;
        }

        if in_cmd_quote {
            if c == '^' {
                match chars.next() {
                    Some('"') => in_cmd_quote = false,
                    Some('\\') => {
                        if chars.peek() == Some(&'^') {
                            chars.next();
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                current.push('"');
                            } else {
                                current.push('\\');
                            }
                        } else {
                            current.push('\\');
                        }
                    }
                    Some(other) => current.push(other),
                    None => {}
                }
            } else if c == '\\' {
                if let Some(next) = chars.next() {
                    match next {
                        '"' | '\\' => current.push(next),
                        'n' => current.push('\n'),
                        't' => current.push('\t'),
                        'r' => current.push('\r'),
                        other => {
                            current.push('\\');
                            current.push(other);
                        }
                    }
                }
            } else {
                current.push(c);
            }
            continue;
        }

        if in_double {
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    if matches!(next, '"' | '\\' | '$' | 'n' | 't' | 'r') {
                        chars.next();
                        match next {
                            'n' => current.push('\n'),
                            't' => current.push('\t'),
                            'r' => current.push('\r'),
                            other => current.push(other),
                        }
                        continue;
                    }
                }
                current.push(c);
            } else if c == '"' {
                in_double = false;
            } else {
                current.push(c);
            }
            continue;
        }

        match c {
            '\'' => {
                in_single = true;
                in_token = true;
            }
            '"' => {
                in_double = true;
                in_token = true;
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_single = true;
                in_token = true;
            }
            '^' => {
                if let Some(next) = chars.next() {
                    if next == '"' {
                        in_cmd_quote = true;
                        in_token = true;
                    } else {
                        current.push(next);
                        in_token = true;
                    }
                }
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_token = true;
                }
            }
            ch if ch.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            other => {
                current.push(other);
                in_token = true;
            }
        }
    }

    if in_token {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chrome_devtools_bash_patch_curl() {
        let cmd = r#"curl 'https://api.example.com/v1/users/42?include=roles' \
  -X 'PATCH' \
  -H 'Accept: application/json' \
  -H 'Authorization: Bearer secret_tok' \
  -H 'Content-Type: application/json' \
  --data-raw '{"name":"Ada","active":true}'"#;

        assert!(looks_like_curl(cmd));
        let req = parse_curl(cmd).expect("should parse bash curl");
        assert_eq!(req.kind, RequestKind::Http(HttpMethod::Patch));
        assert_eq!(req.url, "https://api.example.com/v1/users/42");
        assert_eq!(req.params.len(), 1);
        assert_eq!(req.params[0].key, "include");
        assert_eq!(req.params[0].value, "roles");
        assert_eq!(
            req.auth,
            AuthConfig::Bearer {
                token: "secret_tok".to_string()
            }
        );
        match req.body {
            BodyKind::Json(j) => assert!(j.contains("\"name\": \"Ada\"")),
            other => panic!("expected Json body, got {other:?}"),
        }
    }

    #[test]
    fn parses_windows_cmd_curl_with_proxy_and_insecure() {
        let cmd = "curl ^\"http://localhost:8080/login^\" ^\n  -k -x http://127.0.0.1:8080 -u admin:pass123";
        let req = parse_curl(cmd).expect("should parse cmd curl");
        assert_eq!(req.url, "http://localhost:8080/login");
        assert!(req.network.accept_invalid_certs);
        assert_eq!(req.network.proxy_url, "http://127.0.0.1:8080");
        assert_eq!(
            req.auth,
            AuthConfig::Basic {
                username: "admin".to_string(),
                password: "pass123".to_string()
            }
        );
    }
}
