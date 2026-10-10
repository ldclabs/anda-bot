//! The MCP tools the model can call: `add_mcp_server`, `connect_mcp_server`
//! and `manage_mcp_server`. Each asks the user before it changes anything and
//! then goes through [`McpManager`], like the owner API does.

use crate::util::tool_response::ToolResponse as Response;
use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::{
    McpChange, McpManager, McpServerView,
    manager::SignIn,
    redact::{redact_args, redact_url},
    state::McpSource,
};
use crate::{
    config::{
        McpServerSettings, McpStdioSettings, McpStreamableHttpSettings, McpTransportSettings,
        normalize_string,
    },
    engine::{ActionDetail, McpApprovalKind, approval_detail, require_mcp_approval},
};

/// Builds the user-facing approval card for `add_mcp_server`. Secrets (env
/// values, bearer tokens, header values, credential-like argv, and URL
/// credentials/query values) are never included.
fn add_mcp_server_approval_card(
    server: &McpServerSettings,
    persist: bool,
) -> (String, Vec<ActionDetail>) {
    let mut details = vec![approval_detail("Server id", &server.id, "text")];
    let summary = match &server.transport {
        McpTransportSettings::Stdio(stdio) => {
            let safe_args = redact_args(&stdio.args);
            details.push(approval_detail("Command", &stdio.command, "text"));
            if !safe_args.is_empty() {
                details.push(approval_detail("Args", &safe_args, "list"));
            }
            if !stdio.env.is_empty() {
                let env_keys: Vec<&String> = stdio.env.keys().collect();
                details.push(approval_detail("Environment keys", env_keys, "list"));
            }
            if let Some(cwd) = &stdio.cwd {
                details.push(approval_detail("Working directory", cwd, "text"));
            }
            format!(
                "Run local MCP server: {} {}",
                stdio.command,
                safe_args.join(" ")
            )
            .trim_end()
            .to_string()
        }
        McpTransportSettings::StreamableHttp(http) => {
            let safe_url = redact_url(&http.url);
            details.push(approval_detail("URL", &safe_url, "text"));
            let header_names: Vec<&String> = http.headers.keys().collect();
            if !header_names.is_empty() {
                details.push(approval_detail("Header names", header_names, "list"));
            }
            format!("Connect MCP server: {safe_url}")
        }
    };
    details.push(approval_detail(
        "Persist to mcp.json",
        if persist { "yes" } else { "no" },
        "text",
    ));
    (summary, details)
}

#[derive(Clone)]
pub struct McpServerTool {
    manager: McpManager,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AddMcpServerArgs {
    pub id: String,
    #[serde(default, rename = "type")]
    pub r#type: Option<McpServerTransportType>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub persist: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum McpServerTransportType {
    #[serde(rename = "stdio")]
    Stdio,
    #[serde(rename = "http", alias = "streamable_http")]
    Http,
}

impl McpServerTool {
    pub const NAME: &'static str = "add_mcp_server";

    pub(crate) fn new(manager: McpManager) -> Self {
        Self { manager }
    }
}

fn server_settings(args: AddMcpServerArgs) -> Result<McpServerSettings, BoxError> {
    let AddMcpServerArgs {
        id,
        r#type,
        command,
        args,
        env,
        cwd,
        url,
        bearer_token,
        headers,
        enabled,
        include,
        exclude,
        persist: _,
    } = args;
    let id = normalize_string(&id).ok_or("MCP server id cannot be empty")?;
    let command_present = command.as_deref().and_then(normalize_string).is_some();
    let url_present = url.as_deref().and_then(normalize_string).is_some();
    let transport = match r#type {
        Some(McpServerTransportType::Stdio) => stdio_transport(command, args, env, cwd)?,
        Some(McpServerTransportType::Http) => http_transport(url, bearer_token, headers)?,
        None if command_present => stdio_transport(command, args, env, cwd)?,
        None if url_present => http_transport(url, bearer_token, headers)?,
        None => {
            return Err(
                "MCP server type is missing and transport cannot be inferred from command or url"
                    .into(),
            );
        }
    };

