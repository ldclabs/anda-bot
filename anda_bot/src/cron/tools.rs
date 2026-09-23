use crate::util::tool_response::ToolResponse as Response;
use anda_core::{
    BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolGroupInfo, ToolOutput,
};
use anda_engine::context::BaseCtx;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(test)]
use std::sync::Arc;

use super::{
    store::CronStore,
    types::{
        CreateCronJobArgs, CronJobOrigin, UpdateCronJobArgs,
        deserialize_optional_u64_from_number_or_string,
        deserialize_optional_usize_from_number_or_string, deserialize_u64_from_number_or_string,
    },
};
use crate::engine::{CliWorkspaceGrants, SessionRequestMeta};
use crate::util::request_meta::{keys, request_meta_extra_as};

fn trusted_meta(ctx: &BaseCtx) -> Result<anda_core::RequestMeta, BoxError> {
    let meta = ctx
        .get_state::<SessionRequestMeta>()
        .map(|state| state.get())
        .unwrap_or_else(|| ctx.meta().clone());
    if request_meta_extra_as::<bool>(&meta, keys::EXTERNAL_USER).unwrap_or(false) {
        return Err("Cron tools are unavailable to external IM users".into());
    }
    Ok(meta)
}

async fn job_origin(
    ctx: &BaseCtx,
    meta: &anda_core::RequestMeta,
    grants: Option<&CliWorkspaceGrants>,
) -> Result<Option<CronJobOrigin>, BoxError> {
    let mut origin = CronJobOrigin::from_meta_with_caller(meta, ctx.caller());
    if let Some(grants) = grants
        && let Some(path) = grants.authorize_cron_workspace(ctx.caller(), meta).await?
        && let Some(origin) = &mut origin
    {
        origin.workspace_grant = Some(path.to_string_lossy().into_owned());
    }
    Ok(origin)
}

/// Stable id of the cron scheduler capability group.
pub const CRON_TOOL_GROUP_ID: &str = "cron_scheduler";

/// Returns the shared [`ToolGroupInfo`] for the cron scheduler tools.
///
/// Every `create_cron_job` / `update_cron_job` / `manage_cron_job` /
/// `list_cron_jobs` / `list_cron_runs` tool reports this so the registry
/// presents them as one bundle. The registry fills in the member list from the
/// tools actually registered.
pub fn cron_tool_group_info() -> ToolGroupInfo {
    ToolGroupInfo {
        id: CRON_TOOL_GROUP_ID.to_string(),
        title: "Cron scheduler".to_string(),
        description: "Schedule, inspect, and manage recurring or one-off background jobs that run shell commands or agent prompts.".to_string(),
        instructions: Some(
            "These tools share one cron job store. Typical flow: use `create_cron_job` to schedule a shell command or agent prompt (with a cron expression, RFC3339 timestamp, or interval/once duration), `list_cron_jobs` to discover existing jobs and their ids, and `list_cron_runs` to review run history. Use `update_cron_job` to change a job's schedule or payload in place, and `manage_cron_job` to get, pause, resume, or remove a job by id.".to_string(),
        ),
    }
}

#[derive(Clone)]
pub struct CreateCronTool {
    store: CronStore,
    workspace_grants: Option<CliWorkspaceGrants>,
}

impl CreateCronTool {
    pub const NAME: &'static str = "create_cron_job";

    pub fn new(store: CronStore) -> Self {
        Self {
            store,
            workspace_grants: None,
        }
    }
    pub(crate) fn with_workspace_grants(mut self, grants: CliWorkspaceGrants) -> Self {
        self.workspace_grants = Some(grants);
        self
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
            "Relevant tools: update_cron_job, manage_cron_job, list_cron_jobs, list_cron_runs, shell, tools_select."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: create_cron_job_parameters(),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(cron_tool_group_info())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let meta = trusted_meta(&ctx)?;
        let origin = job_origin(&ctx, &meta, self.workspace_grants.as_ref()).await?;
        let job = self.store.insert_job(args, origin).await?;
        Ok(ToolOutput::new(Response::Ok {
            result: job.to_view(),
            next_cursor: None,
        }))
    }
}

