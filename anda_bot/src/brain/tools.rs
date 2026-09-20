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
}
#[derive(Clone)]
pub struct RuntimeTool {
    host: Host,
    operation: RuntimeOperation,
}
impl RuntimeTool {
    pub const NAMES: [&'static str; 3] =
        ["brain_attention", "brain_respond", "brain_runtime_status"];
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
        }
        .into()
    }
    fn description(&self) -> String {
        match self.operation {
            RuntimeOperation::Attention => "Read your durable Brain inbox. Reading is not a claim or execution permission. A completed page is not completed work. If a cursor expires, restart without it.",
            RuntimeOperation::Respond => "Answer a Brain clarification or record an attributed statement. Use the item's returned id and keep the same event_key AND text when retrying. An answer grants no authority and a statement is not an independent outcome.",
            RuntimeOperation::Status => "Read your Brain runtime configuration and bounded visible status; unavailable features are not active learning.",
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
        let (result, next_cursor) = match self.operation {
            RuntimeOperation::Attention => {
                let mut page = self
                    .host
                    .attention(caller, serde_json::from_value::<AttentionQuery>(args)?)
                    .await?;
                let conversation = ctx
                    .get_state::<super::RecallTurn>()
                    .map(|t| t.0.lock().conversation);
                self.host
                    .associate_attention(&page, caller, conversation, &meta)
                    .await?;
                let next_cursor = page.next_cursor.take();
                (serde_json::to_value(page)?, next_cursor)
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
                (serde_json::to_value(self.host.status(caller).await?)?, None)
            }
        };
        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor,
        }))
    }
}
