//! Python composition and an owner/workspace-scoped library of named tools.
mod library;
mod run;
mod sandbox;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows_profile;
pub(crate) mod worker;

use super::{
    action::live_request_meta,
    agent::memory_policy::MemoryPolicy,
    shell_runtime::{CliWorkspaceGrants, call_workspace},
};
use crate::util::request_meta::{keys, request_meta_extra_as};
use anda_core::{
    BoxError, BoxFut, FunctionDefinition, Principal, RequestMeta, Resource, StateFeatures, Tool,
    ToolGroupInfo, ToolInput, ToolOutput, ToolProvider,
};
use anda_engine::context::{AgentCtx, BaseCtx, RequestCatalog, RequestToolCatalog};
use library::{Declaration, Library};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, Semaphore};

pub(crate) const EXEC: &str = "code_exec";
pub(crate) const REGISTER: &str = "code_register";
pub(crate) const LIST: &str = "code_list";
pub(crate) use run::{CallSite, FinishedRuns, RecoveryHold};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Failure {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}
impl Failure {
    fn new(code: &str, msg: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: msg.into(),
            data: Value::Null,
        }
    }
    fn io(e: impl std::fmt::Display) -> Self {
        Self::new("storage_error", e.to_string())
    }
    fn output(self) -> ToolOutput<Value> {
        ToolOutput {
            output: json!({"error":self}),
            is_error: Some(true),
            ..Default::default()
        }
    }
}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::io(e)
    }
}
impl From<serde_json::Error> for Failure {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_arguments", e.to_string())
    }
}

#[derive(Clone)]
pub(crate) struct CodeMode {
    home: PathBuf,
    grants: CliWorkspaceGrants,
    workspace: PathBuf,
    owner: Principal,
    digest_key: [u8; 48],
    libraries: Arc<Mutex<HashMap<String, Arc<Library>>>>,
    slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct Binding {
    route: Value,
    library: Arc<Library>,
}

fn route(meta: &RequestMeta) -> Value {
    json!([
        request_meta_extra_as::<String>(meta, keys::SOURCE),
        request_meta_extra_as::<String>(meta, keys::REPLY_TARGET),
        request_meta_extra_as::<String>(meta, keys::THREAD),
        request_meta_extra_as::<String>(meta, keys::WORKSPACE),
        request_meta_extra_as::<bool>(meta, keys::EXTERNAL_USER).unwrap_or(false)
    ])
}

impl CodeMode {
    pub(crate) fn new(
        home: PathBuf,
        workspace: PathBuf,
        owner: Principal,
        grants: CliWorkspaceGrants,
        digest_key: [u8; 48],
    ) -> Self {
        Self {
            home: home.join("code_mode"),
            workspace,
            owner,
            grants,
            digest_key,
            libraries: Default::default(),
            slots: Arc::new(Semaphore::new(2)),
        }
    }

    pub(crate) async fn attach(&self, ctx: &AgentCtx, meta: &RequestMeta) {
        ctx.base
            .set_state(RequestToolCatalog(Arc::new(self.clone())));
        let binding = if *ctx.caller() == self.owner
            && !request_meta_extra_as::<bool>(meta, keys::EXTERNAL_USER).unwrap_or(false)
        {
            match self.open_library(&ctx.base, meta).await {
                Ok(library) => Some(Binding {
                    library,
                    route: route(meta),
                }),
                Err(err) => {
                    log::warn!("Code Mode script library unavailable: {err}");
                    None
                }
            }
        } else {
            None
        };
        ctx.base.set_state(binding);
    }

    async fn open_library(
        &self,
        ctx: &BaseCtx,
        meta: &RequestMeta,
    ) -> Result<Arc<Library>, BoxError> {
        let base = ctx.with_caller(*ctx.caller());
        let request_meta = super::SessionRequestMeta::new(meta.clone());
        request_meta.set_cron_workspace(ctx.get_state::<crate::cron::CronWorkspaceGrant>());
        base.set_state(request_meta);
        let workspace = match call_workspace(Some(&self.grants), &base).await? {
            Some(path) => path,
            None => {
                let workspace = request_meta_extra_as::<PathBuf>(meta, keys::WORKSPACE)
                    .filter(|p| !p.as_os_str().is_empty());
                tokio::fs::canonicalize(workspace.as_ref().unwrap_or(&self.workspace)).await?
            }
        };
        let path = workspace
            .to_str()
            .ok_or("Code Mode requires a Unicode workspace path")?;
        let key = super::app_protocol::hash(&serde_json::to_vec(&(self.owner.to_text(), path))?);
        let mut libraries = self.libraries.lock().await;
        let library = if let Some(library) = libraries.get(&key) {
            library.clone()
        } else {
            let library = Arc::new(
                Library::open(self.home.join("v1").join(&key))
                    .await
                    .map_err(|e| e.message)?,
            );
            libraries.insert(key, library.clone());
            library
        };
        Ok(library)
    }

