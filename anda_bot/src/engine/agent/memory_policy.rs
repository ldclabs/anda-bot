//! Immutable, host-owned policy. A prompt or tool argument cannot relax it.
use anda_core::BoxError;
use anda_engine::{
    context::{AgentCtx, BaseCtx},
    hook::Hook,
    memory::Conversation,
};
use async_trait::async_trait;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::Arc;

pub struct MemoryPolicyTool<T>(Arc<T>, Option<Arc<crate::brain::MemoryAccess>>);
impl<T> MemoryPolicyTool<T> {
    pub fn new(inner: Arc<T>) -> Self {
        Self(inner, None)
    }
    pub fn with_access(mut self, access: Arc<crate::brain::MemoryAccess>) -> Self {
        self.1 = Some(access);
        self
    }
}
impl<T: anda_core::Tool<BaseCtx>> anda_core::Tool<BaseCtx> for MemoryPolicyTool<T> {
    type Args = T::Args;
    type Output = T::Output;
    fn name(&self) -> String {
        self.0.name()
    }
    fn description(&self) -> String {
        self.0.description()
    }
    fn definition(&self) -> anda_core::FunctionDefinition {
        self.0.definition()
    }
    fn group(&self) -> Option<anda_core::ToolGroupInfo> {
        self.0.group()
    }
    fn supported_resource_tags(&self) -> Vec<String> {
        self.0.supported_resource_tags()
    }
    async fn init(&self, ctx: BaseCtx) -> Result<(), BoxError> {
        self.0.init(ctx).await
    }
    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<anda_core::Resource>,
    ) -> Result<anda_core::ToolOutput<Self::Output>, BoxError> {
        if !MemoryPolicy::current(&ctx).allows_tool(&self.name()) {
            return Err(
                "This tool is disabled by the conversation's persistent memory policy.".into(),
            );
        }
        let _guard = if let Some(access) = &self.1 {
            let guard = access.gate.lock().await;
            access.check_locked(&ctx).await?;
            Some(guard)
        } else {
            None
        };
        self.0.call(ctx, args, resources).await
    }
}

pub struct MemoryPolicyAgent<T>(Arc<T>);
impl<T> MemoryPolicyAgent<T> {
    pub fn new(inner: Arc<T>) -> Self {
        Self(inner)
    }
}
impl<T: anda_core::Agent<AgentCtx>> anda_core::Agent<AgentCtx> for MemoryPolicyAgent<T> {
    fn name(&self) -> String {
        self.0.name()
    }
    fn description(&self) -> String {
        self.0.description()
    }
    fn definition(&self) -> anda_core::FunctionDefinition {
        self.0.definition()
    }
    fn group(&self) -> Option<anda_core::ToolGroupInfo> {
        self.0.group()
    }
    fn supported_resource_tags(&self) -> Vec<String> {
        self.0.supported_resource_tags()
    }
    fn tool_dependencies(&self) -> Vec<String> {
        self.0.tool_dependencies()
    }
    async fn init(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        self.0.init(ctx).await
    }
    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<anda_core::Resource>,
    ) -> Result<anda_core::AgentOutput, BoxError> {
        if !MemoryPolicy::current(&ctx.base).may_write() {
            return Err("Nested agents are unavailable in restricted memory mode.".into());
        }
        let mut output = self.0.run(ctx.clone(), prompt, resources).await?;
        if let Some(artifacts) = ctx
            .base
            .get_state::<crate::engine::resources::SessionArtifacts>()
        {
            use anda_core::StateFeatures;
            artifacts
                .record(ctx.caller(), &mut output.artifacts)
                .await?;
        }
        Ok(output)
    }
}

pub const POLICY_KEY: &str = "memory_policy";
pub const MODE_KEY: &str = "memory_mode";

