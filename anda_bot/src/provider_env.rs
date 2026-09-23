//! Model-provider environment routing shared by the `anda` config loader and
//! the `anda_launcher` setup wizard. Both binaries compile this same source
//! file (the `daemon_protocol.rs` pattern), so the provider → env-var table
//! cannot drift between the daemon and the launcher. Keep it dependency-free.

/// API base that marks a provider as authenticating through ChatGPT Codex
/// OAuth instead of an API key.
pub const CODEX_API_BASE: &str = "https://chatgpt.com/backend-api/codex";

/// Resolve the endpoint's provider first: model names often identify a vendor
/// different from the service hosting it (for example DeepSeek on OpenRouter).
pub fn api_key_env_candidates(family: &str, model: &str, api_base: &str) -> Vec<&'static str> {
    let family = family.trim().to_ascii_lowercase();
    let model = model.trim().to_ascii_lowercase();
    let api_base = api_base.trim().to_ascii_lowercase();
    let host = api_base
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&api_base)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    let domain = |name: &str| {
        host == name
            || host
                .strip_suffix(name)
                .is_some_and(|prefix| prefix.ends_with('.'))
    };
    let endpoint: &[&str] = if domain("openrouter.ai") {
        &["OPENROUTER_API_KEY"]
    } else if domain("groq.com") {
        &["GROQ_API_KEY"]
    } else if domain("siliconflow.cn") || domain("siliconflow.com") {
        &["SILICONFLOW_API_KEY"]
    } else if domain("deepseek.com") {
        &["DEEPSEEK_API_KEY"]
    } else if domain("minimaxi.com") || domain("minimax.io") {
        &["MINIMAX_API_KEY", "MINIMAXI_API_KEY"]
    } else if domain("xiaomimimo.com") {
        &["MIMO_API_KEY", "XIAOMI_MIMO_API_KEY"]
    } else if domain("moonshot.cn") || domain("moonshot.ai") {
        &["MOONSHOT_API_KEY", "KIMI_API_KEY"]
    } else if domain("bigmodel.cn") {
        &["BIGMODEL_API_KEY", "ZHIPUAI_API_KEY", "GLM_API_KEY"]
    } else if domain("dashscope.aliyuncs.com") || domain("dashscope-intl.aliyuncs.com") {
        &["DASHSCOPE_API_KEY", "QWEN_API_KEY"]
    } else if domain("anthropic.com") {
        &["ANTHROPIC_API_KEY"]
    } else if domain("openai.com") {
        &["OPENAI_API_KEY"]
    } else if domain("googleapis.com") {
        &["GEMINI_API_KEY", "GOOGLE_API_KEY"]
    } else {
        &[]
    };
    if !endpoint.is_empty() {
        return endpoint.to_vec();
    }
    let fallback: &[&str] = if model.contains("deepseek") {
        &["DEEPSEEK_API_KEY"]
    } else if model.contains("minimax") {
        &["MINIMAX_API_KEY", "MINIMAXI_API_KEY"]
    } else if model.contains("mimo") {
        &["MIMO_API_KEY", "XIAOMI_MIMO_API_KEY"]
    } else if model.contains("kimi") {
        &["MOONSHOT_API_KEY", "KIMI_API_KEY"]
    } else if model.contains("glm") {
        &["BIGMODEL_API_KEY", "ZHIPUAI_API_KEY", "GLM_API_KEY"]
    } else if model.contains("qwen") {
        &["DASHSCOPE_API_KEY", "QWEN_API_KEY"]
    } else if model.contains("gemini") {
        &["GEMINI_API_KEY", "GOOGLE_API_KEY"]
    } else {
        match family.as_str() {
            "anthropic" => &["ANTHROPIC_API_KEY"],
            "openai" => &["OPENAI_API_KEY"],
            "gemini" | "google" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
            _ => &[],
        }
    };
    fallback.to_vec()
}

/// First non-empty value among [`api_key_env_candidates`].
pub fn env_api_key(family: &str, model: &str, api_base: &str) -> Option<String> {
    api_key_env_candidates(family, model, api_base)
        .into_iter()
        .find_map(|name| {
            std::env::var(name).ok().and_then(|value| {
                let value = value.trim().to_string();
                (!value.is_empty()).then_some(value)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_wins_over_model_vendor() {
        for (url, model, expected) in [
            (
                "https://openrouter.ai/api/v1",
                "deepseek/deepseek-chat",
                "OPENROUTER_API_KEY",
            ),
            (
                "https://api.siliconflow.cn/v1",
                "moonshotai/Kimi-K2",
                "SILICONFLOW_API_KEY",
            ),
            ("https://api.groq.com/openai/v1", "qwen-3", "GROQ_API_KEY"),
            (
                "https://api.openai.com/v1",
                "gemini-custom",
                "OPENAI_API_KEY",
            ),
        ] {
            assert_eq!(api_key_env_candidates("openai", model, url), vec![expected]);
        }
    }
    #[test]
    fn endpoint_matching_does_not_inspect_userinfo_path_or_query() {
        for url in [
            "https://openrouter.ai@example.test/",
            "https://example.test/openrouter.ai",
            "https://openrouter.ai.example.test",
            "https://example.test/?provider=openrouter.ai",
        ] {
            assert!(api_key_env_candidates("unknown", "custom", url).is_empty());
        }
    }
}