    fn library(&self, ctx: &BaseCtx) -> Result<Arc<Library>, Failure> {
        let binding = ctx.get_state::<Option<Binding>>().flatten().filter(|b| {
            *ctx.caller() == self.owner
                && !request_meta_extra_as::<bool>(ctx.meta(), keys::EXTERNAL_USER).unwrap_or(false)
                && b.route == route(&live_request_meta(ctx))
        });
        binding.map(|b| b.library).ok_or_else(|| {
            Failure::new(
                "scope_denied",
                "private script library is unavailable for this request",
            )
        })
    }

    pub(crate) fn tool(&self, name: &'static str) -> CodeTool {
        CodeTool {
            mode: self.clone(),
            name,
        }
    }

    pub(crate) fn initial_interfaces(
        &self,
        ctx: &BaseCtx,
        mut request: anda_core::CompletionRequest,
    ) -> anda_core::CompletionRequest {
        let mut hints = String::new();
        for def in &request.tools {
            if let Some(hint) = self.interface(ctx, def)
                && hints.len() + hint.len() < 4096
            {
                hints.push('\n');
                hints.push_str(&hint);
            }
        }
        if !hints.is_empty() {
            request
                .instructions
                .push_str("\n\n## Python tool interfaces");
            request.instructions.push_str(&hints);
        }
        request
    }
}

impl RequestCatalog for CodeMode {
    fn provider(&self, ctx: &BaseCtx) -> Option<Arc<dyn ToolProvider<BaseCtx>>> {
        self.library(ctx).ok().map(|library| {
            Arc::new(Scripts {
                mode: self.clone(),
                library,
            }) as _
        })
    }
    fn visible(&self, ctx: &BaseCtx, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        if name == "actions_api"
            || name == super::AndaBot::NAME
            || !MemoryPolicy::current(ctx).allows_tool(&name)
        {
            return false;
        }
        if name == LIST || name == REGISTER {
            return self.library(ctx).is_ok();
        }
        if name.starts_with("script__") {
            return self.library(ctx).is_ok_and(|s| s.get(&name).is_some());
        }
        true
    }
    fn executable(&self, ctx: &BaseCtx, name: &str) -> bool {
        !ctx.get_state::<RecoveryHold>()
            .is_some_and(|hold| hold.waiting())
            && self.visible(ctx, name)
    }
    fn revision(&self, ctx: &BaseCtx, name: &str) -> Option<String> {
        if !name.starts_with("script__") {
            return None;
        }
        self.library(ctx)
            .ok()?
            .get(name)
            .map(|s| s.revision.clone())
    }
    fn interface(&self, ctx: &BaseCtx, def: &FunctionDefinition) -> Option<String> {
        if matches!(def.name.as_str(), EXEC | "tools_select" | "tools_search") {
            return None;
        }
        let output = match def.name.as_str() {
            "list_cron_runs" | "list_cron_jobs" => {
                "dict: page['result'] is a list; use page.get('next_cursor')"
            }
            "read_file" => "dict: out['content'] is the file content",
            "search_file" => "dict: out['paths'] lists matched paths",
            REGISTER => "dict with result.name, revision and status",
            LIST => "dict with result (list) and optional next_cursor",
            name if name.starts_with("script__") => {
                return self
                    .library(ctx)
                    .ok()?
                    .get(name)
                    .map(|s| format!("tools.{name}{}", s.declaration.signature));
            }
            name if name.starts_with("mcp_") => {
                "MCP: out['structured_content'], out['text'], out['resources']"
            }
            _ => "bounded public JSON; inspect the returned fields",
        };
        let params = def.parameters["properties"]
            .as_object()
            .map(|v| {
                v.keys()
                    .map(|key| format!("{key}=…"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        if def.name.contains('-') {
            Some(format!("tools[{:?}]({params}) -> {output}", def.name))
        } else {
            Some(format!("tools.{}({params}) -> {output}", def.name))
        }
    }
}

pub(crate) struct CodeTool {
    mode: CodeMode,
    name: &'static str,
}
fn schema(props: Value) -> Value {
    let required: Vec<_> = props.as_object().unwrap().keys().cloned().collect();
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
impl Tool<BaseCtx> for CodeTool {
    type Args = Value;
    type Output = Value;
    fn name(&self) -> String {
        self.name.into()
    }
    fn description(&self) -> String {
        let description = match self.name {
            EXEC => concat!(
                "Run a Python function body in a fresh sandbox. `return` a value or end with an ",
                "expression; it comes back as JSON. Use one script instead of several direct calls ",
                "when you do not need to read the intermediate results: paging with cursors, details ",
                "for many IDs, joins, filtering, counting, sorting, or one action for many targets. ",
                "Return only what the answer needs. Call tools by keyword: tools.name(**kwargs) or ",
                "tools['name-with-dash'](**kwargs). A call returns the tool's JSON; a failure raises ",
                "ToolError (.code, .message, .data). For independent calls use ",
                "parallel([(name, kwargs), ...]): items keep their order, each a value or a ToolError, ",
                "and their approvals are asked together. Imports: json, itertools, operator, ",
                "functools (reduce, partial, cmp_to_key) and collections (Counter, defaultdict, deque, ",
                "OrderedDict); no files, network, re, math or datetime. Limits: 5 s CPU, 128 tool ",
                "calls, 32 KiB result, 4 KiB print output. Call a single tool directly, use shell for ",
                "local files and programs, and keep original text when the answer depends on reading it."
            ),
            REGISTER => concat!(
                "Save or remove a Python tool for this owner and workspace. Save one annotated ",
                "def with a docstring, name=null and expected_revision=null on creation. To update, ",
                "pass its current revision. To remove, supply its exact script__ name and revision, source=null."
            ),
            _ => concat!(
                "List saved Python functions. Use an exact published name, no cursor and ",
                "include_source=true to read source. Load a saved function with tools_select for direct calls."
            ),
        };
        description.into()
    }
    fn definition(&self) -> FunctionDefinition {
        let nullable = json!({"type":["string","null"]});
        let props = match self.name {
            EXEC => json!({"code":{"type":"string"}}),
            REGISTER => {
                json!({"action":{"type":"string","enum":["save","remove"]},"name":nullable,"source":nullable,"expected_revision":nullable})
            }
            _ => {
                json!({"query":nullable,"cursor":nullable,"limit":{"type":["integer","null"],"minimum":1,"maximum":50},"include_source":{"type":"boolean"}})
            }
        };
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: schema(props),
            strict: Some(true),
        }
    }
    fn group(&self) -> Option<ToolGroupInfo> {
        Some(ToolGroupInfo {
            id: "code_mode".into(),
            title: "Python Code Mode".into(),
            description: "Run, save and list Python tool workflows.".into(),
            instructions: None,
        })
    }
    async fn call(
        &self,
        ctx: BaseCtx,
        args: Value,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Value>, BoxError> {
        let result = self.execute(&ctx, args).await;
        Ok(result.unwrap_or_else(Failure::output))
    }
}
impl CodeTool {
    async fn approve_change(
        &self,
        ctx: &BaseCtx,
        name: &str,
        action: &str,
        revision: Option<&str>,
        source: Option<&str>,
    ) -> Result<(), Failure> {
        super::action::approve_script_change(ctx, name, action, revision, source)
            .await
            .map_err(|err| {
                let message = err.to_string();
                Failure::new(
                    if message.starts_with("approval_required") {
                        "approval_required"
                    } else {
                        "approval_denied"
                    },
                    message,
                )
            })?;
        if !self.mode.visible(ctx, REGISTER) {
            return Err(Failure::new(
                "scope_denied",
                "script registration scope changed",
            ));
        }
        Ok(())
    }

    async fn execute(&self, ctx: &BaseCtx, args: Value) -> Result<ToolOutput<Value>, Failure> {
        if !self.mode.visible(ctx, self.name) {
            return Err(Failure::new(
                "scope_denied",
                "tool unavailable under current policy",
            ));
        }
        match self.name {
            EXEC => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    code: String,
                }
                let input: Input = serde_json::from_value(args)?;
                self.mode
                    .run(ctx, json!({"op":"exec","source":input.code}))
                    .await
            }
            REGISTER => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    action: String,
                    name: Option<String>,
                    source: Option<String>,
                    expected_revision: Option<String>,
                }
                let input: Input = serde_json::from_value(args)?;
                let library = self.mode.library(ctx)?;
                let out = match (input.action.as_str(), input.name, input.source) {
                    ("save", None, Some(source)) => {
                        let source = source.replace("\r\n", "\n").replace('\r', "\n");
                        let inspected = self
                            .mode
                            .run(ctx, json!({"op":"inspect","source":source}))
                            .await?;
                        if inspected.is_error == Some(true) {
                            return Ok(inspected);
                        }
                        let decl: Declaration =
                            serde_json::from_value(inspected.output["result"].clone())?;
                        if !self.mode.visible(ctx, REGISTER) {
                            return Err(Failure::new(
                                "scope_denied",
                                "script registration scope changed",
                            ));
                        }
                        let expected = input.expected_revision.as_deref();
                        if let Some(answer) = library.check_save(&decl.name, &source, expected)? {
                            return Ok(ToolOutput::new(answer));
                        }
                        self.approve_change(ctx, &decl.name, "save", expected, Some(&source))
                            .await?;
                        // After admission, cancellation must not separate a
                        // successful rename from publishing its in-memory entry.
                        tokio::spawn(async move {
                            library
                                .save(source, decl, input.expected_revision.as_deref())
                                .await
                        })
                        .await
                        .map_err(Failure::io)??
                    }
                    ("remove", Some(name), None) => {
                        let revision = input
                            .expected_revision
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                Failure::new("invalid_arguments", "removal requires a revision")
                            })?;
                        if let Some(answer) = library.check_remove(&name, &revision)? {
                            return Ok(ToolOutput::new(answer));
                        }
                        self.approve_change(ctx, &name, "remove", Some(&revision), None)
                            .await?;
                        tokio::spawn(async move { library.remove(&name, &revision).await })
                            .await
                            .map_err(Failure::io)??
                    }
                    _ => {
                        return Err(Failure::new(
                            "invalid_arguments",
                            "save requires source only; remove requires name and revision",
                        ));
                    }
                };
                Ok(ToolOutput::new(out))
            }
            _ => Ok(ToolOutput::new(
                self.mode
                    .library(ctx)?
                    .list(serde_json::from_value(args)?)?,
            )),
        }
    }
}