#[derive(Clone)]
pub struct UpdateCronJobTool {
    store: CronStore,
    workspace_grants: Option<CliWorkspaceGrants>,
}

impl UpdateCronJobTool {
    pub const NAME: &'static str = "update_cron_job";

    pub fn new(store: CronStore) -> Self {
        Self {
            store,
            workspace_grants: None,
        }
    }
    pub(crate) fn with_workspace_grants(mut self, grants: CliWorkspaceGrants) -> Self {
        self.workspace_grants = Some(grants);
        self
    }
}

impl Tool<BaseCtx> for UpdateCronJobTool {
    type Args = UpdateCronJobArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Updates an existing cron job without changing its run history. ",
            "Pass null for fields that should stay unchanged. ",
            "Pass origin=true to replace the job origin with the current caller and request context. ",
            "Schedule edits preserve paused state; use manage_cron_job resume to reactivate. ",
            "Use an empty string for name or tz to clear that field."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: update_cron_job_parameters(),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(cron_tool_group_info())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let meta = trusted_meta(&ctx)?;
        let origin = if args.origin.unwrap_or(false) {
            job_origin(&ctx, &meta, self.workspace_grants.as_ref()).await?
        } else {
            None
        };
        let job = self.store.update_job_with_origin(args, origin).await?;
        Ok(ToolOutput::new(Response::Ok {
            result: job.to_view(),
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
    #[serde(deserialize_with = "deserialize_u64_from_number_or_string")]
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ListCronArgs {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u64_from_number_or_string"
    )]
    pub job_id: Option<u64>,
    pub cursor: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_usize_from_number_or_string"
    )]
    pub limit: Option<usize>,
}

fn create_cron_job_parameters() -> Value {
    json!({
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
        "required": ["job_kind", "job", "schedule_kind", "schedule", "name", "tz"],
        "additionalProperties": false
    })
}

fn update_cron_job_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": ["integer", "string"],
                "description": "The cron job id to update. Numeric strings are accepted."
            },
            "job_kind": {
                "type": ["string", "null"],
                "enum": ["shell", "agent", null],
                "description": "New job kind, or null to leave unchanged."
            },
            "job": {
                "type": ["string", "null"],
                "description": "New shell command or agent prompt, or null to leave unchanged."
            },
            "schedule_kind": {
                "type": ["string", "null"],
                "enum": ["cron", "at", "every", "once", null],
                "description": "New schedule kind, or null to leave unchanged."
            },
            "schedule": {
                "type": ["string", "null"],
                "description": "New schedule value, or null to leave unchanged. For 'cron', provide a cron expression. For 'at', provide an RFC3339 timestamp. For 'every' and 'once', provide a duration using optional s/m/h/d units."
            },
            "name": {
                "type": ["string", "null"],
                "description": "New human-readable name. Null leaves unchanged; an empty string clears the name."
            },
            "tz": {
                "type": ["string", "null"],
                "description": "New IANA timezone name for cron schedules. Null leaves unchanged; an empty string clears the timezone."
            },
            "origin": {
                "type": ["boolean", "null"],
                "description": "Set true to replace the saved origin with the current caller and request metadata; null or false leaves origin unchanged."
            }
        },
        "required": ["id", "job_kind", "job", "schedule_kind", "schedule", "name", "tz", "origin"],
        "additionalProperties": false
    })
}

fn manage_cron_job_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["get", "pause", "resume", "remove"],
                "description": "The management action to perform on the cron job."
            },
            "id": {
                "type": ["integer", "string"],
                "description": "The cron job id to manage. Numeric strings are accepted."
            }
        },
        "required": ["action", "id"],
        "additionalProperties": false
    })
}

fn list_cron_jobs_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cursor": {
                "type": ["string", "null"],
                "description": "Pagination cursor returned by a previous list_cron_jobs call."
            },
            "limit": {
                "type": ["integer", "string", "null"],
                "description": "Maximum number of jobs to return. Numeric strings are accepted. Defaults to 10 and is capped at 100."
            }
        },
        "required": ["cursor", "limit"],
        "additionalProperties": false
    })
}

