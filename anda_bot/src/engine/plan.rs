//! The turn plan: a short step list the model keeps while it works on one
//! request, which clients pin above the composer so the user can follow the
//! work without reading every message.
//!
//! Steps live in the engine's session [`TodoSession`], so a compaction handoff
//! re-injects the unfinished ones. The session runner mirrors them into the
//! conversation's `extra` under [`PLAN_KEY`], and `GetConversation` /
//! `GetConversationDelta` lift that to the top level for polling clients.

use crate::util::{request_meta::keys, tool_response::ToolResponse as Response};
use anda_core::{BoxError, FunctionDefinition, RequestMeta, Resource, Tool, ToolOutput};
use anda_engine::{
    context::BaseCtx,
    extension::todo::{TodoItemInput, TodoSession, todo_session},
    memory::Conversation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{action::ActionSession, agent::AndaBot};

/// Key of the plan in a conversation's `extra` and in the `GetConversation`
/// and `GetConversationDelta` results.
pub const PLAN_KEY: &str = "plan";

const MAX_STEPS: usize = 12;
const MAX_STEP_BYTES: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    /// Reads a [`TodoSession`] status. This tool never writes `cancelled`.
    fn from_todo(status: &str) -> Self {
        match status {
            "in_progress" => Self::InProgress,
            "completed" | "cancelled" => Self::Completed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlanStep {
    pub content: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct UpdatePlanArgs {
    /// The whole plan, replacing the current one; `[]` clears it.
    #[serde(default)]
    pub items: Option<Vec<PlanStep>>,
    /// Completes the step in progress and starts the next pending one.
    #[serde(default)]
    pub advance: Option<bool>,
    /// 1-based steps to complete.
    #[serde(default)]
    pub done: Option<Vec<usize>>,
    /// 1-based step to start.
    #[serde(default)]
    pub start: Option<usize>,
}

/// Shows the user the steps of the request in progress. Only the main AndaBot
/// session writes the plan: subagents share its context state and would
/// overwrite it.
#[derive(Debug, Clone, Default)]
pub struct UpdatePlanTool;

impl UpdatePlanTool {
    pub const NAME: &'static str = "update_plan";
}

impl Tool<BaseCtx> for UpdatePlanTool {
    type Args = UpdatePlanArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Shows the user the steps of the request you are working on, pinned above the chat box: the user follows multi-step work there, not in your messages. ",
            "Use it for every request that edits files or needs more than one tool call; skip it for a single call or plain conversation.\n",
            "- Never call it alone: send it in the same response as another tool call, so it costs no extra round trip. The first call goes with the request's first tool call, even one that only reads.\n",
            "- First call: `items` holds the whole plan in order, 1-12 short imperative steps (about 7 words, no numbering), the step you are starting `in_progress`, the rest `pending`.\n",
            "- Next step: `advance: true` with the call that begins it; it completes the step in progress and starts the next pending one. Your final reply completes the last step, so it needs no call.\n",
            "- Out of order: `done` lists 1-based steps to complete, `start` names the 1-based step to begin.\n",
            "- Plan changed, including when the user redirects you mid-task: send `items` again with the whole new list.\n",
            "Keep exactly one step in progress while working, and do not repeat the plan as a list in your reply."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: update_plan_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        if ctx.agent != AndaBot::NAME {
            return Err("update_plan can only be called by the main AndaBot session".into());
        }

        let replaced = args.items.is_some();
        let session = todo_session(&ctx);
        let steps = apply(load(&session), args)?;
        store(&session, &steps)?;
        if let Some(actions) = ctx.get_state::<ActionSession>() {
            actions.publish_plan(steps.clone()).await;
        }

        Ok(ToolOutput::new(Response::Ok {
            result: json!(summary(&steps, replaced)),
            next_cursor: None,
        }))
    }
}

fn update_plan_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": ["array", "null"],
                "description": "The whole plan in order, replacing the current one. Null keeps it.",
                "items": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "A short imperative step, about 7 words."
                        },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"]
                        }
                    },
                    "required": ["content", "status"],
                    "additionalProperties": false
                }
            },
            "advance": {
                "type": ["boolean", "null"],
                "description": "True completes the step in progress and starts the next pending one."
            },
            "done": {
                "type": ["array", "null"],
                "description": "1-based steps to mark completed.",
                "items": { "type": "integer" }
            },
            "start": {
                "type": ["integer", "null"],
                "description": "1-based step to start; any other step in progress goes back to pending."
            }
        },
        "required": ["items", "advance", "done", "start"],
        "additionalProperties": false
    })
}

