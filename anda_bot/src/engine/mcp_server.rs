use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::{
    context::BaseCtx,
    extension::mcp::{
        McpAuthorizationRequired, McpOAuthConfig, McpOAuthMetadata, McpServerConfig,
        McpToolProvider, McpTransportConfig, OAuthAuthorizationCodeConfig,
    },
};
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::timeout,
};

use crate::{
    config::{
        McpOAuthSettings, McpServerSettings, McpSettings, McpStdioSettings,
        McpStreamableHttpSettings, McpTransportSettings, normalize_string,
    },
    util::text::read_text_file,
};

use super::{
    approval_detail, backup_daemon_config, daemon_config_needs_backup, require_mcp_approval,
    write_daemon_config_atomically,
};

const APPROVAL_REDACTED: &str = "[redacted]";

fn approval_arg_name_is_sensitive(name: &str) -> bool {
    let name = name
        .trim_start_matches(['-', '/'])
        .to_ascii_lowercase()
        .replace('_', "-");
    matches!(name.as_str(), "h" | "header" | "headers" | "key")
        || [
            "token",
            "secret",
            "password",
            "passwd",
            "credential",
            "api-key",
            "apikey",
            "private-key",
            "authorization",
            "bearer",
        ]
        .iter()
        .any(|marker| name.contains(marker))
        || name.ends_with("-key")
}

fn redact_url_for_approval(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return "[invalid URL omitted]".to_string();
    };

    let had_password = url.password().is_some();
    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }
    if had_password {
        let _ = url.set_password(Some("redacted"));
    }

    let query_keys: Vec<String> = url.query_pairs().map(|(key, _)| key.into_owned()).collect();
    if !query_keys.is_empty() {
        url.set_query(None);
        let mut query = url.query_pairs_mut();
        for key in query_keys {
            query.append_pair(&key, APPROVAL_REDACTED);
        }
    }
    url.set_fragment(None);
    url.to_string()
}

fn redact_args_for_approval(args: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            redacted.push(APPROVAL_REDACTED.to_string());
            redact_next = false;
            continue;
        }

        // Only treat network URLs as URLs. `Url::parse` also accepts any
        // `scheme:rest` token, and `redact_url_for_approval` has nothing to
        // strip from those — so `x-api-key:sk-live-...` would be echoed
        // verbatim instead of falling through to the checks below.
        if let Ok(parsed) = reqwest::Url::parse(arg)
            && matches!(parsed.scheme(), "http" | "https" | "ws" | "wss")
        {
            redacted.push(redact_url_for_approval(arg));
            continue;
        }
        // `name=value` and `name:value` both carry secrets in practice
        // (`--password=x`, `X-Api-Key:x`).
        if let Some((name, _value)) = arg.split_once(['=', ':'])
            && approval_arg_name_is_sensitive(name)
        {
            let separator = if arg[name.len()..].starts_with(':') {
                ':'
            } else {
                '='
            };
            redacted.push(format!("{name}{separator}{APPROVAL_REDACTED}"));
            continue;
        }
        if (arg.starts_with('-') || arg.starts_with('/')) && approval_arg_name_is_sensitive(arg) {
            redacted.push(arg.clone());
            redact_next = true;
            continue;
        }

        let lower = arg.to_ascii_lowercase();
        if lower.contains("authorization:") || lower.contains("bearer ") {
            redacted.push(APPROVAL_REDACTED.to_string());
        } else {
            redacted.push(arg.clone());
        }
    }
    redacted
}