struct Scripts {
    mode: CodeMode,
    library: Arc<Library>,
}
impl ToolProvider<BaseCtx> for Scripts {
    fn name(&self) -> String {
        "python_scripts".into()
    }
    fn definitions(&self, names: Option<&[String]>) -> Vec<FunctionDefinition> {
        self.library.definitions(names)
    }
    fn contains_lowercase(&self, name: &str) -> bool {
        self.library.get(name).is_some()
    }
    fn call(
        &self,
        ctx: BaseCtx,
        input: ToolInput<Value>,
    ) -> BoxFut<'_, Result<ToolOutput<Value>, BoxError>> {
        Box::pin(async move {
            let result = async {
                let library = self.mode.library(&ctx)?;
                if !Arc::ptr_eq(&library, &self.library) {
                    return Err(Failure::new("scope_denied", "workspace changed"));
                }
                let script = library
                    .get(&input.name.to_ascii_lowercase())
                    .ok_or_else(|| Failure::new("not_found", "script is no longer registered"))?;
                let python = ctx.get_state::<CallSite>().is_some_and(|site| site.is_direct(&ctx));
                let args = script.arguments(input.args, python)?;
                let request = json!({"op":"invoke","source":script.source,"name":script.declaration.name,"args":args,"revision":script.revision});
                let mut output = self.mode.run(&ctx, request).await?;
                if output.is_error != Some(true) {
                    output.output = json!({"result":output.output["result"]["value"]});
                }
                Ok(output)
            }
            .await;
            Ok(result.unwrap_or_else(Failure::output))
        })
    }
}
