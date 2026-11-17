use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::KeyValueRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub variables: Vec<KeyValueRow>,
}

impl Environment {
    pub fn new(name: impl Into<String>, variables: Vec<KeyValueRow>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            variables,
        }
    }
}

pub fn default_environments() -> Vec<Environment> {
    vec![
        Environment::new(
            "Local / Demo",
            vec![
                KeyValueRow::new("api_base", "https://jsonplaceholder.typicode.com"),
                KeyValueRow::new("local_base", "http://127.0.0.1:8000"),
                KeyValueRow::new("token", ""),
            ],
        ),
        Environment::new(
            "Staging",
            vec![
                KeyValueRow::new("api_base", "https://staging.api.example.com"),
                KeyValueRow::new("token", ""),
            ],
        ),
        Environment::new(
            "Production",
            vec![
                KeyValueRow::new("api_base", "https://api.example.com"),
                KeyValueRow::new("token", ""),
            ],
        ),
    ]
}

/// Parses `.env` text (`KEY=VALUE`, optional `export ` prefix, `#` comments, quoted values).
pub fn parse_dotenv(text: &str) -> Vec<KeyValueRow> {
    let mut rows = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = val.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = value[1..value.len() - 1].to_string();
        } else if let Some((before_comment, _)) = value.split_once(" #") {
            value = before_comment.trim_end().to_string();
        }
        rows.push(KeyValueRow::new(key, value));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotenv_lines_cleanly() {
        let raw = r#"
# Comment
API_URL="http://localhost:3000/api"
export TOKEN='secret_123'
PLAIN=hello_world # inline comment
EMPTY=
"#;
        let rows = parse_dotenv(raw);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].key, "API_URL");
        assert_eq!(rows[0].value, "http://localhost:3000/api");
        assert_eq!(rows[1].key, "TOKEN");
        assert_eq!(rows[1].value, "secret_123");
        assert_eq!(rows[2].key, "PLAIN");
        assert_eq!(rows[2].value, "hello_world");
        assert_eq!(rows[3].key, "EMPTY");
        assert_eq!(rows[3].value, "");
    }
}