#[derive(Clone, Default)]
pub struct InheritedMemorySources(pub Vec<String>);

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    #[default]
    Standard,
    NoStore,
    Off,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPolicy {
    pub version: u32,
    pub mode: MemoryMode,
    pub revision: String,
    pub created_at: u64,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            mode: MemoryMode::Standard,
            revision: "legacy".into(),
            created_at: 0,
        }
    }
}
impl MemoryPolicy {
    pub fn new(mode: MemoryMode) -> Self {
        Self {
            version: 1,
            mode,
            revision: ic_auth_types::Xid::new().to_string(),
            created_at: anda_engine::unix_ms(),
        }
    }
    pub fn from_conversation(conversation: &Conversation) -> Result<Self, BoxError> {
        let Some(value) = conversation.extra.as_ref().and_then(|v| v.get(POLICY_KEY)) else {
            return Ok(Self::default());
        };
        let policy: Self = serde_json::from_value(value.clone())?;
        if policy.version != 1 {
            return Err("Unsupported persistent memory policy version".into());
        }
        Ok(policy)
    }
    pub fn current(ctx: &BaseCtx) -> Self {
        ctx.get_state::<Self>().unwrap_or_default()
    }
    pub fn may_write(&self) -> bool {
        self.mode == MemoryMode::Standard
    }
    pub fn may_read(&self) -> bool {
        self.mode != MemoryMode::Off
    }
    pub fn persist(&self, extra: &mut Map<String, Value>) {
        extra.remove(MODE_KEY);
        extra.insert(POLICY_KEY.into(), serde_json::json!(self));
    }
    pub fn allows_tool(&self, name: &str) -> bool {
        if self.may_write() {
            return true;
        }
        if matches!(
            name,
            "note"
                | "brain_respond"
                | "brain_attention"
                | "brain_runtime_status"
                | "brain_feedback"
                | "create_cron_job"
                | "update_cron_job"
                | "manage_cron_job"
                | "bookmarks_api"
        ) {
            return false;
        }
        self.may_read() || name != crate::brain::Client::NAME
    }
}

pub struct MemoryPolicyHook;
#[async_trait]
impl Hook for MemoryPolicyHook {
    async fn on_tool_start(&self, ctx: &BaseCtx, tool: &str) -> Result<(), BoxError> {
        if !MemoryPolicy::current(ctx).allows_tool(tool) {
            return Err(
                "This tool is disabled by the conversation's persistent memory policy.".into(),
            );
        }
        Ok(())
    }
    async fn on_agent_start(&self, ctx: &AgentCtx, agent: &str) -> Result<(), BoxError> {
        if !MemoryPolicy::current(&ctx.base).may_write() && agent != super::AndaBot::NAME {
            return Err("Nested agents are unavailable in restricted memory mode until their persistence paths are verified.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::{AgentContext, ToolInput};

    #[tokio::test]
    async fn memory_policy_blocks_notes_through_the_actual_model_tool_dispatch_path() {
        let ctx = anda_engine::engine::EngineBuilder::new()
            .register_tool(Arc::new(MemoryPolicyTool::new(Arc::new(
                anda_engine::extension::note::NoteTool::new(),
            ))))
            .unwrap()
            .mock_ctx();
        ctx.base.set_state(MemoryPolicy::new(MemoryMode::NoStore));
        let input = ToolInput::new(
            "note".into(),
            serde_json::json!({"op":"upsert","items":[{"id":"test","content":"must not persist"}]}),
        );
        assert!(ctx.tool_call(input).await.is_err());
        assert!(
            anda_engine::extension::note::load_notes(&ctx)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }
    #[test]
    fn memory_policy_persistence_is_versioned_and_legacy_defaults_do_not_override_restrictions() {
        let mut conversation = Conversation::default();
        assert!(
            MemoryPolicy::from_conversation(&conversation)
                .unwrap()
                .may_write()
        );
        let mut extra = Map::new();
        MemoryPolicy::new(MemoryMode::Off).persist(&mut extra);
        conversation.extra = Some(Value::Object(extra));
        assert!(
            !MemoryPolicy::from_conversation(&conversation)
                .unwrap()
                .may_read()
        );
        conversation.extra.as_mut().unwrap()[POLICY_KEY]["version"] = 99.into();
        assert!(MemoryPolicy::from_conversation(&conversation).is_err());
    }
    #[tokio::test]
    async fn memory_policy_inherits_into_child_contexts_and_blocks_execution_not_just_schemas() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx();
        ctx.base.set_state(MemoryPolicy::new(MemoryMode::Off));
        let child = ctx.child("child", "").unwrap();
        assert!(!MemoryPolicy::current(&child.base).may_read());
        for tool in ["recall_memory", "note", "brain_respond", "create_cron_job"] {
            assert!(
                MemoryPolicyHook
                    .on_tool_start(&child.base, tool)
                    .await
                    .is_err()
            );
        }
        assert!(
            MemoryPolicyHook
                .on_agent_start(&child, "child")
                .await
                .is_err()
        );
    }
}