fn list_cron_runs_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "job_id": {
                "type": ["integer", "string", "null"],
                "description": "Optional cron job id. Numeric strings are accepted. When present, only runs for that job are returned."
            },
            "cursor": {
                "type": ["string", "null"],
                "description": "Pagination cursor returned by a previous list_cron_runs call."
            },
            "limit": {
                "type": ["integer", "string", "null"],
                "description": "Maximum number of runs to return. Numeric strings are accepted. Defaults to 10 and is capped at 100."
            }
        },
        "required": ["job_id", "cursor", "limit"],
        "additionalProperties": false
    })
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
    store: CronStore,
}

impl ManageCronJobTool {
    pub const NAME: &'static str = "manage_cron_job";

    pub fn new(store: CronStore) -> Self {
        Self { store }
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
            parameters: manage_cron_job_parameters(),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(cron_tool_group_info())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        trusted_meta(&ctx)?;
        let result = match args.action {
            CronJobAction::Get => json!({
                "action": "get",
                "job": self.store.get_job(args.id).await?.to_view(),
            }),
            CronJobAction::Pause => json!({
                "action": "pause",
                "job": self.store.pause_job(args.id).await?.to_view(),
            }),
            CronJobAction::Resume => json!({
                "action": "resume",
                "job": self.store.resume_job(args.id).await?.to_view(),
            }),
            CronJobAction::Remove => {
                self.store.remove_job(args.id).await?;
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
    store: CronStore,
}

impl ListCronJobsTool {
    pub const NAME: &'static str = "list_cron_jobs";

    pub fn new(store: CronStore) -> Self {
        Self { store }
    }
}

impl Tool<BaseCtx> for ListCronJobsTool {
    type Args = ListCronArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists scheduled cron jobs with optional cursor pagination. Returns up to 100 jobs per call. ",
            "Paused jobs report paused=true and omit next_run. Lists omit last_result and truncate job/last_error to 512 characters; use manage_cron_job get for full details."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: list_cron_jobs_parameters(),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(cron_tool_group_info())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        trusted_meta(&ctx)?;
        let (jobs, next_cursor) = self.store.list_jobs(args.cursor, args.limit).await?;
        let jobs: Vec<serde_json::Value> = jobs.iter().map(|job| job.to_summary()).collect();
        Ok(ToolOutput::new(Response::Ok {
            result: Value::Array(jobs),
            next_cursor,
        }))
    }
}

#[derive(Clone)]
pub struct ListCronRunsTool {
    store: CronStore,
}

impl ListCronRunsTool {
    pub const NAME: &'static str = "list_cron_runs";

    pub fn new(store: CronStore) -> Self {
        Self { store }
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
            parameters: list_cron_runs_parameters(),
            strict: Some(true),
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        Some(cron_tool_group_info())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        trusted_meta(&ctx)?;
        let (runs, next_cursor) = self
            .store
            .list_runs(args.cursor, args.limit, args.job_id)
            .await?;
        Ok(paginated_response(runs, next_cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::CronRuntime;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use serde_json::json;

    #[test]
    fn cron_tool_schemas_are_openai_strict() {
        for parameters in [
            create_cron_job_parameters(),
            update_cron_job_parameters(),
            manage_cron_job_parameters(),
            list_cron_jobs_parameters(),
            list_cron_runs_parameters(),
        ] {
            assert_openai_strict_parameters(&parameters);
        }
    }

    #[test]
    fn cron_tool_args_accept_numeric_strings() {
        let manage: ManageCronJobArgs = serde_json::from_value(json!({
            "action": "get",
            "id": "5"
        }))
        .unwrap();
        assert_eq!(manage.id, 5);

        let list: ListCronArgs = serde_json::from_value(json!({
            "job_id": "5",
            "cursor": null,
            "limit": "25"
        }))
        .unwrap();
        assert_eq!(list.job_id, Some(5));
        assert_eq!(list.limit, Some(25));

        let update: UpdateCronJobArgs = serde_json::from_value(json!({
            "id": "5",
            "job_kind": null,
            "job": "echo updated",
            "schedule_kind": null,
            "schedule": null,
            "name": null,
            "tz": null,
            "origin": null
        }))
        .unwrap();
        assert_eq!(update.id, 5);
        assert_eq!(update.job, Some("echo updated".to_string()));
        assert_eq!(update.origin, None);
    }

    use super::super::types::{JobKind, ScheduleKind};
    use anda_engine::engine::{EngineBuilder, EngineRef};

    async fn test_cron_runtime() -> Arc<CronRuntime> {
        let db = crate::test_support::memory_db("cron_tools").await;
        Arc::new(
            CronRuntime::connect(Arc::new(EngineRef::new()), db)
                .await
                .unwrap(),
        )
    }

    fn create_args(name: &str) -> CreateCronJobArgs {
        CreateCronJobArgs {
            job_kind: JobKind::Shell,
            job: "echo hello".to_string(),
            schedule_kind: ScheduleKind::Every,
            schedule: "60".to_string(),
            name: Some(name.to_string()),
            tz: None,
        }
    }

    fn result_of(output: ToolOutput<Response>) -> Value {
        match output.output {
            Response::Ok { result, .. } => result,
            other => panic!("expected ok response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_update_and_manage_cron_jobs_through_tools() {
        let cron = test_cron_runtime().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        // Create a job; the origin is derived from the calling context.
        let created = result_of(
            CreateCronTool::new(cron.store.clone())
                .call(ctx.clone(), create_args("daily"), Vec::new())
                .await
                .unwrap(),
        );
        let job_id = created["_id"].as_u64().expect("job id");
        assert_eq!(created["name"], "daily");
        assert_eq!(created["schedule_kind"], "every");

        // Update the schedule and replace the origin with the current caller.
        let update_args: UpdateCronJobArgs = serde_json::from_value(json!({
            "id": job_id,
            "schedule_kind": "every",
            "schedule": "5m",
            "origin": true,
        }))
        .unwrap();
        let updated = result_of(
            UpdateCronJobTool::new(cron.store.clone())
                .call(ctx.clone(), update_args, Vec::new())
                .await
                .unwrap(),
        );
        assert_eq!(updated["schedule"], "5m");

        // Update without origin replacement keeps the previous origin.
        let update_args: UpdateCronJobArgs = serde_json::from_value(json!({
            "id": job_id,
            "name": "renamed",
        }))
        .unwrap();
        let updated = result_of(
            UpdateCronJobTool::new(cron.store.clone())
                .call(ctx.clone(), update_args, Vec::new())
                .await
                .unwrap(),
        );
        assert_eq!(updated["name"], "renamed");

        let manage = ManageCronJobTool::new(cron.store.clone());
        let got = result_of(
            manage
                .call(
                    ctx.clone(),
                    ManageCronJobArgs {
                        action: CronJobAction::Get,
                        id: job_id,
                    },
                    Vec::new(),
                )
                .await
                .unwrap(),
        );
        assert_eq!(got["action"], "get");
        assert_eq!(got["job"]["_id"], job_id);

        let paused = result_of(
            manage
                .call(
                    ctx.clone(),
                    ManageCronJobArgs {
                        action: CronJobAction::Pause,
                        id: job_id,
                    },
                    Vec::new(),
                )
                .await
                .unwrap(),
        );
        assert_eq!(paused["action"], "pause");
        // Paused jobs surface `paused` instead of the storage sentinel.
        assert_eq!(paused["job"]["paused"], true);
        assert!(paused["job"].get("next_run").is_none());

        let resumed = result_of(
            manage
                .call(
                    ctx.clone(),
                    ManageCronJobArgs {
                        action: CronJobAction::Resume,
                        id: job_id,
                    },
                    Vec::new(),
                )
                .await
                .unwrap(),
        );
        assert_eq!(resumed["action"], "resume");
        assert_eq!(resumed["job"]["paused"], false);
        assert!(resumed["job"]["next_run"].as_u64().is_some());

        let removed = result_of(
            manage
                .call(
                    ctx.clone(),
                    ManageCronJobArgs {
                        action: CronJobAction::Remove,
                        id: job_id,
                    },
                    Vec::new(),
                )
                .await
                .unwrap(),
        );
        assert_eq!(removed["action"], "remove");
        assert_eq!(removed["id"], job_id);

        // The removed job is gone.
        assert!(
            manage
                .call(
                    ctx,
                    ManageCronJobArgs {
                        action: CronJobAction::Get,
                        id: job_id,
                    },
                    Vec::new(),
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn list_tools_paginate_jobs_and_runs() {
        let cron = test_cron_runtime().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        for name in ["a", "b", "c"] {
            CreateCronTool::new(cron.store.clone())
                .call(ctx.clone(), create_args(name), Vec::new())
                .await
                .unwrap();
        }

        let listed = ListCronJobsTool::new(cron.store.clone())
            .call(
                ctx.clone(),
                ListCronArgs {
                    job_id: None,
                    cursor: None,
                    limit: Some(2),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match listed.output {
            Response::Ok {
                result,
                next_cursor,
            } => {
                assert_eq!(result.as_array().map(Vec::len), Some(2));
                assert!(next_cursor.is_some());
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let runs = ListCronRunsTool::new(cron.store.clone())
            .call(ctx, ListCronArgs::default(), Vec::new())
            .await
            .unwrap();
        match runs.output {
            Response::Ok { result, .. } => {
                assert_eq!(result.as_array().map(Vec::len), Some(0));
            }
            other => panic!("expected ok response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cron_tool_metadata_exposes_names_and_strict_schemas() {
        let cron = test_cron_runtime().await;
        assert_eq!(
            CreateCronTool::new(cron.store.clone()).name(),
            "create_cron_job"
        );
        assert_eq!(
            UpdateCronJobTool::new(cron.store.clone()).name(),
            "update_cron_job"
        );
        assert_eq!(
            ManageCronJobTool::new(cron.store.clone()).name(),
            "manage_cron_job"
        );
        assert_eq!(
            ListCronJobsTool::new(cron.store.clone()).name(),
            "list_cron_jobs"
        );
        assert_eq!(
            ListCronRunsTool::new(cron.store.clone()).name(),
            "list_cron_runs"
        );

        for definition in [
            CreateCronTool::new(cron.store.clone()).definition(),
            UpdateCronJobTool::new(cron.store.clone()).definition(),
            ManageCronJobTool::new(cron.store.clone()).definition(),
            ListCronJobsTool::new(cron.store.clone()).definition(),
            ListCronRunsTool::new(cron.store.clone()).definition(),
        ] {
            assert_eq!(definition.strict, Some(true));
            assert!(!definition.description.is_empty());
        }
    }

    #[tokio::test]
    async fn cron_tools_form_one_capability_group() {
        use anda_core::ToolSet;

        let cron = test_cron_runtime().await;
        let mut tools = ToolSet::<BaseCtx>::new();
        tools
            .add(Arc::new(CreateCronTool::new(cron.store.clone())))
            .unwrap();
        tools
            .add(Arc::new(UpdateCronJobTool::new(cron.store.clone())))
            .unwrap();
        tools
            .add(Arc::new(ManageCronJobTool::new(cron.store.clone())))
            .unwrap();
        tools
            .add(Arc::new(ListCronJobsTool::new(cron.store.clone())))
            .unwrap();
        tools
            .add(Arc::new(ListCronRunsTool::new(cron.store.clone())))
            .unwrap();

        let groups = tools.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, CRON_TOOL_GROUP_ID);
        // All five registered tools land in the group, sorted by name.
        assert_eq!(
            groups[0].members,
            vec![
                "create_cron_job".to_string(),
                "list_cron_jobs".to_string(),
                "list_cron_runs".to_string(),
                "manage_cron_job".to_string(),
                "update_cron_job".to_string(),
            ]
        );
        assert!(groups[0].instructions.is_some());
    }

    #[tokio::test]
    async fn external_live_context_cannot_access_any_cron_tool() {
        let cron = test_cron_runtime().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let job = cron
            .store
            .insert_job(create_args("owner"), None)
            .await
            .unwrap();
        let mut meta = ctx.meta().clone();
        meta.extra.insert(keys::EXTERNAL_USER.into(), true.into());
        ctx.set_state(SessionRequestMeta::new(meta));
        assert!(
            CreateCronTool::new(cron.store.clone())
                .call(ctx.clone(), create_args("external"), vec![])
                .await
                .is_err()
        );
        let update = serde_json::from_value(json!({"id":job._id,"job":"replace"})).unwrap();
        assert!(
            UpdateCronJobTool::new(cron.store.clone())
                .call(ctx.clone(), update, vec![])
                .await
                .is_err()
        );
        for action in [
            CronJobAction::Get,
            CronJobAction::Pause,
            CronJobAction::Resume,
            CronJobAction::Remove,
        ] {
            assert!(
                ManageCronJobTool::new(cron.store.clone())
                    .call(
                        ctx.clone(),
                        ManageCronJobArgs {
                            action,
                            id: job._id
                        },
                        vec![]
                    )
                    .await
                    .is_err()
            );
        }
        assert!(
            ListCronJobsTool::new(cron.store.clone())
                .call(ctx.clone(), ListCronArgs::default(), vec![])
                .await
                .is_err()
        );
        assert!(
            ListCronRunsTool::new(cron.store.clone())
                .call(ctx.clone(), ListCronArgs::default(), vec![])
                .await
                .is_err()
        );
        assert_eq!(cron.store.get_job(job._id).await.unwrap().job, job.job);
        // The same authenticated channel Principal can still serve a trusted request.
        ctx.set_state(SessionRequestMeta::new(ctx.meta().clone()));
        assert!(
            ListCronJobsTool::new(cron.store.clone())
                .call(ctx, ListCronArgs::default(), vec![])
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn list_previews_keep_full_details_available() {
        let cron = test_cron_runtime().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let mut args = create_args("large");
        args.job = "测".repeat(1000);
        let job = cron.store.insert_job(args, None).await.unwrap();
        let list = result_of(
            ListCronJobsTool::new(cron.store.clone())
                .call(ctx.clone(), ListCronArgs::default(), vec![])
                .await
                .unwrap(),
        );
        assert_eq!(list[0]["job"].as_str().unwrap().chars().count(), 513);
        assert!(list[0].get("last_result").is_none());
        let detail = result_of(
            ManageCronJobTool::new(cron.store.clone())
                .call(
                    ctx,
                    ManageCronJobArgs {
                        action: CronJobAction::Get,
                        id: job._id,
                    },
                    vec![],
                )
                .await
                .unwrap(),
        );
        assert_eq!(detail["job"]["job"].as_str().unwrap().chars().count(), 1000);
    }

    #[tokio::test]
    async fn creation_persists_only_an_authorized_cli_workspace() {
        let cron = test_cron_runtime().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        let grants = CliWorkspaceGrants::new(*ctx.caller());
        let mut meta = ctx.meta().clone();
        meta.extra.insert(
            keys::SOURCE.into(),
            format!("cli:{}", path.display()).into(),
        );
        meta.extra.insert(
            keys::WORKSPACE.into(),
            path.to_string_lossy().to_string().into(),
        );
        ctx.set_state(SessionRequestMeta::new(meta));
        let tool = CreateCronTool::new(cron.store.clone()).with_workspace_grants(grants.clone());
        assert!(
            tool.call(ctx.clone(), create_args("cli"), vec![])
                .await
                .is_err()
        );
        grants.register(&path).await.unwrap();
        let created = result_of(tool.call(ctx, create_args("cli"), vec![]).await.unwrap());
        let job = cron
            .store
            .get_job(created["_id"].as_u64().unwrap())
            .await
            .unwrap();
        assert_eq!(
            job.origin.unwrap().workspace_grant.as_deref(),
            path.to_str()
        );
    }
}
