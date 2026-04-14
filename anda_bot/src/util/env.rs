use std::collections::BTreeSet;

pub fn env_string(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

pub fn env_option(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .and_then(|value| (!value.trim().is_empty()).then_some(value))
}

pub fn env_bool(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|value| (!value.trim().is_empty()).then_some(value))
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub struct EnvUpdate {
    key: &'static str,
    value: Option<String>,
}

impl EnvUpdate {
    pub fn set(key: &'static str, value: String) -> Self {
        Self {
            key,
            value: Some(value),
        }
    }

    pub fn optional(key: &'static str, value: Option<String>) -> Self {
        Self { key, value }
    }
}

pub fn merge_env_file(existing: &str, updates: &[EnvUpdate]) -> String {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();

    for line in existing.lines() {
        let Some(key) = env_key_for_line(line) else {
            merged.push(line.to_string());
            continue;
        };

        let Some(update) = updates.iter().find(|update| update.key == key) else {
            merged.push(line.to_string());
            continue;
        };

        seen.insert(update.key);
        if let Some(value) = update.value.as_deref() {
            merged.push(format_env_assignment(update.key, value));
        }
    }

    for update in updates {
        if seen.contains(update.key) {
            continue;
        }

        if let Some(value) = update.value.as_deref() {
            merged.push(format_env_assignment(update.key, value));
        }
    }

    if merged.is_empty() {
        return String::new();
    }

    let mut output = merged.join("\n");
    output.push('\n');
    output
}

fn env_key_for_line(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }

    Some(key)
}

fn format_env_assignment(key: &str, value: &str) -> String {
    format!("{key}={}", format_env_value(value))
}

fn format_env_value(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/' | b'@' | b'+' | b',')
    }) {
        return value.to_string();
    }

    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}