/// Builds the user-facing approval card for `add_mcp_server`. Secrets (env
/// values, bearer tokens, header values, credential-like argv, and URL
/// credentials/query values) are never included.
fn add_mcp_server_approval_card(server: &McpServerSettings, persist: bool) -> (String, Vec<Value>) {
    let mut details = vec![approval_detail("Server id", &server.id, "text")];
    let summary = match &server.transport {
        McpTransportSettings::Stdio(stdio) => {
            let safe_args = redact_args_for_approval(&stdio.args);
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
            let safe_url = redact_url_for_approval(&http.url);
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
    provider: Arc<McpToolProvider>,
    home_dir: PathBuf,
    default_cwd: Option<PathBuf>,
    config_path: PathBuf,
    config_write_lock: Arc<Mutex<()>>,
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

    pub fn new(
        provider: Arc<McpToolProvider>,
        home_dir: PathBuf,
        default_cwd: Option<PathBuf>,
        config_path: PathBuf,
        config_write_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            provider,
            home_dir,
            default_cwd,
            config_path,
            config_write_lock,
        }
    }

    fn server_settings(&self, args: AddMcpServerArgs) -> Result<McpServerSettings, BoxError> {
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
        };
        let issues = McpSettings {
            servers: vec![server.clone()],
        }
        .setup_issues();
        if !issues.is_empty() {
            return Err(format!("invalid MCP server configuration: {}", issues.join(", ")).into());
        }
        Ok(server)
    }

    async fn persist_server(&self, server: McpServerSettings) -> Result<(), BoxError> {
        let _guard = self.config_write_lock.lock().await;
        persist_mcp_server_config(&self.config_path, server).await
    }
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
        let server = self.server_settings(args)?;
        let enabled = !server.disabled;
        if !enabled && !persist {
            return Err("MCP server enabled=false is only useful with persist=true".into());
        }
        if enabled && self.provider.contains_server(&server.id) {
            return Err(format!("MCP server {} already exists", server.id).into());
        }

        // Stdio servers spawn arbitrary local processes and HTTP servers open
        // connections to arbitrary endpoints, so this always needs approval
        // outside FullAccess mode.
        let (summary, details) = add_mcp_server_approval_card(&server, persist);
        require_mcp_approval(
            &ctx,
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
        if enabled {
            let server_configs = McpSettings {
                servers: vec![server.clone()],
            }
            .server_configs(&self.home_dir, self.default_cwd.as_deref())?;
            let server_config = server_configs
                .into_iter()
                .next()
                .ok_or("MCP server configuration was unexpectedly empty")?;
            self.provider.add_server(server_config).await?;
        }

        let mut persisted = false;
        if persist {
            if let Err(err) = self.persist_server(server.clone()).await {
                return Err(format!(
                    "MCP server {server_id} was added for the current daemon, but failed to persist to {}: {err}",
                    self.config_path.display()
                )
                .into());
            }
            persisted = true;
        }

        let tools = self
            .provider
            .routes()
            .into_iter()
            .filter(|route| route.server_id == server_id)
            .map(|route| {
                json!({
                    "name": route.name,
                    "remote_name": route.remote_name,
                    "server_id": route.server_id,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput::new(Response::Ok {
            result: json!({
                "status": if enabled { "added" } else { "saved_disabled" },
                "server_id": server_id,
                "persisted": persisted,
                "enabled": enabled,
                "tools": tools,
            }),
            next_cursor: None,
        }))
    }
}

fn add_mcp_server_parameters() -> serde_json::Value {
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

async fn persist_mcp_server_config(
    config_path: &Path,
    server: McpServerSettings,
) -> Result<(), BoxError> {
    let content = match read_text_file(config_path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(err) => return Err(err.into()),
    };

    let settings = McpSettings::from_json_contents(&content)?;
    let issues = settings.setup_issues();
    if !issues.is_empty() {
        return Err(format!("invalid mcp.json: {}", issues.join(", ")).into());
    }
    if settings
        .servers
        .iter()
        .any(|existing| existing.id.trim() == server.id)
    {
        return Err(format!("MCP server {} already exists in mcp.json", server.id).into());
    }

    let content = append_mcp_server_to_config_content(&content, &server)?;

    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if daemon_config_needs_backup(config_path, content.as_bytes()).await? {
        backup_daemon_config(config_path).await?;
    }
    write_daemon_config_atomically(config_path, content.as_bytes()).await
}

fn append_mcp_server_to_config_content(
    content: &str,
    server: &McpServerSettings,
) -> Result<String, BoxError> {
    let mut root = if content.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(content)?
    };
    let object = root
        .as_object_mut()
        .ok_or("mcp.json root must be an object")?;

    let root_key = if object.contains_key("mcpServers") {
        "mcpServers"
    } else if object.contains_key("servers") {
        "servers"
    } else {
        object.insert("mcpServers".to_string(), json!({}));
        "mcpServers"
    };

    let servers = object
        .get_mut(root_key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("mcp.json {root_key} must be an object to persist a server"))?;
    if servers.contains_key(&server.id) {
        return Err(format!("MCP server {} already exists in mcp.json", server.id).into());
    }
    servers.insert(server.id.clone(), mcp_server_json(server));

    let mut content = serde_json::to_string_pretty(&root)?;
    content.push('\n');
    Ok(content)
}

fn mcp_server_json(server: &McpServerSettings) -> Value {
    let mut object = Map::new();
    match &server.transport {
        McpTransportSettings::Stdio(stdio) => {
            object.insert("type".to_string(), json!("stdio"));
            object.insert("command".to_string(), json!(stdio.command));
            if !stdio.args.is_empty() {
                object.insert("args".to_string(), json!(stdio.args));
            }
            if !stdio.env.is_empty() {
                object.insert("env".to_string(), string_map_json(&stdio.env));
            }
            if let Some(cwd) = &stdio.cwd {
                object.insert("cwd".to_string(), json!(cwd));
            }
        }
        McpTransportSettings::StreamableHttp(http) => {
            object.insert("type".to_string(), json!("http"));
            object.insert("url".to_string(), json!(http.url));
            let mut headers = http.headers.clone();
            if let Some(token) = &http.bearer_token {
                let has_authorization = headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization"));
                if !has_authorization {
                    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
                }
            }
            if !headers.is_empty() {
                object.insert("headers".to_string(), string_map_json(&headers));
            }
            if let Some(oauth) = &http.oauth {
                object.insert("oauth".to_string(), json!(oauth));
            }
        }
    }

    if server.disabled {
        object.insert("enabled".to_string(), json!(false));
    }
    if !server.include.is_empty() {
        object.insert("include".to_string(), json!(server.include));
    }
    if !server.exclude.is_empty() {
        object.insert("exclude".to_string(), json!(server.exclude));
    }

    Value::Object(object)
}

fn string_map_json(values: &BTreeMap<String, String>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(key, value)| (key.clone(), json!(value)))
            .collect(),
    )
}

/// Maximum time to wait for the user to complete the browser authorization
/// before giving up and cleaning up the half-registered server. Generous
/// because the ceremony may include a sign-in step before the consent page
/// (authorization codes themselves live longer, e.g. 10 minutes on alink).
const OAUTH_REDIRECT_TIMEOUT: Duration = Duration::from_secs(300);

/// Connects an MCP server by URL, transparently running the OAuth flow when the
/// endpoint requires it.
///
/// Anda Bot runs locally, so authorization uses a native-app loopback redirect
/// (RFC 8252): the tool binds an ephemeral `127.0.0.1` port, opens the user's
/// browser at the authorization URL, and captures the redirect to finish the
/// flow — no copy/paste and no public callback URL.
///
/// A successful OAuth connection outlives the daemon: the tokens go to the
/// provider's credential store and the server (with an `oauth` marker, never
/// tokens) is persisted to mcp.json, so restarts reconnect silently from the
/// stored refresh token. When those credentials die (revoked in the remote
/// console, store deleted), calling this tool again on the same server runs a
/// fresh browser authorization instead of failing with "already exists".
#[derive(Clone)]
pub struct McpConnectTool {
    provider: Arc<McpToolProvider>,
    config_path: PathBuf,
    config_write_lock: Arc<Mutex<()>>,
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
}

impl McpConnectTool {
    pub const NAME: &'static str = "connect_mcp_server";

    pub fn new(
        provider: Arc<McpToolProvider>,
        config_path: PathBuf,
        config_write_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            provider,
            config_path,
            config_write_lock,
        }
    }

    async fn connect(&self, args: ConnectMcpServerArgs) -> Result<Value, BoxError> {
        let url = normalize_string(&args.url).ok_or("MCP server url cannot be empty")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("MCP server url must start with http:// or https://".into());
        }
        let id = match args.id.as_deref().and_then(normalize_string) {
            Some(id) => id,
            None => default_server_id_from_url(&url)?,
        };
        if self.provider.contains_server(&id) {
            match self.provider.refresh_server(&id).await {
                Ok(()) => return Ok(self.connected_summary(&id, false)),
                // Dead credentials (revoked, or the store was cleared): drop the
                // registration and run a fresh interactive authorization below.
                Err(err) if err.downcast_ref::<McpAuthorizationRequired>().is_some() => {
                    self.provider.remove_server(&id);
                }
                Err(err) => {
                    return Err(format!(
                        "MCP server {id} already exists but is unreachable: {err}"
                    )
                    .into());
                }
            }
        }

        // Let the endpoint itself decide the auth mode: OAuth servers advertise
        // authorization metadata, others (static bearer / none) do not.
        match McpToolProvider::discover_http_oauth(&url).await? {
            None => {
                self.provider
                    .add_server(McpServerConfig::streamable_http(id.clone(), url))
                    .await?;
                Ok(self.connected_summary(&id, false))
            }
            Some(meta) => {
                let scopes = self
                    .authorize_and_connect(&id, url.clone(), args.scopes, meta)
                    .await?;
                let persisted = self.persist_oauth_server(&id, url, scopes).await?;
                Ok(self.connected_summary(&id, persisted))
            }
        }
    }

    /// Runs the interactive flow and returns the scopes that were requested.
    async fn authorize_and_connect(
        &self,
        id: &str,
        url: String,
        scopes: Vec<String>,
        meta: McpOAuthMetadata,
    ) -> Result<Vec<String>, BoxError> {
        // Bind the loopback listener first so its port can be advertised as the
        // redirect URI before the authorization request is built.
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let redirect_uri = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr()?.port()
        );
        let scopes = if scopes.is_empty() {
            meta.scopes_supported
        } else {
            scopes
        };

        let mut config = McpServerConfig::streamable_http(id.to_string(), url);
        if let McpTransportConfig::StreamableHttp(http) = &mut config.transport {
            http.auth = Some(McpOAuthConfig::AuthorizationCode(
                OAuthAuthorizationCodeConfig {
                    redirect_uri,
                    scopes: scopes.clone(),
                    client_name: Some("Anda Bot".to_string()),
                    client_id: None,
                },
            ));
        }
        self.provider.register_server(config)?;

        // Any failure past registration must not leave a half-registered server.
        let result = self.run_authorization(id, listener).await;
        if result.is_err() {
            self.provider.remove_server(id);
        }
        result.map(|()| scopes)
    }

    /// Persists an OAuth server to mcp.json so it reconnects after a restart.
    /// Returns whether a new entry was written (re-authorizations find the
    /// entry already present). Tokens never touch mcp.json — they live in the
    /// provider's credential store.
    async fn persist_oauth_server(
        &self,
        id: &str,
        url: String,
        scopes: Vec<String>,
    ) -> Result<bool, BoxError> {
        let _guard = self.config_write_lock.lock().await;
        if mcp_config_contains_server(&self.config_path, id).await? {
            return Ok(false);
        }
        let server = McpServerSettings {
            id: id.to_string(),
            disabled: false,
            transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                url,
                bearer_token: None,
                headers: BTreeMap::new(),
                oauth: Some(McpOAuthSettings {
                    client_id: None,
                    scopes,
                }),
            }),
            include: BTreeSet::new(),
            exclude: BTreeSet::new(),
        };
        persist_mcp_server_config(&self.config_path, server)
            .await
            .map_err(|err| {
                format!(
                    "MCP server {id} is connected, but failed to persist to {}: {err}",
                    self.config_path.display()
                )
            })?;
        Ok(true)
    }

    async fn run_authorization(&self, id: &str, listener: TcpListener) -> Result<(), BoxError> {
        let auth_url = self.provider.begin_authorization(id).await?;
        log::info!("MCP `{id}` requires authorization. Opening browser: {auth_url}");
        // Best-effort: open the user's browser. Harmless if it is unavailable —
        // the URL is also logged for the user to open manually.
        let _ = open_in_browser(&auth_url);

        let redirect = timeout(OAUTH_REDIRECT_TIMEOUT, wait_for_oauth_redirect(listener)).await;
        let redirect_url = match redirect {
            Ok(result) => result?,
            Err(_) => {
                let secs = OAUTH_REDIRECT_TIMEOUT.as_secs();
                // The pending PKCE state and the loopback port die with this
                // attempt, so finishing the old authorization URL later cannot
                // work; only a fresh call can.
                return Err(format!(
                    "authorization timed out after {secs}s waiting for the browser redirect; \
                     call connect_mcp_server again to restart authorization"
                )
                .into());
            }
        };

        self.provider
            .complete_authorization(id, &redirect_url)
            .await?;
        self.provider.refresh_server(id).await?;
        Ok(())
    }

    fn connected_summary(&self, id: &str, newly_persisted: bool) -> Value {
        let tools = self
            .provider
            .routes()
            .into_iter()
            .filter(|route| route.server_id == id)
            .map(|route| json!({"name": route.name, "remote_name": route.remote_name}))
            .collect::<Vec<_>>();
        json!({
            "status": "connected",
            "server_id": id,
            "newly_persisted": newly_persisted,
            "tools": tools,
        })
    }
}

