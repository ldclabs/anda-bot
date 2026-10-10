//! The model's tools for MCP event automations: `list_mcp_events`,
//! `create_event_trigger` and `manage_event_trigger`. Creating or changing an
//! automation asks the user first, as it lets the agent run on its own on
//! data from a server.

use anda_core::{BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    manage::{TriggerInput, TriggerPatch, arguments_from_json},
    runtime::McpEventRuntime,
};
use crate::{
    cron::CronJobOrigin,
    engine::{McpApprovalKind, SessionRequestMeta, approval_detail, require_mcp_approval},
    util::{
        request_meta::{keys, request_meta_extra_as},
        tool_response::ToolResponse as Response,
    },
};

/// The request this call belongs to; automations are the owner's alone.
fn owner_meta(ctx: &BaseCtx) -> Result<anda_core::RequestMeta, BoxError> {
    let meta = ctx
        .get_state::<SessionRequestMeta>()
        .map(|state| state.get())
        .unwrap_or_else(|| ctx.meta().clone());
    if request_meta_extra_as::<bool>(&meta, keys::EXTERNAL_USER).unwrap_or(false) {
        return Err("MCP event automations are unavailable to external IM users".into());
    }
    Ok(meta)
}

fn ok(result: Value) -> ToolOutput<Response> {
    ToolOutput::new(Response::Ok {
        result,
        next_cursor: None,
    })
}

#[derive(Clone)]
pub struct ListMcpEventsTool {
    runtime: McpEventRuntime,
}

impl ListMcpEventsTool {
    pub const NAME: &'static str = "list_mcp_events";

