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

pub const MAX_ENV_NAME_CHARS: usize = 32;

pub fn default_environments() -> Vec<Environment> {
    Vec::new()
}

/// Truncates `text` cleanly at a UTF-8 character boundary, appending `...` when shortened.
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    let prefix: String = text.chars().take(max_chars - 3).collect();
    format!("{prefix}...")
}

/// Picks the next sequential `Environment <N>` name based on the current count,
/// skipping any numbers already taken so names never collide.
pub fn next_environment_name(environments: &[Environment]) -> String {
    let mut idx = environments.len() + 1;
    loop {
        let candidate = format!("Environment {idx}");
        if !environments
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        idx += 1;
    }
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

    #[test]
    fn picks_next_environment_name_by_count_without_collisions() {
        assert_eq!(next_environment_name(&[]), "Environment 1");

        let four = vec![
            Environment::new("Environment 1", vec![]),
            Environment::new("Environment 2", vec![]),
            Environment::new("Environment 3", vec![]),
            Environment::new("Environment 4", vec![]),
        ];
        assert_eq!(next_environment_name(&four), "Environment 5");

        let with_gap = vec![Environment::new("Environment 2", vec![])];
        assert_eq!(next_environment_name(&with_gap), "Environment 3");
    }

    #[test]
    fn truncates_long_labels_safely() {
        assert_eq!(truncate_chars("Short", 18), "Short");
        assert_eq!(truncate_chars("ProductionUSWestCluster", 12), "Productio...");
        assert_eq!(truncate_chars("Exact10Chr", 10), "Exact10Chr");
    }
}