/// Returns whether mcp.json already carries a server with this id. A missing
/// or empty file simply means "no".
async fn mcp_config_contains_server(config_path: &Path, id: &str) -> Result<bool, BoxError> {
    let content = match read_text_file(config_path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let settings = McpSettings::from_json_contents(&content)?;
    Ok(settings.servers.iter().any(|server| server.id.trim() == id))
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
        let safe_url = redact_url_for_approval(&url);
        let mut details = vec![approval_detail("URL", &safe_url, "text")];
        if let Some(id) = args.id.as_deref() {
            details.push(approval_detail("Server id", id, "text"));
        }
        if !args.scopes.is_empty() {
            details.push(approval_detail("OAuth scopes", &args.scopes, "list"));
        }
        require_mcp_approval(
            &ctx,
            Self::NAME,
            format!("Connect MCP server: {safe_url}"),
            details,
            json!({ "url": &safe_url }),
        )
        .await?;

        let result = self.connect(args).await?;
        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

fn connect_mcp_server_parameters() -> serde_json::Value {
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
            }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

/// Derives a server id from a URL host, e.g. `https://api.al.ink/mcp` -> `api.al.ink`.
fn default_server_id_from_url(url: &str) -> Result<String, BoxError> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    // `user:pass@host` must yield the host, not the userinfo.
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        // Bracketed IPv6 literal: the colons inside are part of the host.
        rest.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    }
    .trim_end_matches('.');
    normalize_string(host)
        .ok_or_else(|| "cannot derive an MCP server id from the url; pass an explicit id".into())
}