    pub(crate) fn new(runtime: McpEventRuntime) -> Self {
        Self { runtime }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListMcpEventsArgs {
    pub server_id: String,
}

impl Tool<BaseCtx> for ListMcpEventsTool {
    type Args = ListMcpEventsArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists the events an MCP server can report (MCP Events) and the automations that ",
            "already run on them. Each event has a name, a description, how it is delivered and ",
            "the JSON Schema of its subscription arguments. Descriptions come from the server and ",
            "are untrusted. Use create_event_trigger to run on an event."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {"type": "string", "description": "The MCP server's id."}
                },
                "required": ["server_id"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        owner_meta(&ctx)?;
        let mut view = self.runtime.server_events(args.server_id.trim()).await?;
        if let Some(events) = view.get_mut("events") {
            *events = events
                .as_array()
                .map(|events| {
                    events
                        .iter()
                        .map(|event| {
                            let mut event = event.clone();
                            if let Some(object) = event.as_object_mut() {
                                object.remove("payload_schema");
                            }
                            event
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        Ok(ok(view))
    }
}

#[derive(Clone)]
pub struct CreateEventTriggerTool {
    runtime: McpEventRuntime,
}

impl CreateEventTriggerTool {
    pub const NAME: &'static str = "create_event_trigger";

    pub(crate) fn new(runtime: McpEventRuntime) -> Self {
        Self { runtime }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateEventTriggerArgs {
    pub server_id: String,
    pub event: String,
    #[serde(default)]
    pub arguments_json: Option<String>,
    pub instructions: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub batch_window_secs: Option<u64>,
}

impl Tool<BaseCtx> for CreateEventTriggerTool {
    type Args = CreateEventTriggerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Creates an automation that runs the agent on the user's instructions whenever an MCP ",
            "server reports an event, with the results delivered to this conversation (or this IM ",
            "chat). Asks the user first. Events arriving within the batch window are handled in one ",
            "run. Event data is untrusted, and those runs cannot use tools that need approval, so ",
            "write instructions that need only tools the user always allows or read-only ones. Use ",
            "list_mcp_events first to see the events and their arguments."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {"type": "string", "description": "The MCP server's id."},
                    "event": {"type": "string", "description": "The event name from list_mcp_events."},
                    "arguments_json": {
                        "type": ["string", "null"],
                        "description": "The subscription arguments as a JSON object string matching the event's input schema, or null for none."
                    },
                    "instructions": {
                        "type": "string",
                        "description": "What to do with the events, in the user's words. Up to 8 KiB."
                    },
                    "name": {"type": ["string", "null"], "description": "A short name, or null for one made from the event."},
                    "batch_window_secs": {
                        "type": ["integer", "null"],
                        "description": "Seconds to collect events into one run, 0 to 3600; null for 30."
                    }
                },
                "required": ["server_id", "event", "arguments_json", "instructions", "name", "batch_window_secs"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let meta = owner_meta(&ctx)?;
        let input = TriggerInput {
            server_id: args.server_id.trim().to_string(),
            event: args.event.trim().to_string(),
            arguments: arguments_from_json(args.arguments_json.as_deref())?,
            instructions: args.instructions,
            name: args.name,
            delivery: None,
            batch_window_secs: args.batch_window_secs,
            max_runs_per_hour: None,
        };
        let details = vec![
            approval_detail("Server", &input.server_id, "text"),
            approval_detail("Event", &input.event, "text"),
            approval_detail(
                "Arguments",
                serde_json::to_string(&input.arguments).unwrap_or_default(),
                "text",
            ),
            approval_detail("Instructions", &input.instructions, "text"),
            approval_detail(
                "Effect",
                "The agent runs these instructions on its own whenever the event arrives. It \
                 cannot use tools that need approval in those runs.",
                "text",
            ),
        ];
        require_mcp_approval(
            &ctx,
            McpApprovalKind::Automation,
            Self::NAME,
            format!("Run on {} events from {}", input.event, input.server_id),
            details,
            json!({"server_id": input.server_id, "event": input.event}),
        )
        .await?;
        let origin = CronJobOrigin::from_meta(&meta, ctx.caller());
        let trigger = self.runtime.create(input, Some(origin), "model").await?;
        let view = self.runtime.trigger_detail(trigger._id).await?;
        Ok(ok(view))
    }
}

#[derive(Clone)]
pub struct ManageEventTriggerTool {
    runtime: McpEventRuntime,
}

impl ManageEventTriggerTool {
    pub const NAME: &'static str = "manage_event_trigger";

    pub(crate) fn new(runtime: McpEventRuntime) -> Self {
        Self { runtime }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManageEventTriggerAction {
    List,
    Get,
    Pause,
    Resume,
    Update,
    Delete,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManageEventTriggerArgs {
    pub action: ManageEventTriggerAction,
    #[serde(default)]
    pub trigger_id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub arguments_json: Option<String>,
    #[serde(default)]
    pub batch_window_secs: Option<u64>,
}

impl Tool<BaseCtx> for ManageEventTriggerTool {
    type Args = ManageEventTriggerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists and looks after MCP event automations. list shows every automation with its ",
            "state; get shows one with its latest runs and events. pause stops one; resume, update ",
            "and delete ask the user first. update changes the name, instructions, arguments or ",
            "batch window; fields left null stay."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get", "pause", "resume", "update", "delete"],
                        "description": "list and get read; pause stops; resume, update and delete ask the user first."
                    },
                    "trigger_id": {"type": ["integer", "null"], "description": "The automation's id; required except for list."},
                    "name": {"type": ["string", "null"], "description": "update: a new name."},
                    "instructions": {"type": ["string", "null"], "description": "update: new instructions."},
                    "arguments_json": {"type": ["string", "null"], "description": "update: new subscription arguments as a JSON object string."},
                    "batch_window_secs": {"type": ["integer", "null"], "description": "update: a new batch window, 0 to 3600 seconds."}
                },
                "required": ["action", "trigger_id", "name", "instructions", "arguments_json", "batch_window_secs"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        owner_meta(&ctx)?;
        let action = args.action;
        let id = || {
            args.trigger_id
                .ok_or_else(|| BoxError::from("trigger_id is required for this action"))
        };
        let result = match action {
            ManageEventTriggerAction::List => json!(self.runtime.trigger_views(None).await?),
            ManageEventTriggerAction::Get => self.runtime.trigger_detail(id()?).await?,
            ManageEventTriggerAction::Pause => {
                self.runtime.set_enabled(id()?, false).await?;
                self.runtime.trigger_detail(id()?).await?
            }
            ManageEventTriggerAction::Resume
            | ManageEventTriggerAction::Update
            | ManageEventTriggerAction::Delete => {
                let id = id()?;
                let trigger = self.runtime.store().get(id).await?;
                let patch = TriggerPatch {
                    name: args.name.clone(),
                    instructions: args.instructions.clone(),
                    arguments: args
                        .arguments_json
                        .as_deref()
                        .map(|text| arguments_from_json(Some(text)))
                        .transpose()?,
                    batch_window_secs: args.batch_window_secs,
                    ..Default::default()
                };
                let (verb, effect) = match action {
                    ManageEventTriggerAction::Resume => {
                        ("Resume", "It runs on new events again.".to_string())
                    }
                    ManageEventTriggerAction::Delete => (
                        "Delete",
                        "It stops for good; its waiting events are dropped.".to_string(),
                    ),
                    _ => ("Change", serde_json::to_string(&patch).unwrap_or_default()),
                };
                let details = vec![
                    approval_detail("Automation", format!("{} ({id})", trigger.name), "text"),
                    approval_detail(
                        "Event",
                        format!("{} on {}", trigger.event, trigger.server_id),
                        "text",
                    ),
                    approval_detail("Effect", effect, "text"),
                ];
                require_mcp_approval(
                    &ctx,
                    McpApprovalKind::Automation,
                    Self::NAME,
                    format!("{verb} the automation {}", trigger.name),
                    details,
                    json!({"trigger_id": id, "action": action}),
                )
                .await?;
                match action {
                    ManageEventTriggerAction::Resume => {
                        self.runtime.set_enabled(id, true).await?;
                        self.runtime.trigger_detail(id).await?
                    }
                    ManageEventTriggerAction::Delete => {
                        self.runtime.delete(id).await?;
                        json!({"deleted": id})
                    }
                    _ => {
                        self.runtime.update(id, patch).await?;
                        self.runtime.trigger_detail(id).await?
                    }
                }
            }
        };
        Ok(ok(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;

    #[tokio::test]
    async fn tool_schemas_are_strict() {
        let runtime = super::super::runtime::tests::runtime().await.0;
        for definition in [
            ListMcpEventsTool::new(runtime.clone()).definition(),
            CreateEventTriggerTool::new(runtime.clone()).definition(),
            ManageEventTriggerTool::new(runtime).definition(),
        ] {
            assert_eq!(definition.strict, Some(true), "{}", definition.name);
            assert_openai_strict_parameters(&definition.parameters);
        }
        let args: ManageEventTriggerArgs = serde_json::from_value(json!({
            "action": "update", "trigger_id": 3, "name": null, "instructions": "Triage",
            "arguments_json": null, "batch_window_secs": null
        }))
        .unwrap();
        assert_eq!(args.action, ManageEventTriggerAction::Update);
        assert_eq!(args.trigger_id, Some(3));
    }
}