    let server = McpServerSettings {
        id,
        disabled: enabled == Some(false),
        transport,
        include: normalize_string_set(include),
        exclude: normalize_string_set(exclude),
        // Protocol-negotiation, startup and tasks tuning stay operator
        // knobs edited in mcp.json; a quick add takes the defaults.
        lifecycle: None,
        startup: None,
        tasks: None,
        // The agent cannot choose how its own calls are approved.
        approval: Default::default(),
        allow_external_users: false,
        timeouts: Default::default(),
        concurrency: None,
        limits: Default::default(),
        events: None,
        resources: None,
        elicitation: None,
    };
    let issues = server.setup_issues();
    if !issues.is_empty() {
        return Err(format!("invalid MCP server configuration: {}", issues.join("; ")).into());
    }
    Ok(server)
}

impl Tool<BaseCtx> for McpServerTool {
    type Args = AddMcpServerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Connects a new MCP server to the current Anda daemon and exposes its tools dynamically. ",
            "Use stdio for local child-process MCP servers and http for remote MCP endpoints. ",
            "Set persist=true only when the server should be written to mcp.json and survive daemon restart. ",
            "Stdio commands are spawned directly without a shell."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: add_mcp_server_parameters(),
            strict: Some(false),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let persist = args.persist;
        let server = server_settings(args)?;
        let enabled = !server.disabled;
        if !enabled && !persist {
            return Err("MCP server enabled=false is only useful with persist=true".into());
        }
        // Refuse a duplicate before connecting, not after.
        self.manager.check_new(&server).await?;

        // Stdio servers spawn arbitrary local processes and HTTP servers open
        // connections to arbitrary endpoints, so this always needs approval
        // outside FullAccess mode.
        let (summary, details) = add_mcp_server_approval_card(&server, persist);
        require_mcp_approval(
            &ctx,
            McpApprovalKind::Connect,
            Self::NAME,
            summary,
            details,
            json!({
                "server_id": &server.id,
                "persist": persist,
            }),
        )
        .await?;

        let server_id = server.id.clone();
        let persisted = self
            .manager
            .add_connected(server, persist, McpSource::Model)
            .await?;
        Ok(ToolOutput::new(Response::Ok {
            result: json!({
                "status": if enabled { "added" } else { "saved_disabled" },
                "server_id": server_id,
                "persisted": persisted,
                "enabled": enabled,
                "tools": self.manager.server_tools(&server_id),
            }),
            next_cursor: None,
        }))
    }
}

fn add_mcp_server_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "Stable server id used in local tool names. Example: filesystem, github, browser."
            },
            "type": {
                "type": "string",
                "enum": ["stdio", "http", "streamable_http"],
                "description": "Matches mcp.json server type. Use stdio for a local child process, or http for an HTTP MCP endpoint. Omit to infer from command or url; streamable_http is accepted for compatibility."
            },
            "command": {
                "type": "string",
                "description": "Executable for stdio transport. Required when type is stdio."
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Arguments for stdio transport."
            },
            "env": {
                "type": "object",
                "additionalProperties": { "type": "string" },
                "description": "Additional environment variables for stdio transport, matching mcp.json env object."
            },
            "cwd": {
                "type": "string",
                "description": "Optional working directory for stdio. Relative paths are rooted under ANDA_HOME. Omit to use the first Anda workspace."
            },
            "url": {
                "type": "string",
                "description": "MCP HTTP endpoint URL. Required when type is http."
            },
            "bearer_token": {
                "type": "string",
                "description": "Optional Streamable HTTP bearer token without the Bearer prefix. Prefer headers.Authorization for portable mcp.json-compatible config."
            },
            "headers": {
                "type": "object",
                "additionalProperties": { "type": "string" },
                "description": "Custom HTTP headers for HTTP transport, matching mcp.json headers object."
            },
            "enabled": {
                "type": "boolean",
                "description": "Matches mcp.json enabled. Omit or set true to connect now; set false only with persist=true to save a disabled entry."
            },
            "include": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional remote MCP tool allowlist. Omit to include all tools except excluded ones."
            },
            "exclude": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional remote MCP tool denylist."
            },
            "persist": {
                "type": "boolean",
                "description": "Tool-only option. Set true to also write this server to mcp.json so it survives daemon restart. Defaults to false."
            }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn normalize_string_set(values: Vec<String>) -> BTreeSet<String> {
    values
        .into_iter()
        .filter_map(|value| normalize_string(&value))
        .collect()
}