/// Serves the loopback redirect endpoint and returns the full redirect URL once
/// the authorization server sends the user back with `code`/`state`.
async fn wait_for_oauth_redirect(listener: TcpListener) -> Result<String, BoxError> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        // Per-connection failures (port probes, browser preconnects, clients
        // hanging up early) must not abort the authorization; keep listening.
        let Some(request_line) = read_request_line(&mut stream).await else {
            continue;
        };
        let Some(target) = request_line.split_whitespace().nth(1) else {
            let _ = write_http_response(&mut stream, "400 Bad Request", "Bad Request").await;
            continue;
        };
        // Ignore ancillary requests such as /favicon.ico; wait for the callback.
        if !(target.contains("code=") || target.contains("error=")) {
            let _ = write_http_response(&mut stream, "404 Not Found", "Not Found").await;
            continue;
        }
        // Best-effort: the redirect is already captured even if the browser
        // never renders the confirmation page.
        let _ = write_http_response(
            &mut stream,
            "200 OK",
            "授权完成，可以关闭此页面并返回 Anda Bot。\nAuthorization complete; you can close this tab.",
        )
        .await;
        return Ok(format!("http://127.0.0.1{target}"));
    }
}

/// Reads from the connection until the request line is complete (terminated by
/// a newline) and returns it. `None` when the client disconnects or errors
/// before sending one, or floods more than 16 KiB without a line break.
async fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    while !buffer.contains(&b'\n') {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
        if buffer.len() > 16 * 1024 {
            return None;
        }
    }
    let request = String::from_utf8_lossy(&buffer);
    request.lines().next().map(str::to_string)
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    body: &str,
) -> Result<(), BoxError> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// Best-effort launch of the user's default browser at `url`.
///
/// Only http/https URLs are opened: the authorization URL is built from the
/// remote server's discovery metadata, so treat it as untrusted input to the
/// local system.
fn open_in_browser(url: &str) -> std::io::Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to open a non-http(s) URL in the browser",
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    // Not `cmd /C start`: cmd.exe would reparse the URL, splitting on `&`
    // (executing the rest as commands) and expanding `%..%` sequences.
    // rundll32 receives the URL as a plain argument.
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");

    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::StateFeatures;
    use anda_engine::extension::mcp::McpServerConfig;

    /// A mock ctx whose ActionSession auto-approves every approval card, so
    /// tool-level tests can exercise the behavior behind the approval gate.
    fn auto_approving_ctx() -> BaseCtx {
        use super::super::action::{ActionEvent, ActionResponseArgs, ActionRuntime, ActionSession};

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
                    && let Some(action_id) = super::super::action_id_from_message(&message)
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
                            },
                        )
                        .await;
                }
            }
        });
        ctx
    }

    fn test_tool() -> McpServerTool {
        let provider = Arc::new(McpToolProvider::new(Vec::new()).unwrap());
        let home = PathBuf::from("/tmp/anda-home");
        McpServerTool::new(
            provider,
            home.clone(),
            Some(home.join("workspace")),
            McpSettings::file_path(&home),
            Arc::new(Mutex::new(())),
        )
    }

    #[test]
    fn add_mcp_server_schema_matches_mcp_json_server_shape() {
        let tool = test_tool();
        let definition = tool.definition();
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
    fn add_mcp_server_args_convert_to_stdio_settings() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: " filesystem ".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some(" npx ".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                ],
                env: BTreeMap::from([(" TOKEN ".to_string(), "secret".to_string())]),
                cwd: Some(" workspace ".to_string()),
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: None,
                include: vec![" read_file ".to_string(), " ".to_string()],
                exclude: vec!["write_file".to_string()],
                persist: false,
            })
            .unwrap();

        assert_eq!(server.id, "filesystem");
        assert_eq!(server.include, BTreeSet::from(["read_file".to_string()]));
        assert_eq!(server.exclude, BTreeSet::from(["write_file".to_string()]));
        match server.transport {
            McpTransportSettings::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(stdio.cwd.as_deref(), Some("workspace"));
                assert_eq!(stdio.env.get("TOKEN").map(String::as_str), Some("secret"));
            }
            _ => panic!("expected stdio"),
        }
    }

    #[test]
    fn add_mcp_server_args_convert_to_http_settings() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: "remote".to_string(),
                r#type: Some(McpServerTransportType::Http),
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: Some(" https://mcp.example.test/mcp ".to_string()),
                bearer_token: Some(" token ".to_string()),
                headers: BTreeMap::from([("x-client".to_string(), "anda".to_string())]),
                enabled: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            })
            .unwrap();

        match server.transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
                assert_eq!(http.bearer_token.as_deref(), Some("token"));
                assert_eq!(
                    http.headers.get("x-client").map(String::as_str),
                    Some("anda")
                );
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn add_mcp_server_args_infer_transport_and_enabled_false() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: "remote".to_string(),
                r#type: None,
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: Some("https://mcp.example.test/mcp".to_string()),
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: Some(false),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            })
            .unwrap();

        assert!(server.disabled);
        match server.transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[tokio::test]
    async fn persist_mcp_server_config_appends_server() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(&config_path, "{\n  \"note\": true\n}\n")
            .await
            .unwrap();

        persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let settings = McpSettings::from_file(dir.path()).await.unwrap();
        assert_eq!(settings.servers.len(), 1);
        assert_eq!(settings.servers[0].id, "remote");

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["note"], true);
        assert_eq!(json["mcpServers"]["remote"]["type"], "http");
        assert_eq!(
            json["mcpServers"]["remote"]["url"],
            "https://mcp.example.test/mcp"
        );
    }

    #[tokio::test]
    async fn persist_mcp_server_config_preserves_existing_servers_root() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(
            &config_path,
            r#"{
  "servers": {
    "existing": {
      "type": "stdio",
      "command": "existing-mcp"
    }
  },
  "other": 1
}
"#,
        )
        .await
        .unwrap();

        persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["other"], 1);
        assert!(json.get("mcpServers").is_none());
        assert_eq!(json["servers"]["existing"]["command"], "existing-mcp");
        assert_eq!(json["servers"]["remote"]["type"], "http");

        let settings = McpSettings::from_file(dir.path()).await.unwrap();
        assert_eq!(settings.servers.len(), 2);
    }

    #[tokio::test]
    async fn persist_mcp_server_config_rejects_non_object_root() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(&config_path, "[]\n").await.unwrap();

        let err = persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("root must be an object"));
    }

    #[tokio::test]
    async fn call_requires_approval_outside_full_access() {
        // A plain mock ctx runs in OnRisk mode with no ActionSession, so both
        // MCP tools must fail closed instead of executing.
        let err = Tool::call(
            &test_tool(),
            anda_engine::engine::EngineBuilder::new().mock_ctx().base,
            AddMcpServerArgs {
                id: "srv".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some("missing-command".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: None,
                include: Vec::new(),
                exclude: Vec::new(),
                persist: false,
            },
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("approval"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        let connect_tool = McpConnectTool::new(
            Arc::new(McpToolProvider::new(Vec::new()).unwrap()),
            McpSettings::file_path(dir.path()),
            Arc::new(Mutex::new(())),
        );
        let err = Tool::call(
            &connect_tool,
            anda_engine::engine::EngineBuilder::new().mock_ctx().base,
            ConnectMcpServerArgs {
                url: "http://127.0.0.1:9/mcp".to_string(),
                id: None,
                scopes: Vec::new(),
            },
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("approval"), "{err}");
    }

    #[tokio::test]
    async fn call_persists_disabled_server_without_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let tool = McpServerTool::new(
            Arc::new(McpToolProvider::new(Vec::new()).unwrap()),
            dir.path().to_path_buf(),
            None,
            McpSettings::file_path(dir.path()),
            Arc::new(Mutex::new(())),
        );

        let output = Tool::call(
            &tool,
            auto_approving_ctx(),
            AddMcpServerArgs {
                id: "disabled".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some("missing-command".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: Some(false),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            },
            Vec::new(),
        )
        .await
        .unwrap();

        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["status"], "saved_disabled");
                assert_eq!(result["enabled"], false);
            }
            _ => panic!("expected ok response"),
        }
        assert!(tool.provider.routes().is_empty());

        let content = tokio::fs::read_to_string(McpSettings::file_path(dir.path()))
            .await
            .unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["mcpServers"]["disabled"]["enabled"], false);
        assert_eq!(json["mcpServers"]["disabled"]["command"], "missing-command");
    }

    #[tokio::test]
    async fn call_rejects_duplicate_runtime_server_before_connecting() {
        let provider = Arc::new(
            McpToolProvider::new(vec![McpServerConfig::stdio("dupe", "missing-command")]).unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let tool = McpServerTool::new(
            provider,
            dir.path().to_path_buf(),
            None,
            McpSettings::file_path(dir.path()),
            Arc::new(Mutex::new(())),
        );

        let err = Tool::call(
            &tool,
            anda_engine::engine::EngineBuilder::new().mock_ctx().base,
            AddMcpServerArgs {
                id: "dupe".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some("missing-command".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: None,
                include: Vec::new(),
                exclude: Vec::new(),
                persist: false,
            },
            Vec::new(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("already exists"));
    }

    fn test_connect_tool(config_path: PathBuf) -> McpConnectTool {
        let provider = Arc::new(McpToolProvider::new(Vec::new()).unwrap());
        McpConnectTool::new(provider, config_path, Arc::new(Mutex::new(())))
    }

    #[tokio::test]
    async fn persist_oauth_server_writes_marker_and_skips_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        let tool = test_connect_tool(config_path.clone());

        let newly = tool
            .persist_oauth_server(
                "alink",
                "https://api.al.ink/mcp".to_string(),
                vec!["events:read".to_string()],
            )
            .await
            .unwrap();
        assert!(newly);

        // The entry re-parses with the oauth marker and no token material.
        let settings = McpSettings::from_file(dir.path()).await.unwrap();
        assert_eq!(settings.servers.len(), 1);
        match &settings.servers[0].transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://api.al.ink/mcp");
                assert!(http.bearer_token.is_none());
                let oauth = http.oauth.as_ref().expect("oauth marker");
                assert_eq!(oauth.scopes, vec!["events:read".to_string()]);
            }
            _ => panic!("expected HTTP"),
        }
        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        assert!(!content.contains("token"), "no tokens in mcp.json");

        // Re-authorization finds the entry already present and does not fail.
        let newly = tool
            .persist_oauth_server("alink", "https://api.al.ink/mcp".to_string(), Vec::new())
            .await
            .unwrap();
        assert!(!newly);
    }

    #[tokio::test]
    async fn mcp_config_contains_server_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        assert!(
            !mcp_config_contains_server(&config_path, "alink")
                .await
                .unwrap()
        );
    }

    #[test]
    fn default_server_id_from_url_uses_host() {
        assert_eq!(
            default_server_id_from_url("https://api.al.ink/mcp").unwrap(),
            "api.al.ink"
        );
        assert_eq!(
            default_server_id_from_url("http://127.0.0.1:8080/mcp?x=1").unwrap(),
            "127.0.0.1"
        );
        assert_eq!(
            default_server_id_from_url("https://user:pass@api.al.ink/mcp").unwrap(),
            "api.al.ink"
        );
        assert_eq!(
            default_server_id_from_url("http://[::1]:8080/mcp").unwrap(),
            "::1"
        );
        assert!(default_server_id_from_url("https:///mcp").is_err());
    }

    #[test]
    fn open_in_browser_rejects_non_http_urls() {
        assert!(open_in_browser("javascript:alert(1)").is_err());
        assert!(open_in_browser("file:///etc/passwd").is_err());
    }

    #[tokio::test]
    async fn oauth_redirect_survives_stray_connections_and_split_requests() {
        use tokio::io::AsyncWriteExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let redirect = tokio::spawn(wait_for_oauth_redirect(listener));

        // A probe that connects and hangs up without sending anything.
        drop(TcpStream::connect(addr).await.unwrap());
        // An ancillary request (favicon) on its own connection.
        let mut favicon = TcpStream::connect(addr).await.unwrap();
        favicon
            .write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        drop(favicon);

        // The real callback, with the request line split across two writes.
        let mut callback = TcpStream::connect(addr).await.unwrap();
        callback.write_all(b"GET /callback?co").await.unwrap();
        callback.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        callback
            .write_all(b"de=abc&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();

        let url = timeout(Duration::from_secs(5), redirect)
            .await
            .expect("redirect must resolve")
            .unwrap()
            .unwrap();
        assert_eq!(url, "http://127.0.0.1/callback?code=abc&state=xyz");
    }

    #[tokio::test]
    async fn connect_rejects_non_http_urls() {
        let dir = tempfile::tempdir().unwrap();
        let tool = test_connect_tool(McpSettings::file_path(dir.path()));
        let err = tool
            .connect(ConnectMcpServerArgs {
                url: "ftp://example.test/mcp".to_string(),
                id: None,
                scopes: Vec::new(),
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("http:// or https://"));
    }
}
