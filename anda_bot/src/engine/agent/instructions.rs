//! System instruction rendering: the static self instructions plus the
//! runtime context (self knowledge, notes, available tools, environment,
//! user profile, local time) assembled per request.

use anda_core::{AgentContext, BoxError, Principal, StateFeatures};
use anda_engine::{
    context::AgentCtx,
    extension::note::{load_notes, load_notes_from_legacy},
};
use chrono::{DateTime, Local, Utc};

use crate::engine::ActionsTool;

use super::AndaBot;

static SELF_INSTRUCTIONS: &str = include_str!("../../../assets/SelfInstructions.md");

struct SystemInstructionSections<'a> {
    self_knowledge: &'a str,
    notes: &'a str,
    available_tools: &'a [String],
    home_dir: &'a str,
    workspace: &'a str,
    user_profile: &'a str,
    local_date: &'a str,
}

fn render_system_instructions(sections: SystemInstructionSections<'_>) -> String {
    format!(
        "{ins}\n\n---\n\n# Runtime Context\n\n## Self Knowledge\n{knowledge}\n\n## Notes\n{notes}\n\n## Available Callable Names\nNames only; schemas are intentionally omitted here. Use `tools_select` before calling any name whose full schema is not already loaded.\n{tools}\n\n## Environment\n- home: {home}\n- current workspace (authoritative): {workspace}\n\nUse the current workspace for filesystem and shell operations. Workspace paths in history are historical unless the user explicitly selects them.\n\n## User Profile\n{user_profile}\n\n## Current Datetime: {local_date}",
        ins = SELF_INSTRUCTIONS.trim(),
        knowledge = sections.self_knowledge,
        notes = sections.notes,
        tools = format_available_tools(sections.available_tools),
        home = sections.home_dir,
        workspace = sections.workspace,
        user_profile = sections.user_profile,
        local_date = sections.local_date,
    )
}

fn format_available_tools(available_tools: &[String]) -> String {
    if available_tools.is_empty() {
        "none".to_string()
    } else {
        available_tools.join(", ")
    }
}

pub(super) async fn available_tool_names(ctx: &AgentCtx) -> Vec<String> {
    ctx.definitions(None)
        .await
        .into_iter()
        .filter_map(|def| {
            if def.name == AndaBot::NAME || def.name == ActionsTool::NAME {
                None
            } else {
                Some(def.name)
            }
        })
        .collect()
}

impl AndaBot {
    pub(super) async fn build_system_instructions(
        &self,
        ctx: &AgentCtx,
        home_dir: &str,
        workspace: &str,
        available_tools: &[String],
        now_ms: u64,
    ) -> Result<String, BoxError> {
        self.build_system_instructions_for_user(
            ctx,
            ctx.caller(),
            home_dir,
            workspace,
            available_tools,
            now_ms,
        )
        .await
    }

    pub(super) async fn build_system_instructions_for_user(
        &self,
        ctx: &AgentCtx,
        user: &Principal,
        home_dir: &str,
        workspace: &str,
        available_tools: &[String],
        now_ms: u64,
    ) -> Result<String, BoxError> {
        let primer = self.inner.brain.describe_primer().await?;
        let user_profile = self.inner.brain.user_info(user.to_string(), None).await?;
        let notes = match load_notes(ctx).await {
            Some(notes) => notes,
            None => load_notes_from_legacy(ctx).await.unwrap_or_default(),
        };
        let local_date = format_local_date(now_ms);
        let self_knowledge = serde_json::to_string(primer.get("identity").unwrap_or(&primer))?;
        let notes = serde_json::to_string(&notes.items)?;
        let user_profile = serde_json::to_string(&user_profile)?;

        Ok(render_system_instructions(SystemInstructionSections {
            self_knowledge: &self_knowledge,
            notes: &notes,
            available_tools,
            home_dir,
            workspace,
            user_profile: &user_profile,
            local_date: &local_date,
        }))
    }
}

fn format_local_date(now_ms: u64) -> String {
    let local_datetime: Option<DateTime<Local>> =
        DateTime::<Utc>::from_timestamp_millis(now_ms as i64).map(|d| d.with_timezone(&Local));
    local_datetime
        .map(|dt| dt.format("%Y-%m-%d %I%p %:z").to_string())
        .unwrap_or_else(|| "invalid timestamp".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::unix_ms;

    #[test]
    fn system_instructions_explain_system_identity_and_tool_selection() {
        assert!(SELF_INSTRUCTIONS.contains(r#"{ "type": "Person", "name": "$system" }"#));
        assert!(SELF_INSTRUCTIONS.contains(r#"{ "type": "Person", "name": "$external_user" }"#));
        assert!(SELF_INSTRUCTIONS.contains("external untrusted user"));
        assert!(SELF_INSTRUCTIONS.contains("Available Callable Names"));
        assert!(SELF_INSTRUCTIONS.contains("tools_groups"));
        assert!(SELF_INSTRUCTIONS.contains("tools_select"));
        assert!(SELF_INSTRUCTIONS.contains(r#"{ "group": "group_id" }"#));
        assert!(SELF_INSTRUCTIONS.contains("Never invent tool parameters"));
    }

    #[test]
    fn render_system_instructions_groups_runtime_context() {
        let tools = vec![
            "shell".to_string(),
            "tools_groups".to_string(),
            "tools_select".to_string(),
        ];
        let prompt = render_system_instructions(SystemInstructionSections {
            self_knowledge: "{}",
            notes: "[]",
            available_tools: &tools,
            home_dir: "/home/anda",
            workspace: "/workspace/current",
            user_profile: "{}",
            local_date: "2026-05-09",
        });

        assert!(prompt.contains("# Runtime Context"));
        assert!(prompt.contains("## Available Callable Names"));
        assert!(prompt.contains("shell, tools_groups, tools_select"));
        assert!(prompt.contains("schemas are intentionally omitted"));
        assert!(prompt.contains("current workspace (authoritative): /workspace/current"));
    }

    #[test]
    fn format_local_date_returns_datetime_with_timezone() {
        let now_ms = unix_ms();
        let result = format_local_date(now_ms);
        println!("Formatted local date: {}", result);
        // 2026-05-12 01PM +08:00
    }

    #[test]
    fn format_available_tools_renders_none_when_empty() {
        assert_eq!(format_available_tools(&[]), "none");
        assert_eq!(
            format_available_tools(&["a".to_string(), "b".to_string()]),
            "a, b"
        );
    }

    #[test]
    fn format_local_date_handles_invalid_timestamp() {
        // i64::MAX milliseconds is far outside the representable DateTime range.
        assert_eq!(format_local_date(i64::MAX as u64), "invalid timestamp");
    }

    #[tokio::test]
    async fn available_tool_names_excludes_self_and_collects_definitions() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx();
        let names = available_tool_names(&ctx).await;
        assert!(!names.iter().any(|name| name == AndaBot::NAME));
    }
}