/// Applies `args` in order: `items`, `done`, `advance`, `start`.
fn apply(mut steps: Vec<PlanStep>, args: UpdatePlanArgs) -> Result<Vec<PlanStep>, String> {
    let advance = args.advance.unwrap_or(false);
    let done = args.done.unwrap_or_default();
    if args.items.is_none() && !advance && done.is_empty() && args.start.is_none() {
        return Err("pass items, advance, done or start".into());
    }

    if let Some(items) = args.items {
        if items.len() > MAX_STEPS {
            return Err(format!("a plan holds at most {MAX_STEPS} steps"));
        }
        steps = items
            .into_iter()
            .map(|item| {
                let content = item
                    .content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if content.is_empty() || content.len() > MAX_STEP_BYTES {
                    return Err(format!(
                        "each step needs 1-{MAX_STEP_BYTES} bytes of text; keep it a short phrase"
                    ));
                }
                Ok(PlanStep {
                    content,
                    status: item.status,
                })
            })
            .collect::<Result<_, _>>()?;
    }

    let total = steps.len();
    let index = |step: usize| {
        if step == 0 || step > total {
            Err(format!("step {step} is not in the plan of {total} steps"))
        } else {
            Ok(step - 1)
        }
    };

    for step in done {
        steps[index(step)?].status = PlanStatus::Completed;
    }

    if advance {
        if steps.is_empty() {
            return Err("there is no plan to advance; send items first".into());
        }
        let current = steps
            .iter()
            .position(|step| step.status == PlanStatus::InProgress);
        if let Some(i) = current {
            steps[i].status = PlanStatus::Completed;
        }
        let after = current.map_or(0, |i| i + 1);
        let next = steps[after..]
            .iter()
            .position(|step| step.status == PlanStatus::Pending)
            .map(|i| i + after)
            .or_else(|| {
                steps
                    .iter()
                    .position(|step| step.status == PlanStatus::Pending)
            });
        if let Some(i) = next {
            steps[i].status = PlanStatus::InProgress;
        }
    }

    if let Some(step) = args.start {
        let i = index(step)?;
        for step in steps.iter_mut() {
            if step.status == PlanStatus::InProgress {
                step.status = PlanStatus::Pending;
            }
        }
        steps[i].status = PlanStatus::InProgress;
    }

    if steps
        .iter()
        .filter(|step| step.status == PlanStatus::InProgress)
        .count()
        > 1
    {
        return Err("keep exactly one step in progress".into());
    }
    Ok(steps)
}

fn summary(steps: &[PlanStep], replaced: bool) -> String {
    let total = steps.len();
    let noun = if total == 1 { "step" } else { "steps" };
    if total == 0 {
        "Plan cleared.".to_string()
    } else if replaced {
        format!("Plan shown ({total} {noun}).")
    } else if let Some(i) = steps
        .iter()
        .position(|step| step.status == PlanStatus::InProgress)
    {
        format!("Plan updated: step {} of {total} is in progress.", i + 1)
    } else if steps
        .iter()
        .all(|step| step.status == PlanStatus::Completed)
    {
        format!("Plan updated: all {total} {noun} are done.")
    } else {
        "Plan updated: no step is in progress.".to_string()
    }
}

fn load(session: &TodoSession) -> Vec<PlanStep> {
    session
        .snapshot()
        .into_iter()
        .map(|item| PlanStep {
            status: PlanStatus::from_todo(&item.status),
            content: item.content,
        })
        .collect()
}

