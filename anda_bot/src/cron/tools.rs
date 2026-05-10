use anda_core::{BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{runtime::CronRuntime, types::CreateCronJobArgs, types::CronJobOrigin};
use crate::engine::SessionRequestMeta;

#[derive(Clone)]
pub struct CreateCronTool {
    cron: Arc<CronRuntime>,
}

impl CreateCronTool {
    pub const NAME: &'static str = "create_cron_job";

    pub fn new(cron: Arc<CronRuntime>) -> Self {
        Self { cron }
    }
}

impl Tool<BaseCtx> for CreateCronTool {
    type Args = CreateCronJobArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Creates a scheduled cron job with the specified parameters. ",
            "Example 1: {\"job_kind\":\"shell\",\"job\":\"echo hello\",\"schedule_kind\":\"cron\",\"schedule\":\"0 9 * * 1-5\",\"tz\":\"Asia/Shanghai\"}. ",
            "Example 2: {\"job_kind\":\"agent\",\"job\":\"Send the daily summary to me\",\"schedule_kind\":\"every\",\"schedule\":\"30m\",\"name\":\"daily-summary\"}.",
            "Relevant tools: manage_cron_job, list_cron_jobs, list_cron_runs, shell, tools_select."
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
                    "job_kind": {
                        "type": "string",
                        "enum": ["shell", "agent"],
                        "description": "Use 'shell' to execute a shell command, or 'agent' to submit a prompt to the agent runtime."
                    },
                    "job": {
                        "type": "string",
                        "description": "The shell command or agent prompt to execute."
                    },
                    "schedule_kind": {
                        "type": "string",
                        "enum": ["cron", "at", "every", "once"],
                        "description": "How to interpret schedule."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "The schedule value. For 'cron', provide a cron expression. For 'at', provide an RFC3339 timestamp. For 'every' and 'once', provide a duration using optional s/m/h/d units, such as '60', '5m', '2h', or '1d'. When omitted, the unit defaults to seconds."
                    },
                    "name": {
                        "type": ["string", "null"],
                        "description": "Optional human-readable name for the cron job."
                    },
                    "tz": {
                        "type": ["string", "null"],
                        "description": "Optional IANA timezone name, only used when schedule_kind is 'cron'."
                    }
                },
                "required": ["job_kind", "job", "schedule_kind", "schedule"],
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
        let meta = ctx
            .get_state::<SessionRequestMeta>()
            .map(|state| state.get())
            .unwrap_or_else(|| ctx.meta().clone());
        let origin = CronJobOrigin::from_meta_with_caller(&meta, ctx.caller());
        let job = self.cron.store.insert_job(args, origin).await?;
        Ok(ToolOutput::new(Response::Ok {
            result: json!(job),
            next_cursor: None,
        }))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CronJobAction {
    Get,
    Pause,
    Resume,
    Remove,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManageCronJobArgs {
    pub action: CronJobAction,
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ListCronArgs {
    pub job_id: Option<u64>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

fn paginated_response<T>(items: T, next_cursor: Option<String>) -> ToolOutput<Response>
where
    T: Serialize,
{
    ToolOutput::new(Response::Ok {
        result: json!(items),
        next_cursor,
    })
}

#[derive(Clone)]
pub struct ManageCronJobTool {
    cron: Arc<CronRuntime>,
}

impl ManageCronJobTool {
    pub const NAME: &'static str = "manage_cron_job";

    pub fn new(cron: Arc<CronRuntime>) -> Self {
        Self { cron }
    }
}

impl Tool<BaseCtx> for ManageCronJobTool {
    type Args = ManageCronJobArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Manages an existing cron job by action. ",
            "Supported actions are get, pause, resume, and remove."
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
                        "enum": ["get", "pause", "resume", "remove"],
                        "description": "The management action to perform on the cron job."
                    },
                    "id": {
                        "type": "integer",
                        "description": "The cron job id to manage."
                    }
                },
                "required": ["action", "id"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let result = match args.action {
            CronJobAction::Get => json!({
                "action": "get",
                "job": self.cron.store.get_job(args.id).await?,
            }),
            CronJobAction::Pause => json!({
                "action": "pause",
                "job": self.cron.store.pause_job(args.id).await?,
            }),
            CronJobAction::Resume => json!({
                "action": "resume",
                "job": self.cron.store.resume_job(args.id).await?,
            }),
            CronJobAction::Remove => {
                self.cron.store.remove_job(args.id).await?;
                json!({
                    "action": "remove",
                    "id": args.id,
                })
            }
        };

        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

#[derive(Clone)]
pub struct ListCronJobsTool {
    cron: Arc<CronRuntime>,
}

impl ListCronJobsTool {
    pub const NAME: &'static str = "list_cron_jobs";

    pub fn new(cron: Arc<CronRuntime>) -> Self {
        Self { cron }
    }
}

impl Tool<BaseCtx> for ListCronJobsTool {
    type Args = ListCronArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Lists scheduled cron jobs with optional cursor pagination. Returns up to 100 jobs per call."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cursor": {
                        "type": ["string", "null"],
                        "description": "Pagination cursor returned by a previous list_cron_jobs call."
                    },
                    "limit": {
                        "type": ["integer", "null"],
                        "description": "Maximum number of jobs to return. Defaults to 10 and is capped at 100."
                    }
                },
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let (jobs, next_cursor) = self.cron.store.list_jobs(args.cursor, args.limit).await?;
        Ok(paginated_response(jobs, next_cursor))
    }
}

#[derive(Clone)]
pub struct ListCronRunsTool {
    cron: Arc<CronRuntime>,
}

impl ListCronRunsTool {
    pub const NAME: &'static str = "list_cron_runs";

    pub fn new(cron: Arc<CronRuntime>) -> Self {
        Self { cron }
    }
}

impl Tool<BaseCtx> for ListCronRunsTool {
    type Args = ListCronArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists recent cron run history with optional cursor pagination. ",
            "When job_id is provided, only runs for that cron job are returned."
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
                    "job_id": {
                        "type": ["integer", "null"],
                        "description": "Optional cron job id. When present, only runs for that job are returned."
                    },
                    "cursor": {
                        "type": ["string", "null"],
                        "description": "Pagination cursor returned by a previous list_cron_runs call."
                    },
                    "limit": {
                        "type": ["integer", "null"],
                        "description": "Maximum number of runs to return. Defaults to 10 and is capped at 100."
                    }
                },
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let (runs, next_cursor) = self
            .cron
            .store
            .list_runs(args.cursor, args.limit, args.job_id)
            .await?;
        Ok(paginated_response(runs, next_cursor))
    }
}
