use super::{AttentionQuery, AttentionResponse, Host};
use anda_core::{BoxError, FunctionDefinition, Json, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use serde::Deserialize;
use serde_json::json;

use crate::{
    engine::SessionRequestMeta,
    util::{
        request_meta::{keys, request_meta_extra_as},
        tool_response::ToolResponse as Response,
    },
};

#[derive(Clone, Copy)]
pub enum RuntimeOperation {
    Attention,
    Respond,
    Status,
    Feedback,
}
#[derive(Clone)]
pub struct RuntimeTool {
    host: Host,
    operation: RuntimeOperation,
}
impl RuntimeTool {
    pub const NAMES: [&'static str; 4] = [
        "brain_attention",
        "brain_respond",
        "brain_runtime_status",
        "brain_feedback",
    ];
    pub fn new(host: Host, operation: RuntimeOperation) -> Self {
        Self { host, operation }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseArgs {
    id: String,
    response: ResponseInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedbackArgs {
    statement: String,
    decision_ref: Option<String>,
    attempt_ref: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseInput {
    kind: String,
    event_key: String,
    answer: Option<String>,
    statement: Option<String>,
}
impl ResponseInput {
    fn into_native(self) -> Result<AttentionResponse, BoxError> {
        match (self.kind.as_str(), self.answer, self.statement) {
            ("clarification", Some(answer), None) => Ok(AttentionResponse::Clarification {
                event_key: self.event_key,
                answer,
            }),
            ("agent_statement", None, Some(statement)) => Ok(AttentionResponse::AgentStatement {
                event_key: self.event_key,
                statement,
            }),
            _ => Err(
                "provide only answer for clarification, or only statement for agent_statement"
                    .into(),
            ),
        }
    }
}

impl Tool<BaseCtx> for RuntimeTool {
    type Args = Json;
    type Output = Response;
    fn name(&self) -> String {
        match self.operation {
            RuntimeOperation::Attention => Self::NAMES[0],
            RuntimeOperation::Respond => Self::NAMES[1],
            RuntimeOperation::Status => Self::NAMES[2],
            RuntimeOperation::Feedback => Self::NAMES[3],
        }
        .into()
    }
    fn description(&self) -> String {
        match self.operation {
            RuntimeOperation::Attention => "Read your durable Brain inbox and the memory attention raised since you last looked (due commitments, fired watches). Reading is not a claim or execution permission. A completed page is not completed work. If a cursor expires, restart without it.",
            RuntimeOperation::Respond => "Answer a Brain clarification or record an attributed statement. Use the item's returned id and keep the same event_key AND text when retrying. An answer grants no authority and a statement is not an independent outcome.",
            RuntimeOperation::Status => "Inspect optional attention, action and learning runtime configuration, inbox visibility and the Memory Interface levels Brain advertises. Visible items count inbox entries. Memory recall availability and migration completion require separate checks; configured=false alone does not indicate missing memories.",
            RuntimeOperation::Feedback => "Record your own report about how a decision or attempt went, in your words, as attributed agent evidence. It is kept with its origin, never becomes a grade, outcome or verified fact, and changes no permission. Use decision_ref/attempt_ref only with ids Brain returned.",
        }.into()
    }
    fn definition(&self) -> FunctionDefinition {
        let parameters = match self.operation {
            RuntimeOperation::Attention => {
                json!({"type":"object","properties":{"cursor":{"type":["string","null"]},"limit":{"type":["integer","null"],"minimum":1,"maximum":50}},"required":["cursor","limit"],"additionalProperties":false})
            }
            RuntimeOperation::Status => {
                json!({"type":"object","properties":{},"required":[],"additionalProperties":false})
            }
            RuntimeOperation::Feedback => {
                json!({"type":"object","properties":{"statement":{"type":"string","minLength":1,"maxLength":8192},"decision_ref":{"type":["string","null"]},"attempt_ref":{"type":["string","null"]}},"required":["statement","decision_ref","attempt_ref"],"additionalProperties":false})
            }
            RuntimeOperation::Respond => {
                json!({"type":"object","properties":{"id":{"type":"string"},"response":{
                "type":"object","properties":{"kind":{"type":"string","enum":["clarification","agent_statement"]},"event_key":{"type":"string"},"answer":{"type":["string","null"]},"statement":{"type":["string","null"]}},"required":["kind","event_key","answer","statement"],"additionalProperties":false
            }},"required":["id","response"],"additionalProperties":false})
            }
        };
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters,
            strict: Some(true),
        }
    }
    async fn call(
        &self,
        ctx: BaseCtx,
        args: Json,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        if !crate::engine::MemoryPolicy::current(&ctx).allows_tool(&self.name()) {
            return Err(
                "Memory runtime actions are disabled by this conversation's memory policy.".into(),
            );
        }
        // IM runtimes may execute under an owner's engine identity for routing.
        // That never promotes an external sender to a native inbox recipient.
        let meta = ctx
            .get_state::<SessionRequestMeta>()
            .map(|state| state.get())
            .unwrap_or_else(|| ctx.meta().clone());
        if request_meta_extra_as::<bool>(&meta, keys::EXTERNAL_USER).unwrap_or(false) {
            return Err("external users cannot access the owner's Brain runtime".into());
        }
        let caller = *ctx.caller();
        let conversation = ctx
            .get_state::<super::RecallTurn>()
            .map(|t| t.0.lock().conversation)
            .filter(|id| *id > 0);
        let (result, next_cursor) = match self.operation {
            RuntimeOperation::Attention => {
                // Memory attention needs no runtime bindings; the inbox does.
                let query = serde_json::from_value::<AttentionQuery>(args)?;
                let (mut result, next_cursor) = if self.host.runtime_installed().await? {
                    let mut page = self.host.attention(caller, query).await?;
                    self.host
                        .associate_attention(&page, caller, conversation, &meta)
                        .await?;
                    let next_cursor = page.next_cursor.take();
                    (serde_json::to_value(page)?, next_cursor)
                } else {
                    (json!({"items":[],"inbox":"not_configured"}), None)
                };
                result["memory_attention"] =
                    self.host.memory_attention(&caller.to_string()).await?;
                (result, next_cursor)
            }
            RuntimeOperation::Feedback => {
                let args: FeedbackArgs = serde_json::from_value(args)?;
                let response = self
                    .host
                    .memory_feedback(
                        &caller.to_string(),
                        conversation,
                        args.statement,
                        args.decision_ref,
                        args.attempt_ref,
                    )
                    .await?;
                if let Some(error) = super::memory::failure(&response) {
                    return Err(error);
                }
                (
                    json!({"receipt": response.receipt, "progress": response.progress, "status": response.status}),
                    None,
                )
            }
            RuntimeOperation::Respond => {
                let args: ResponseArgs = serde_json::from_value(args)?;
                (
                    serde_json::to_value(
                        self.host
                            .respond(caller, args.id, args.response.into_native()?)
                            .await?,
                    )?,
                    None,
                )
            }
            RuntimeOperation::Status => {
                if args != json!({}) {
                    return Err("brain_runtime_status accepts an empty object".into());
                }
                let mut status = serde_json::to_value(self.host.status(caller).await?)?;
                status["memory_interface"] = match self.host.memory_descriptor().await {
                    Ok(descriptor) => serde_json::to_value(descriptor)?,
                    Err(error) => json!({"error": error.to_string()}),
                };
                (status, None)
            }
        };
        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor,
        }))
    }
}