/// Stores the steps with their 1-based positions as ids, so the handoff after
/// a compaction lists them by the numbers `done` and `start` take.
fn store(session: &TodoSession, steps: &[PlanStep]) -> Result<(), String> {
    session
        .set(
            steps
                .iter()
                .enumerate()
                .map(|(i, step)| TodoItemInput {
                    id: (i + 1).to_string(),
                    content: Some(step.content.clone()),
                    status: Some(step.status.as_str().to_string()),
                })
                .collect(),
        )
        .map(|_| ())
}

/// Whether a client shows the plan for this request. IM channels relay text
/// only, and nobody watches cron jobs or MCP automations as they run.
pub(crate) fn shown_for(meta: &RequestMeta) -> bool {
    meta.get_extra_as::<String>(keys::REPLY_TARGET).is_none()
        && meta.get_extra_as::<u64>(keys::CRON_JOB_ID).is_none()
        && meta.get_extra_as::<u64>(keys::MCP_TRIGGER_ID).is_none()
}

/// Writes `steps` to the conversation's `extra`, removing the key for an
/// empty plan. Returns whether anything changed.
pub(crate) fn save(conversation: &mut Conversation, steps: &[PlanStep]) -> bool {
    let next = (!steps.is_empty()).then(|| json!(steps));
    let current = conversation
        .extra
        .as_ref()
        .and_then(|extra| extra.get(PLAN_KEY));
    if current == next.as_ref() {
        return false;
    }
    match (&mut conversation.extra, next) {
        (Some(Value::Object(extra)), Some(value)) => {
            extra.insert(PLAN_KEY.to_string(), value);
        }
        (Some(Value::Object(extra)), None) => {
            extra.remove(PLAN_KEY);
        }
        (extra @ None, Some(value)) => *extra = Some(json!({ PLAN_KEY: value })),
        _ => return false,
    }
    true
}

/// Mirrors the session's plan into the conversation, as after a compaction
/// moved the work to a child conversation.
pub(crate) fn record(base: &BaseCtx, conversation: &mut Conversation) -> bool {
    let steps = base
        .get_state::<TodoSession>()
        .map(|session| load(&session))
        .unwrap_or_default();
    save(conversation, &steps)
}

/// Drops the plan of the previous request when a new one starts.
pub(crate) fn clear(base: &BaseCtx, conversation: &mut Conversation) -> bool {
    if let Some(session) = base.get_state::<TodoSession>() {
        let _ = session.set(Vec::new());
    }
    save(conversation, &[])
}

