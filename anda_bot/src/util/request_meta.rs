use anda_core::RequestMeta;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Canonical `RequestMeta.extra` keys.
///
/// This map is a cross-module contract: channel runtime, gateway, CLI, and
/// cron write these keys; the engine (session routing, approval policy,
/// conversation binding) reads them, and `Conversation.extra` persists them
/// verbatim. Production code must name keys through these constants; tests
/// keep the string literals on purpose, pinning the wire spelling.
pub mod keys {
    /// Where the request came from (`"telegram"`, `"cli:<dir>"`, ...). With
    /// `REPLY_TARGET`/`THREAD` it forms the route replies must return to.
    pub const SOURCE: &str = "source";
    /// Platform conversation/recipient the reply must go back to.
    pub const REPLY_TARGET: &str = "reply_target";
    /// Platform thread/topic/session marker inside `REPLY_TARGET`.
    pub const THREAD: &str = "thread";
    /// Absolute path of the workspace the request operates in.
    pub const WORKSPACE: &str = "workspace";
    /// Engine conversation id (`u64`); `0` or absent means "open a new one".
    pub const CONVERSATION: &str = "conversation";
    /// `true` marks the sender as an untrusted external IM user. Security
    /// signal: it must survive routing so external users are never treated
    /// as the owner or a trusted partner.
    pub const EXTERNAL_USER: &str = "external_user";
    /// Declared approval policy for shell/MCP actions; see
    /// `ApprovalMode::from_meta`. Transient: dropped on conversation recovery.
    pub const APPROVAL_MODE: &str = "approval_mode";
    /// `APPROVAL_MODE` value that disables approval prompts entirely.
    pub const APPROVAL_MODE_FULL_ACCESS: &str = "full_access";
    /// Id of the cron job that fired this request. Marks the run as
    /// unattended. Transient: dropped on conversation recovery.
    pub const CRON_JOB_ID: &str = "cron_job_id";
    /// Human-readable name of the firing cron job. Transient.
    pub const CRON_JOB_NAME: &str = "cron_job_name";
    /// Job kind (`"agent"`, `"shell"`, ...) of the firing cron job. Transient.
    pub const CRON_JOB_KIND: &str = "cron_job_kind";
}

pub fn request_meta_extra_as<T>(meta: &RequestMeta, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    extra_map_as(&meta.extra, key)
}

pub fn extra_map_as<T>(extra: &Map<String, Value>, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    extra_map_value(extra, key).and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn extra_map_value<'a>(extra: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    extra.get(key).or_else(|| {
        extra
            .get("extra")
            .and_then(Value::as_object)
            .and_then(|extra| extra.get(key))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_meta_extra_as_reads_flattened_extra() {
        let meta: RequestMeta = serde_json::from_value(json!({
            "source": "browser:chrome:1",
            "conversation": 42,
        }))
        .expect("request meta should deserialize");

        assert_eq!(
            request_meta_extra_as::<String>(&meta, "source"),
            Some("browser:chrome:1".to_string())
        );
        assert_eq!(
            request_meta_extra_as::<u64>(&meta, "conversation"),
            Some(42)
        );
    }

    #[test]
    fn request_meta_extra_as_reads_nested_extra() {
        let meta: RequestMeta = serde_json::from_value(json!({
            "extra": {
                "source": "browser:chrome:1",
                "conversation": 42,
                "workspace": "/tmp/browser",
            }
        }))
        .expect("legacy request meta should deserialize");

        assert_eq!(meta.get_extra_as::<String>("source"), None);
        assert_eq!(
            request_meta_extra_as::<String>(&meta, "source"),
            Some("browser:chrome:1".to_string())
        );
        assert_eq!(
            request_meta_extra_as::<u64>(&meta, "conversation"),
            Some(42)
        );
        assert_eq!(
            request_meta_extra_as::<String>(&meta, "workspace"),
            Some("/tmp/browser".to_string())
        );
    }
}
