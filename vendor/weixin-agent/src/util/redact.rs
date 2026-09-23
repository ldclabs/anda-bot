//! Log redaction utilities for sensitive data.

const DEFAULT_BODY_MAX_LEN: usize = 200;
const DEFAULT_TOKEN_PREFIX_LEN: usize = 6;

/// Find a safe UTF-8 char boundary at or before `max` bytes.
fn safe_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    s.char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max)
        .last()
        .unwrap_or(0)
}

/// Truncate a string, appending a length indicator when trimmed.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let boundary = safe_boundary(s, max);
    format!("{}…(len={})", &s[..boundary], s.len())
}

/// Redact a token: show only the first few chars + total length.
pub fn redact_token(token: &str, prefix_len: usize) -> String {
    if token.is_empty() {
        return "(none)".to_owned();
    }
    if token.len() <= prefix_len {
        return format!("****(len={})", token.len());
    }
    let boundary = safe_boundary(token, prefix_len);
    format!("{}…(len={})", &token[..boundary], token.len())
}

/// Strip query string from a URL for safe logging.
pub fn redact_url(raw_url: &str) -> String {
    match url::Url::parse(raw_url) {
        Ok(u) => {
            let base = format!("{}{}", u.origin().ascii_serialization(), u.path());
            if u.query().is_some() {
                format!("{base}?<redacted>")
            } else {
                base
            }
        }
        Err(_) => truncate(raw_url, 80),
    }
}

/// Truncate and redact sensitive fields in a JSON body string.
pub fn redact_body(body: &str, max_len: usize) -> String {
    if body.is_empty() {
        return "(empty)".to_owned();
    }
    let mut redacted = body.to_owned();
    for key in &[
        "context_token",
        "bot_token",
        "token",
        "authorization",
        "Authorization",
    ] {
        let pattern = format!("\"{key}\":\"");
        let mut search_from = 0;
        while search_from < redacted.len() {
            let Some(pos) = redacted[search_from..].find(&pattern) else {
                break;
            };
            let start = search_from + pos;
            let value_start = start + pattern.len();
            if value_start >= redacted.len() {
                break;
            }
            let Some(end) = redacted[value_start..].find('"') else {
                break;
            };
            redacted.replace_range(value_start..value_start + end, "<redacted>");
            search_from = value_start + "<redacted>".len() + 1;
        }
    }
    if redacted.len() <= max_len {
        return redacted;
    }
    let boundary = safe_boundary(&redacted, max_len);
    format!(
        "{}…(truncated, totalLen={})",
        &redacted[..boundary],
        redacted.len()
    )
}

/// Convenience wrapper with default max length.
pub fn redact_body_default(body: &str) -> String {
    redact_body(body, DEFAULT_BODY_MAX_LEN)
}

/// Convenience wrapper with default prefix length.
pub fn redact_token_default(token: &str) -> String {
    redact_token(token, DEFAULT_TOKEN_PREFIX_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_token_empty() {
        assert_eq!(redact_token("", 6), "(none)");
    }

    #[test]
    fn redact_token_short() {
        assert_eq!(redact_token("abc", 6), "****(len=3)");
    }

    #[test]
    fn redact_token_normal() {
        let r = redact_token("abcdefghij", 6);
        assert!(r.starts_with("abcdef"));
        assert!(r.contains("len=10"));
    }

    #[test]
    fn redact_token_multibyte() {
        // Should not panic on multi-byte UTF-8
        let r = redact_token("你好世界测试数据", 6);
        assert!(r.contains("len="));
    }

    #[test]
    fn redact_url_with_query() {
        let r = redact_url("https://example.com/path?secret=123");
        assert!(r.contains("/path"));
        assert!(r.contains("<redacted>"));
        assert!(!r.contains("secret"));
    }

    #[test]
    fn redact_url_without_query() {
        let r = redact_url("https://example.com/path");
        assert_eq!(r, "https://example.com/path");
    }

    #[test]
    fn redact_body_sensitive_fields() {
        let body = r#"{"token":"secret123","name":"bob"}"#;
        let r = redact_body(body, 500);
        assert!(r.contains("<redacted>"));
        assert!(!r.contains("secret123"));
        assert!(r.contains("bob"));
    }

    #[test]
    fn redact_body_truncation() {
        let body = r#"{"data":"x"}"#;
        let r = redact_body(body, 5);
        assert!(r.contains("truncated"));
    }

    #[test]
    fn redact_body_empty() {
        assert_eq!(redact_body("", 100), "(empty)");
    }

    #[test]
    fn redact_body_multibyte() {
        let body = r#"{"token":"密码","name":"你好"}"#;
        let r = redact_body(body, 500);
        assert!(r.contains("<redacted>"));
        assert!(!r.contains("密码"));
    }

    #[test]
    fn truncate_short() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_long() {
        let r = truncate("hello world", 5);
        assert!(r.starts_with("hello"));
        assert!(r.contains("len=11"));
    }

    #[test]
    fn truncate_multibyte() {
        // Should not panic
        let r = truncate("你好世界", 3);
        assert!(r.contains("len="));
    }
}