/// The model ended its turn: its final reply completes the step in progress
/// when no step is left pending. Steps it stopped before stay as they are.
pub(crate) fn finish_turn(base: &BaseCtx, conversation: &mut Conversation) -> bool {
    let Some(session) = base.get_state::<TodoSession>() else {
        return false;
    };
    let mut steps = load(&session);
    if steps.iter().any(|step| step.status == PlanStatus::Pending) {
        return false;
    }
    let Some(step) = steps
        .iter_mut()
        .find(|step| step.status == PlanStatus::InProgress)
    else {
        return false;
    };
    step.status = PlanStatus::Completed;
    if store(&session, &steps).is_err() {
        return false;
    }
    save(conversation, &steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::action::{ActionEvent, ActionRuntime},
        util::json_schema::assert_openai_strict_parameters,
    };
    use PlanStatus::*;
    use anda_core::StateFeatures;
    use anda_engine::{engine::EngineBuilder, model::Models};
    use std::sync::{Arc, atomic::AtomicU64};

    fn plan(statuses: &[PlanStatus]) -> Vec<PlanStep> {
        statuses
            .iter()
            .enumerate()
            .map(|(i, status)| PlanStep {
                content: format!("Step {}", i + 1),
                status: *status,
            })
            .collect()
    }

    fn statuses(steps: &[PlanStep]) -> Vec<PlanStatus> {
        steps.iter().map(|step| step.status).collect()
    }

    #[test]
    fn parameters_fit_strict_mode() {
        assert_openai_strict_parameters(&update_plan_parameters());
    }

    #[test]
    fn items_replace_the_plan_and_normalize_text() {
        let steps = apply(
            plan(&[Completed]),
            UpdatePlanArgs {
                items: Some(vec![
                    PlanStep {
                        content: "  Read   the\nconfig ".into(),
                        status: InProgress,
                    },
                    PlanStep {
                        content: "Fix it".into(),
                        status: Pending,
                    },
                ]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(steps[0].content, "Read the config");
        assert_eq!(statuses(&steps), [InProgress, Pending]);
        assert_eq!(summary(&steps, true), "Plan shown (2 steps).");
    }

    #[test]
    fn advance_moves_to_the_next_pending_step() {
        let advance = || UpdatePlanArgs {
            advance: Some(true),
            ..Default::default()
        };
        let steps = apply(plan(&[InProgress, Pending, Pending]), advance()).unwrap();
        assert_eq!(statuses(&steps), [Completed, InProgress, Pending]);
        assert_eq!(
            summary(&steps, false),
            "Plan updated: step 2 of 3 is in progress."
        );

        // A step skipped earlier is picked up once the later ones are done.
        let steps = apply(plan(&[Pending, Completed, InProgress]), advance()).unwrap();
        assert_eq!(statuses(&steps), [InProgress, Completed, Completed]);

        let steps = apply(plan(&[Completed, InProgress]), advance()).unwrap();
        assert_eq!(statuses(&steps), [Completed, Completed]);
        assert_eq!(
            summary(&steps, false),
            "Plan updated: all 2 steps are done."
        );

        assert!(apply(Vec::new(), advance()).is_err());
    }

    #[test]
    fn done_and_start_reorder_work() {
        let steps = apply(
            plan(&[Completed, InProgress, Pending, Pending]),
            UpdatePlanArgs {
                done: Some(vec![2]),
                start: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            statuses(&steps),
            [Completed, Completed, Pending, InProgress]
        );

        // Starting a step sends the one in progress back to pending.
        let steps = apply(
            plan(&[InProgress, Pending]),
            UpdatePlanArgs {
                start: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(statuses(&steps), [Pending, InProgress]);
    }

    #[test]
    fn rejects_invalid_updates() {
        let current = plan(&[InProgress, Pending]);
        let err = |args| apply(current.clone(), args).unwrap_err();

        assert!(err(UpdatePlanArgs::default()).contains("pass items"));
        assert!(
            err(UpdatePlanArgs {
                done: Some(vec![3]),
                ..Default::default()
            })
            .contains("step 3")
        );
        assert!(
            err(UpdatePlanArgs {
                start: Some(0),
                ..Default::default()
            })
            .contains("step 0")
        );
        assert!(
            err(UpdatePlanArgs {
                items: Some(plan(&[InProgress, InProgress])),
                ..Default::default()
            })
            .contains("one step in progress")
        );
        assert!(
            err(UpdatePlanArgs {
                items: Some(plan(&[Pending; MAX_STEPS + 1])),
                ..Default::default()
            })
            .contains("at most")
        );
        assert!(
            err(UpdatePlanArgs {
                items: Some(vec![PlanStep {
                    content: " ".into(),
                    status: Pending,
                }]),
                ..Default::default()
            })
            .contains("bytes of text")
        );
    }

    #[test]
    fn empty_items_clear_the_plan() {
        let steps = apply(
            plan(&[InProgress]),
            UpdatePlanArgs {
                items: Some(Vec::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(steps.is_empty());
        assert_eq!(summary(&steps, true), "Plan cleared.");
    }

    #[test]
    fn todo_session_round_trip_numbers_steps() {
        let session = TodoSession::new();
        let steps = plan(&[Completed, InProgress, Pending]);
        store(&session, &steps).unwrap();
        assert_eq!(load(&session), steps);
        let ids: Vec<_> = session.snapshot().into_iter().map(|item| item.id).collect();
        assert_eq!(ids, ["1", "2", "3"]);
        // The compaction handoff lists unfinished steps by those numbers.
        let handoff = session.format_for_injection().unwrap();
        assert!(handoff.contains("2. Step 2"));
        assert!(handoff.contains("3. Step 3"));
        assert!(!handoff.contains("Step 1"));
    }

    #[test]
    fn save_writes_and_removes_the_extra_key() {
        let mut conversation = Conversation::default();
        assert!(!save(&mut conversation, &[]));
        assert!(save(&mut conversation, &plan(&[InProgress])));
        assert_eq!(
            conversation.extra,
            Some(json!({ PLAN_KEY: [{"content": "Step 1", "status": "in_progress"}] }))
        );
        assert!(!save(&mut conversation, &plan(&[InProgress])));

        conversation
            .extra
            .as_mut()
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert("source".into(), json!("desktop"));
        assert!(save(&mut conversation, &[]));
        assert_eq!(conversation.extra, Some(json!({ "source": "desktop" })));
    }

    #[test]
    fn shown_only_where_a_client_watches() {
        let meta = |key: &str, value: Value| {
            let mut meta = RequestMeta::default();
            meta.extra.insert(key.to_string(), value);
            meta
        };
        assert!(shown_for(&RequestMeta::default()));
        assert!(shown_for(&meta(keys::SOURCE, json!("desktop:/tmp"))));
        assert!(!shown_for(&meta(keys::REPLY_TARGET, json!("chat-1"))));
        assert!(!shown_for(&meta(keys::CRON_JOB_ID, json!(7))));
        assert!(!shown_for(&meta(keys::MCP_TRIGGER_ID, json!(3))));
    }

    #[test]
    fn finish_turn_completes_only_the_last_step() {
        let base = EngineBuilder::new().mock_ctx().base;
        let session = todo_session(&base);
        let mut conversation = Conversation::default();

        store(&session, &plan(&[Completed, InProgress, Pending])).unwrap();
        assert!(!finish_turn(&base, &mut conversation));
        assert_eq!(statuses(&load(&session)), [Completed, InProgress, Pending]);

        store(&session, &plan(&[Completed, InProgress])).unwrap();
        assert!(finish_turn(&base, &mut conversation));
        assert_eq!(statuses(&load(&session)), [Completed, Completed]);
        assert_eq!(
            conversation.extra.unwrap()[PLAN_KEY][1]["status"],
            "completed"
        );
    }

    #[test]
    fn clear_and_record_follow_the_session() {
        let base = EngineBuilder::new().mock_ctx().base;
        let session = todo_session(&base);
        let mut conversation = Conversation::default();

        store(&session, &plan(&[InProgress])).unwrap();
        assert!(record(&base, &mut conversation));
        assert!(conversation.extra.as_ref().unwrap().get(PLAN_KEY).is_some());

        assert!(clear(&base, &mut conversation));
        assert!(load(&session).is_empty());
        assert!(conversation.extra.as_ref().unwrap().get(PLAN_KEY).is_none());
    }

    #[tokio::test]
    async fn tool_stores_the_plan_and_publishes_it() {
        let mut ctx = EngineBuilder::new().mock_ctx().base;
        let args = || UpdatePlanArgs {
            items: Some(plan(&[InProgress, Pending])),
            ..Default::default()
        };

        let err = UpdatePlanTool
            .call(ctx.clone(), args(), Vec::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("main AndaBot session"), "{err}");

        ctx.agent = AndaBot::NAME.to_string();
        let (event_sender, mut event_rx) = tokio::sync::mpsc::channel(4);
        ctx.set_state(ActionSession::new(
            Arc::new(ActionRuntime::new()),
            event_sender,
            ctx.caller().to_text(),
            "session_1".to_string(),
            Arc::new(AtomicU64::new(1)),
            Arc::new(Models::default()),
            std::env::temp_dir(),
        ));
        let output = UpdatePlanTool
            .call(ctx.clone(), args(), Vec::new())
            .await
            .unwrap();
        assert_eq!(
            output.output,
            Response::Ok {
                result: json!("Plan shown (2 steps)."),
                next_cursor: None,
            }
        );
        assert_eq!(load(&todo_session(&ctx)), plan(&[InProgress, Pending]));
        let Some(ActionEvent::Plan(steps)) = event_rx.recv().await else {
            panic!("expected a plan event");
        };
        assert_eq!(steps, plan(&[InProgress, Pending]));
    }
}
