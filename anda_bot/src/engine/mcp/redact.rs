//! Keeps secrets out of everything Anda shows about an MCP server: approval
//! cards (including a tool call's arguments), the owner API and the CLI. Env values, header values and bearer
//! tokens are never shown, and argv and URLs lose the parts that carry
//! credentials. Even the owner gets no plaintext back: a secret is changed by
//! setting it again.

use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

use crate::config::secret_references;

const REDACTED: &str = "[redacted]";

fn arg_name_is_sensitive(name: &str) -> bool {
    let name = name
        .trim_start_matches(['-', '/'])
        .to_ascii_lowercase()
        .replace('_', "-");
    matches!(name.as_str(), "h" | "header" | "headers" | "key")
        || [
            "token",
            "secret",
            "password",
            "passwd",
            "credential",
            "api-key",
            "apikey",
            "private-key",
            "authorization",
            "bearer",
        ]
        .iter()
        .any(|marker| name.contains(marker))
        || name.ends_with("-key")
}

/// Drops the credentials of a URL: its user info, query values and fragment.
pub(crate) fn redact_url(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return "[invalid URL omitted]".to_string();
    };

    let had_password = url.password().is_some();
    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }
    if had_password {
        let _ = url.set_password(Some("redacted"));
    }

    let query_keys: Vec<String> = url.query_pairs().map(|(key, _)| key.into_owned()).collect();
    if !query_keys.is_empty() {
        url.set_query(None);
        let mut query = url.query_pairs_mut();
        for key in query_keys {
            query.append_pair(&key, REDACTED);
        }
    }
    url.set_fragment(None);
    url.to_string()
}

/// A configured URL for display: redacted, unless it is a bare reference such
/// as `${ENDPOINT}`, which names a variable rather than holding a secret.
pub(crate) fn display_url(raw: &str) -> String {
    if raw.trim_start().starts_with('$') {
        raw.to_string()
    } else {
        redact_url(raw)
    }
}

/// Redacts the credential-bearing arguments of a command line.
pub(crate) fn redact_args(args: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            redacted.push(REDACTED.to_string());
            redact_next = false;
            continue;
        }

        // Only treat URLs with an authority as URLs: that covers connection
        // strings like `postgresql://user:password@host/db` as well as http.
        // `Url::parse` also accepts any `scheme:rest` token, and
        // `redact_url` has nothing to strip from those — so
        // `x-api-key:sk-live-...` would be echoed verbatim instead of falling
        // through to the checks below.
        if let Ok(parsed) = reqwest::Url::parse(arg)
            && parsed.has_host()
        {
            redacted.push(redact_url(arg));
            continue;
        }
        // `name=value` and `name:value` both carry secrets in practice
        // (`--password=x`, `X-Api-Key:x`).
        if let Some((name, _value)) = arg.split_once(['=', ':'])
            && arg_name_is_sensitive(name)
        {
            let separator = if arg[name.len()..].starts_with(':') {
                ':'
            } else {
                '='
            };
            redacted.push(format!("{name}{separator}{REDACTED}"));
            continue;
        }
        if (arg.starts_with('-') || arg.starts_with('/')) && arg_name_is_sensitive(arg) {
            redacted.push(arg.clone());
            redact_next = true;
            continue;
        }

        let lower = arg.to_ascii_lowercase();
        if lower.contains("authorization:") || lower.contains("bearer ") {
            redacted.push(REDACTED.to_string());
        } else {
            redacted.push(arg.clone());
        }
    }
    redacted
}

/// One mcp.json entry with every secret replaced by `{"redacted": true}`,
/// which also names the stored secrets (`${secret:NAME}`) a value uses.
/// Works on the raw JSON, so an entry that failed to parse is shown safely too.
pub(crate) fn redact_entry(entry: &Value) -> Value {
    let Value::Object(entry) = entry else {
        return redacted_value();
    };
    let mut out = Map::new();
    for (key, value) in entry {
        let value = match key.as_str() {
            "env" | "environment" | "headers" => match value {
                Value::Object(values) => Value::Object(
                    values
                        .iter()
                        .map(|(name, value)| (name.clone(), redacted_secret(value)))
                        .collect(),
                ),
                _ => redacted_value(),
            },
            "bearer_token" => redacted_secret(value),
            "args" => match value {
                Value::Array(args) if args.iter().all(Value::is_string) => {
                    let args: Vec<String> = args
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                    json!(redact_args(&args))
                }
                _ => redacted_value(),
            },
            "url" => match value.as_str() {
                Some(url) => json!(display_url(url)),
                None => redacted_value(),
            },
            // The older list form nests the transport fields.
            "transport" => redact_entry(value),
            _ => value.clone(),
        };
        out.insert(key.clone(), value);
    }
    Value::Object(out)
}