fn stdio_transport(
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<String>,
) -> Result<McpTransportSettings, BoxError> {
    let command = normalize_string(command.as_deref().unwrap_or_default())
        .ok_or("MCP stdio command cannot be empty")?;
    Ok(McpTransportSettings::Stdio(McpStdioSettings {
        command,
        args,
        env: normalize_string_map("env", env)?,
        cwd: cwd.and_then(|cwd| normalize_string(&cwd)),
        inherit_env: None,
    }))
}

fn http_transport(
    url: Option<String>,
    bearer_token: Option<String>,
    headers: BTreeMap<String, String>,
) -> Result<McpTransportSettings, BoxError> {
    let url = normalize_string(url.as_deref().unwrap_or_default())
        .ok_or("MCP HTTP URL cannot be empty")?;
    Ok(McpTransportSettings::StreamableHttp(
        McpStreamableHttpSettings {
            url,
            bearer_token: bearer_token.and_then(|token| normalize_string(&token)),
            headers: normalize_string_map("headers", headers)?,
            oauth: None,
        },
    ))
}

fn normalize_string_map(
    field: &str,
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, BoxError> {
    let mut map = BTreeMap::new();
    for (raw_key, value) in values {
        let key = normalize_string(&raw_key)
            .ok_or_else(|| format!("MCP server {field} entries cannot have an empty key"))?;
        if map.insert(key.clone(), value).is_some() {
            return Err(format!("MCP server {field} contains duplicate key {key}").into());
        }
    }
    Ok(map)
}

/// Connects an MCP server by URL, transparently running the OAuth flow when the
/// endpoint requires it.
///
/// Authorization uses a native-app loopback redirect (RFC 8252), served by the
/// gateway at a fixed path rather than by a port bound per attempt — see
/// [`super::oauth`] for why that distinction decides whether a remote user
/// can authorize at all. When the browser opens here, the call waits and
/// returns a connected server; when it cannot, the call returns the
/// authorization URL so the agent can hand it to the user, and the gateway
/// finishes the flow whenever they get to it.
///
/// A successful OAuth connection outlives the daemon: the tokens go to the
/// provider's credential store and the server (with an `oauth` marker, never
/// tokens) is persisted to mcp.json, so restarts reconnect silently from the
/// stored refresh token. When those credentials die (revoked in the remote
/// console, store deleted), calling this tool again on the same server runs a
/// fresh browser authorization instead of failing with "already exists".
#[derive(Clone)]
pub struct McpConnectTool {
    manager: McpManager,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConnectMcpServerArgs {
    /// MCP endpoint URL (http or https), e.g. `https://api.al.ink/mcp`.
    pub url: String,
    /// Optional stable server id used in local tool names. Defaults to the host.
    #[serde(default)]
    pub id: Option<String>,
    /// Optional OAuth scopes to request. Defaults to the scopes the server
    /// advertises during discovery.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Re-run the browser authorization even though the stored credentials are
    /// still valid, discarding them first.
    #[serde(default)]
    pub reauthorize: bool,
    /// Full redirect URL copied from the browser's address bar, for finishing an
    /// authorization whose redirect could not reach this daemon.
    #[serde(default)]
    pub redirect_url: Option<String>,
}

impl McpConnectTool {
    pub const NAME: &'static str = "connect_mcp_server";

    pub(crate) fn new(manager: McpManager) -> Self {
        Self { manager }
    }
}

impl Tool<BaseCtx> for McpConnectTool {
    type Args = ConnectMcpServerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Connects an MCP server by URL and exposes its tools dynamically. ",
            "If the server requires OAuth authorization, this opens the user's browser so they ",
            "can approve access, then finishes connecting automatically; the connection is ",
            "persisted (tokens in the local credential store, server in mcp.json) and survives ",
            "daemon restarts. Calling it again on a connected server verifies the connection, ",
            "and re-runs the browser authorization if the credentials have died. ",
            "Pass reauthorize=true to sign out and re-consent while the stored credentials are ",
            "still valid, which is what changing the granted scopes requires. ",
            "When no browser can be opened here, the call returns status=authorization_required ",
            "with an authorization_url to give the user; the connection then completes by itself. ",
            "Before calling, tell the user a browser window may open for them to confirm ",
            "authorization. Use add_mcp_server instead for local stdio servers or ",
            "bearer-token/no-auth HTTP servers that should be persisted."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: connect_mcp_server_parameters(),
            strict: Some(false),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let url = args.url.trim().to_string();
        let safe_url = redact_url(&url);
        let mut details = vec![approval_detail("URL", &safe_url, "text")];
        if let Some(id) = args.id.as_deref() {
            details.push(approval_detail("Server id", id, "text"));
        }
        if !args.scopes.is_empty() {
            details.push(approval_detail("OAuth scopes", &args.scopes, "list"));
        }
        // Re-authorizing discards a working grant, so the card must say so
        // rather than reading like an ordinary connect.
        let title = if args.reauthorize {
            details.push(approval_detail(
                "Re-authorize",
                "discards the stored credentials and asks for consent again",
                "text",
            ));
            format!("Re-authorize MCP server: {safe_url}")
        } else {
            format!("Connect MCP server: {safe_url}")
        };
        require_mcp_approval(
            &ctx,
            McpApprovalKind::Connect,
            Self::NAME,
            title,
            details,
            json!({ "url": &safe_url }),
        )
        .await?;

        let redirect_url = args.redirect_url.as_deref().and_then(normalize_string);
        if url.is_empty() && redirect_url.is_none() {
            return Err("MCP server url cannot be empty".into());
        }
        let result = self
            .manager
            .sign_in(
                SignIn {
                    url: Some(url),
                    id: args.id,
                    scopes: args.scopes,
                    reauthorize: args.reauthorize,
                    redirect_url,
                },
                true,
            )
            .await?;
        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

fn connect_mcp_server_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "MCP endpoint URL. Must be http or https. Example: https://api.al.ink/mcp"
            },
            "id": {
                "type": "string",
                "description": "Optional stable server id used in local tool names. Defaults to the URL host."
            },
            "scopes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional OAuth scopes to request. Omit to request the scopes the server advertises."
            },
            "reauthorize": {
                "type": "boolean",
                "description": "Set true to sign out of an already-connected server and run the browser authorization again, for example to grant different scopes. Omit for a normal connect."
            },
            "redirect_url": {
                "type": "string",
                "description": "Full URL from the browser address bar after the user approved access, used only when the redirect could not reach this daemon. Everything else is ignored when this is set."
            }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

/// Lets the model see the configured MCP servers and look after them:
/// list and inspect them, reconnect one that failed, and, with the user's
/// approval, enable, disable, remove or sign out of one.
#[derive(Clone)]
pub struct ManageMcpServerTool {
    manager: McpManager,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManageMcpAction {
    List,
    Status,
    Reconnect,
    Enable,
    Disable,
    Remove,
    SignOut,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ManageMcpServerArgs {
    pub action: ManageMcpAction,
    #[serde(default)]
    pub server_id: Option<String>,
}

impl ManageMcpServerTool {
    pub const NAME: &'static str = "manage_mcp_server";

    pub(crate) fn new(manager: McpManager) -> Self {
        Self { manager }
    }

    async fn approve(
        &self,
        ctx: &BaseCtx,
        action: ManageMcpAction,
        id: &str,
    ) -> Result<(), BoxError> {
        let server = self.manager.server(id).await?.server;
        let (verb, consequence) = match action {
            ManageMcpAction::Enable => ("Enable", "Its tools become available to the agent."),
            ManageMcpAction::Disable => (
                "Disable",
                "Its tools stop being available; the entry stays in mcp.json.",
            ),
            ManageMcpAction::Remove => (
                "Remove",
                "Deletes its mcp.json entry and any stored sign-in.",
            ),
            ManageMcpAction::SignOut => (
                "Sign out of",
                "Deletes its stored sign-in; using it again needs a new authorization.",
            ),
            ManageMcpAction::List | ManageMcpAction::Status | ManageMcpAction::Reconnect => {
                return Ok(());
            }
        };
        let details = vec![
            approval_detail("Server id", id, "text"),
            approval_detail("Server", &server.summary, "text"),
            approval_detail("Effect", consequence, "text"),
        ];
        require_mcp_approval(
            ctx,
            McpApprovalKind::Change,
            Self::NAME,
            format!("{verb} MCP server {id}"),
            details,
            json!({ "server_id": id, "action": action }),
        )
        .await
    }
}

impl Tool<BaseCtx> for ManageMcpServerTool {
    type Args = ManageMcpServerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists and looks after the MCP servers this Anda daemon is configured with. ",
            "list shows every server with its status, tool count and last error, including servers ",
            "that failed, need sign-in or were skipped for a configuration problem. status shows one ",
            "server and its tools. reconnect retries one server, or every failed one when server_id ",
            "is null. enable, disable, remove and sign_out change the user's configuration and ask ",
            "for their approval first. Use add_mcp_server or connect_mcp_server to add a server."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: manage_mcp_server_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let action = args.action;
        let id = args.server_id.as_deref().and_then(normalize_string);
        let required_id = || {
            id.clone().ok_or_else(|| {
                BoxError::from(format!(
                    "server_id is required for {}",
                    json!(action).as_str().unwrap_or_default()
                ))
            })
        };
        let result = match action {
            ManageMcpAction::List => {
                let snapshot = self.manager.snapshot().await;
                json!({
                    "config_changed_on_disk": snapshot.config_changed_on_disk,
                    "diagnostics": snapshot.diagnostics,
                    "servers": snapshot.servers.iter().map(model_view).collect::<Vec<_>>(),
                })
            }
            ManageMcpAction::Status => {
                let detail = self.manager.server(&required_id()?).await?;
                let mut view = model_view(&detail.server);
                view["tools"] = detail
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "remote_name": tool.remote_name,
                            "hidden": tool.hidden,
                        })
                    })
                    .collect();
                view
            }
            ManageMcpAction::Reconnect => json!(self.manager.reconnect(id.as_deref()).await?),
            ManageMcpAction::Enable | ManageMcpAction::Disable => {
                let id = required_id()?;
                self.approve(&ctx, action, &id).await?;
                let change = McpChange::SetEnabled {
                    id,
                    enabled: action == ManageMcpAction::Enable,
                };
                json!(self.manager.apply(change, None, McpSource::Model).await?)
            }
            ManageMcpAction::Remove => {
                let id = required_id()?;
                self.approve(&ctx, action, &id).await?;
                let change = McpChange::Remove {
                    id,
                    keep_credentials: false,
                };
                json!(self.manager.apply(change, None, McpSource::Model).await?)
            }
            ManageMcpAction::SignOut => {
                let id = required_id()?;
                self.approve(&ctx, action, &id).await?;
                self.manager.sign_out(&id).await?;
                json!({ "signed_out": id })
            }
        };
        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

