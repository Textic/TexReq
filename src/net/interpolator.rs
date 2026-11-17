use std::collections::HashMap;

use serde_json::Value;

/// Replaces `{{key}}` placeholders with values from `vars`. Unknown variables are kept intact.
pub fn interpolate(input: &str, vars: &HashMap<String, String>) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let key = after_open[..end].trim();
            if let Some(val) = vars.get(key) {
                out.push_str(val);
            } else {
                out.push_str("{{");
                out.push_str(&after_open[..end]);
                out.push_str("}}");
            }
            rest = &after_open[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }

    out.push_str(rest);
    out
}

/// Extracts a scalar/string representation from a JSON value using simple paths
/// such as `$.access_token`, `$.data.user.id`, `$[0].id`, or `data.items[1].name`.
pub fn extract_json_path(value: &Value, path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let normalized = normalized.strip_prefix('.').unwrap_or(normalized);

    if normalized.is_empty() {
        return value_to_capture_string(value);
    }

    let mut current = value;
    for segment in normalized.split('.') {
        if segment.is_empty() {
            continue;
        }
        let mut remainder = segment;
        while let Some(bracket_start) = remainder.find('[') {
            let key_part = &remainder[..bracket_start];
            if !key_part.is_empty() {
                current = current.get(key_part)?;
            }
            let after_open = &remainder[bracket_start + 1..];
            let bracket_end = after_open.find(']')?;
            let index_str = after_open[..bracket_end].trim();
            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
            remainder = &after_open[bracket_end + 1..];
        }
        if !remainder.is_empty() {
            current = current.get(remainder)?;
        }
    }

    value_to_capture_string(current)
}

fn value_to_capture_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn interpolates_known_variables_and_preserves_unknown() {
        let mut vars = HashMap::new();
        vars.insert("base".to_string(), "https://api.local".to_string());
        vars.insert("token".to_string(), "abc123".to_string());

        let result = interpolate("{{ base }}/v1/users?auth={{token}}&x={{missing}}", &vars);
        assert_eq!(result, "https://api.local/v1/users?auth=abc123&x={{missing}}");
    }

    #[test]
    fn extracts_nested_and_indexed_json_paths() {
        let data = json!({
            "access_token": "tok_999",
            "user": { "id": 42, "active": true },
            "items": [
                { "code": "A1" },
                { "code": "B2" }
            ]
        });

        assert_eq!(extract_json_path(&data, "$.access_token").as_deref(), Some("tok_999"));
        assert_eq!(extract_json_path(&data, "$.user.id").as_deref(), Some("42"));
        assert_eq!(extract_json_path(&data, "$.user.active").as_deref(), Some("true"));
        assert_eq!(extract_json_path(&data, "$.items[1].code").as_deref(), Some("B2"));
        assert_eq!(extract_json_path(&data, "items[0].code").as_deref(), Some("A1"));
        assert_eq!(extract_json_path(&data, "$.missing.field"), None);

        let arr = json!([{ "id": 7 }]);
        assert_eq!(extract_json_path(&arr, "$[0].id").as_deref(), Some("7"));
    }
}