fn redacted_value() -> Value {
    json!({ "redacted": true })
}

/// A redacted value, naming the stored secrets it references: those are
/// names, not secrets, and say which secret to set to change it.
fn redacted_secret(value: &Value) -> Value {
    let mut names = BTreeSet::new();
    if let Some(text) = value.as_str() {
        secret_references(text, &mut names);
    }
    if names.is_empty() {
        redacted_value()
    } else {
        json!({ "redacted": true, "secrets": names })
    }
}

/// A tool call's arguments for an approval card: values under a secret-like
/// key, and strings that carry a credential, are replaced. The model wrote
/// them, but they can still hold a token it read somewhere.
pub(crate) fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if arg_name_is_sensitive(key) {
                        json!(REDACTED)
                    } else {
                        redact_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::String(text) => match redact_args(std::slice::from_ref(text)).pop() {
            Some(redacted) => json!(redacted),
            None => json!(REDACTED),
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_lose_every_secret_but_keep_their_shape() {
        let entry = json!({
            "command": "gh-mcp",
            "args": ["--token", "argv-secret", "--verbose"],
            "env": { "GITHUB_TOKEN": "env-secret" },
            "url": "https://alice:url-password@example.com/mcp?key=query-secret",
            "headers": { "Authorization": "Bearer header-secret" },
            "bearer_token": "bearer-secret",
            "approval": { "default": "ask" },
            "transport": { "env": { "NESTED": "nested-secret" } }
        });
        let redacted = redact_entry(&entry);
        let rendered = redacted.to_string();
        for secret in [
            "argv-secret",
            "env-secret",
            "alice",
            "url-password",
            "query-secret",
            "header-secret",
            "bearer-secret",
            "nested-secret",
        ] {
            assert!(!rendered.contains(secret), "leaked {secret}: {rendered}");
        }
        assert_eq!(redacted["env"]["GITHUB_TOKEN"]["redacted"], true);
        assert_eq!(redacted["args"][2], "--verbose");
        assert_eq!(redacted["approval"]["default"], "ask");
        assert_eq!(redacted["command"], "gh-mcp");
    }

    #[test]
    fn call_arguments_lose_secret_values() {
        let args = json!({
            "repo": "ldclabs/anda-bot",
            "api_key": "key-secret",
            "options": {"Authorization": "Bearer header-secret", "draft": true},
            "links": ["https://bob:url-password@example.com/x?token=query-secret"],
            "note": "Bearer inline-secret"
        });
        let redacted = redact_json(&args);
        let rendered = redacted.to_string();
        for secret in [
            "key-secret",
            "header-secret",
            "url-password",
            "query-secret",
            "inline-secret",
        ] {
            assert!(!rendered.contains(secret), "leaked {secret}: {rendered}");
        }
        assert_eq!(redacted["repo"], "ldclabs/anda-bot");
        assert_eq!(redacted["options"]["draft"], true);
    }

    #[test]
    fn redacted_values_name_the_secrets_they_use() {
        let redacted = redact_entry(&json!({
            "headers": { "Authorization": "Bearer ${secret:GITHUB_PAT}" },
            "bearer_token": "${secret:TOKEN}",
            "env": { "PLAIN": "plain-secret" }
        }));
        assert_eq!(
            redacted["headers"]["Authorization"],
            json!({ "redacted": true, "secrets": ["GITHUB_PAT"] })
        );
        assert_eq!(redacted["bearer_token"]["secrets"], json!(["TOKEN"]));
        assert_eq!(redacted["env"]["PLAIN"], json!({ "redacted": true }));
    }

    #[test]
    fn a_url_that_names_a_variable_is_shown() {
        let redacted = redact_entry(&json!({ "url": "${ENDPOINT}" }));
        assert_eq!(redacted["url"], "${ENDPOINT}");
    }
}