/// A server as the model sees it: its state, without the configuration.
fn model_view(server: &McpServerView) -> Value {
    let mut view = json!(server);
    if let Some(object) = view.as_object_mut() {
        object.remove("settings");
        object.remove("source");
    }
    view
}

fn manage_mcp_server_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["list", "status", "reconnect", "enable", "disable", "remove", "sign_out"],
                "description": "list and status read; reconnect retries; enable, disable, remove and sign_out ask the user first."
            },
            "server_id": {
                "type": ["string", "null"],
                "description": "The server's id. Required for every action except list, and reconnect retries every failed server when it is null."
            }
        },
        "required": ["action", "server_id"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::McpSettings, util::json_schema::assert_openai_strict_parameters};
    use anda_core::StateFeatures;
    use std::sync::Arc;

    /// A mock ctx whose ActionSession auto-approves every approval card, so
    /// tool-level tests can exercise the behavior behind the approval gate.
    fn auto_approving_ctx() -> BaseCtx {
        use crate::engine::{
            ActionEvent, ActionResponseArgs, ActionRuntime, ActionSession, action_id_from_message,
        };

        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let caller = ctx.caller().to_text();
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, mut event_rx) = tokio::sync::mpsc::channel(4);
        let session = ActionSession::new(
            runtime.clone(),
            event_sender,
            caller.clone(),
            "session_test".to_string(),
            Arc::new(std::sync::atomic::AtomicU64::new(1)),
            Arc::new(anda_engine::model::Models::default()),
            std::env::temp_dir(),
        );
        ctx.set_state(session);
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let ActionEvent::Add(message) = event
                    && let Some(action_id) = action_id_from_message(&message)
                {
                    let _ = runtime
                        .respond(
                            &caller,
                            0,
                            ActionResponseArgs {
                                action_id,
                                approve: Some(true),
                                choice_id: None,
                                choice_text: None,
                                remember: None,
                            },
                        )
                        .await;
                }
            }
        });
        ctx
    }

    fn plain_ctx() -> BaseCtx {
        anda_engine::engine::EngineBuilder::new().mock_ctx().base
    }

    fn stdio_args(id: &str, enabled: Option<bool>, persist: bool) -> AddMcpServerArgs {
        AddMcpServerArgs {
            id: id.to_string(),
            r#type: Some(McpServerTransportType::Stdio),
            command: Some("missing-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            bearer_token: None,
            headers: BTreeMap::new(),
            enabled,
            include: Vec::new(),
            exclude: Vec::new(),
            persist,
        }
    }

    #[tokio::test]
    async fn tool_schemas_keep_their_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let manager = McpManager::for_test(dir.path()).await;

        let definition = McpServerTool::new(manager.clone()).definition();
        assert_eq!(definition.strict, Some(false));
        let properties = definition
            .parameters
            .get("properties")
            .and_then(Value::as_object)
            .unwrap();
        assert!(properties.get("transport_type").is_none());
        assert!(properties.get("type").is_some());
        assert_eq!(properties["env"]["type"], "object");
        assert_eq!(properties["env"]["additionalProperties"]["type"], "string");
        assert_eq!(properties["headers"]["type"], "object");
        assert_eq!(
            properties["headers"]["additionalProperties"]["type"],
            "string"
        );
        assert_eq!(properties["enabled"]["type"], "boolean");

        let definition = ManageMcpServerTool::new(manager).definition();
        assert_eq!(definition.strict, Some(true));
        assert_openai_strict_parameters(&definition.parameters);
        let args: ManageMcpServerArgs =
            serde_json::from_value(json!({"action": "sign_out", "server_id": null})).unwrap();
        assert_eq!(args.action, ManageMcpAction::SignOut);
    }

    #[test]
    fn mcp_approval_card_redacts_argv_and_url_credentials() {
        let stdio = McpServerSettings {
            id: "secret-server".to_string(),
            transport: McpTransportSettings::Stdio(McpStdioSettings {
                command: "mcp-server".to_string(),
                args: vec![
                    "--api-key".to_string(),
                    "api-secret-value".to_string(),
                    "--password=hunter2".to_string(),
                    // `Url::parse` accepts this as scheme `x-api-key`; it must
                    // still be redacted as a credential-bearing argument.
                    "x-api-key:header-secret-value".to_string(),
                    // Connection strings carry credentials in their authority.
                    "postgresql://pg-user:pg-secret-value@localhost/db".to_string(),
                    "redis://:redis-secret-value@cache:6379".to_string(),
                    "https://alice:url-password-value@example.com/mcp?token=url-secret&mode=fast"
                        .to_string(),
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (summary, details) = add_mcp_server_approval_card(&stdio, false);
        let rendered = format!("{summary} {}", serde_json::to_string(&details).unwrap());
        for secret in [
            "api-secret-value",
            "hunter2",
            "header-secret-value",
            "pg-user",
            "pg-secret-value",
            "redis-secret-value",
            "alice",
            "url-password-value",
            "url-secret",
            "fast",
        ] {
            assert!(!rendered.contains(secret), "leaked {secret}: {rendered}");
        }
        assert!(rendered.contains("redacted"));

        let http = McpServerSettings {
            id: "remote".to_string(),
            transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                url: "https://bob:http-password-value@example.com/mcp?access_token=top-secret"
                    .to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (summary, details) = add_mcp_server_approval_card(&http, true);
        let rendered = format!("{summary} {}", serde_json::to_string(&details).unwrap());
        for secret in ["bob", "http-password-value", "top-secret"] {
            assert!(!rendered.contains(secret), "leaked {secret}: {rendered}");
        }
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn add_mcp_server_args_convert_to_settings() {
        let server = server_settings(AddMcpServerArgs {
            id: " filesystem ".to_string(),
            command: Some(" npx ".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
            ],
            env: BTreeMap::from([(" TOKEN ".to_string(), "secret".to_string())]),
            cwd: Some(" workspace ".to_string()),
            include: vec![" read_file ".to_string(), " ".to_string()],
            exclude: vec!["write_file".to_string()],
            ..stdio_args("x", None, false)
        })
        .unwrap();
        assert_eq!(server.id, "filesystem");
        assert_eq!(server.include, BTreeSet::from(["read_file".to_string()]));
        assert_eq!(server.exclude, BTreeSet::from(["write_file".to_string()]));
        let McpTransportSettings::Stdio(stdio) = server.transport else {
            panic!("expected stdio")
        };
        assert_eq!(stdio.command, "npx");
        assert_eq!(stdio.cwd.as_deref(), Some("workspace"));
        assert_eq!(stdio.env.get("TOKEN").map(String::as_str), Some("secret"));

        let server = server_settings(AddMcpServerArgs {
            r#type: Some(McpServerTransportType::Http),
            command: None,
            url: Some(" https://mcp.example.test/mcp ".to_string()),
            bearer_token: Some(" token ".to_string()),
            headers: BTreeMap::from([("x-client".to_string(), "anda".to_string())]),
            ..stdio_args("remote", Some(true), true)
        })
        .unwrap();
        let McpTransportSettings::StreamableHttp(http) = server.transport else {
            panic!("expected HTTP")
        };
        assert_eq!(http.url, "https://mcp.example.test/mcp");
        assert_eq!(http.bearer_token.as_deref(), Some("token"));
        assert_eq!(
            http.headers.get("x-client").map(String::as_str),
            Some("anda")
        );

        // The transport is inferred, and enabled=false is kept.
        let server = server_settings(AddMcpServerArgs {
            r#type: None,
            command: None,
            url: Some("https://mcp.example.test/mcp".to_string()),
            ..stdio_args("remote", Some(false), true)
        })
        .unwrap();
        assert!(server.disabled);
        assert!(matches!(
            server.transport,
            McpTransportSettings::StreamableHttp(_)
        ));
    }

    #[tokio::test]
    async fn changes_need_approval_outside_full_access() {
        // A plain mock ctx runs in OnRisk mode with no ActionSession, so every
        // tool that changes servers must fail closed instead of executing.
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(
            McpSettings::file_path(dir.path()),
            r#"{"mcpServers":{"docs":{"url":"http://127.0.0.1:9/mcp"}}}"#,
        )
        .await
        .unwrap();
        let manager = McpManager::for_test(dir.path()).await;

        let err = Tool::call(
            &McpServerTool::new(manager.clone()),
            plain_ctx(),
            stdio_args("srv", None, false),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("approval"), "{err}");

        let err = Tool::call(
            &McpConnectTool::new(manager.clone()),
            plain_ctx(),
            ConnectMcpServerArgs {
                url: "http://127.0.0.1:9/mcp".to_string(),
                id: None,
                scopes: Vec::new(),
                reauthorize: false,
                redirect_url: None,
            },
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("approval"), "{err}");

        let manage = ManageMcpServerTool::new(manager.clone());
        for action in [
            ManageMcpAction::Disable,
            ManageMcpAction::Remove,
            ManageMcpAction::SignOut,
        ] {
            let err = Tool::call(
                &manage,
                plain_ctx(),
                ManageMcpServerArgs {
                    action,
                    server_id: Some("docs".to_string()),
                },
                Vec::new(),
            )
            .await
            .unwrap_err();
            assert!(
                err.to_string()
                    .contains("changing an MCP server requires user approval"),
                "{err}"
            );
        }
        // Nothing changed.
        let content = tokio::fs::read_to_string(McpSettings::file_path(dir.path()))
            .await
            .unwrap();
        assert!(content.contains("docs") && !content.contains("enabled"));
    }

    #[tokio::test]
    async fn manage_reads_without_approval_and_changes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = McpSettings::file_path(dir.path());
        tokio::fs::write(
            &path,
            r#"{"mcpServers":{"docs":{"url":"http://127.0.0.1:9/mcp","headers":{"Authorization":"Bearer header-secret"}},"broken":{"command":"x","lifecycle":"handshake"}}}"#,
        )
        .await
        .unwrap();
        let manager = McpManager::for_test(dir.path()).await;
        let tool = ManageMcpServerTool::new(manager);
        let call = |action, server_id: Option<&str>, ctx| {
            let tool = tool.clone();
            let server_id = server_id.map(str::to_string);
            async move {
                match Tool::call(
                    &tool,
                    ctx,
                    ManageMcpServerArgs { action, server_id },
                    Vec::new(),
                )
                .await?
                .output
                {
                    Response::Ok { result, .. } => Ok::<Value, BoxError>(result),
                    other => panic!("unexpected {other:?}"),
                }
            }
        };

        let list = call(ManageMcpAction::List, None, plain_ctx())
            .await
            .unwrap();
        let rendered = list.to_string();
        assert!(!rendered.contains("header-secret"), "{rendered}");
        let servers = list["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 2);
        assert!(
            servers
                .iter()
                .all(|server| server.get("settings").is_none())
        );
        let broken = servers.iter().find(|s| s["id"] == "broken").unwrap();
        assert_eq!(broken["status"], "invalid");
        assert!(
            broken["diagnostics"][0]
                .as_str()
                .unwrap()
                .contains("lifecycle")
        );

        let status = call(ManageMcpAction::Status, Some("docs"), plain_ctx())
            .await
            .unwrap();
        assert_eq!(status["id"], "docs");
        assert_eq!(status["auth"], "bearer");
        let err = call(ManageMcpAction::Status, None, plain_ctx())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("server_id is required"), "{err}");

        let disabled = call(ManageMcpAction::Disable, Some("docs"), auto_approving_ctx())
            .await
            .unwrap();
        assert_eq!(disabled["removed"], json!(["docs"]));
        let json: Value =
            serde_json::from_str(&tokio::fs::read_to_string(&path).await.unwrap()).unwrap();
        assert_eq!(json["mcpServers"]["docs"]["enabled"], false);

        call(
            ManageMcpAction::Remove,
            Some("broken"),
            auto_approving_ctx(),
        )
        .await
        .unwrap();
        let json: Value =
            serde_json::from_str(&tokio::fs::read_to_string(&path).await.unwrap()).unwrap();
        assert!(json["mcpServers"].get("broken").is_none());

        let err = call(ManageMcpAction::SignOut, Some("docs"), auto_approving_ctx())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("does not sign in with OAuth"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn add_persists_a_disabled_server_without_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let manager = McpManager::for_test(dir.path()).await;
        let tool = McpServerTool::new(manager.clone());

        let output = Tool::call(
            &tool,
            auto_approving_ctx(),
            stdio_args("disabled", Some(false), true),
            Vec::new(),
        )
        .await
        .unwrap();
        let Response::Ok { result, .. } = output.output else {
            panic!("expected ok response")
        };
        assert_eq!(result["status"], "saved_disabled");
        assert_eq!(result["enabled"], false);
        assert_eq!(result["persisted"], true);
        assert!(manager.provider().routes().is_empty());
        assert!(!manager.provider().contains_server("disabled"));

        let content = tokio::fs::read_to_string(McpSettings::file_path(dir.path()))
            .await
            .unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["mcpServers"]["disabled"]["enabled"], false);
        assert_eq!(json["mcpServers"]["disabled"]["command"], "missing-command");

        let err = Tool::call(
            &tool,
            auto_approving_ctx(),
            stdio_args("x", Some(false), false),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("only useful with persist=true"));
    }

    #[tokio::test]
    async fn add_refuses_taken_ids_before_connecting() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(
            McpSettings::file_path(dir.path()),
            r#"{"mcpServers":{"saved":{"type":"stdio","command":"x","enabled":false}}}"#,
        )
        .await
        .unwrap();
        let manager = McpManager::for_test(dir.path()).await;
        manager
            .provider()
            .register_server(anda_engine::extension::mcp::McpServerConfig::stdio(
                "running",
                "missing-command",
            ))
            .unwrap();
        let tool = McpServerTool::new(manager.clone());

        // Refused ahead of the approval gate: the plain ctx would fail it.
        let err = Tool::call(
            &tool,
            plain_ctx(),
            stdio_args("running", None, false),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "MCP server running already exists");

        let err = Tool::call(
            &tool,
            plain_ctx(),
            stdio_args("saved", None, true),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "MCP server saved already exists in mcp.json"
        );
        assert!(!manager.provider().contains_server("saved"));
    }
}
