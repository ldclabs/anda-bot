use anda_core::BoxError;
use anda_engine::extension::mcp::{
    McpConcurrency, McpLifecycle, McpLimits, McpOAuthConfig, McpServerConfig, McpStartup,
    McpStdioTransport, McpStreamableHttpTransport, McpTasksConfig, McpTimeouts, McpTransportConfig,
    OAuthAuthorizationCodeConfig,
};
use http::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::util::command_path::command_path;

use super::normalize_string;

pub const MCP_CONFIG_FILE_NAME: &str = "mcp.json";

/// The values of `${secret:NAME}` references, by name. They come from the MCP
/// secret store (`mcp_secrets.json`), never from mcp.json itself.
pub type McpSecretValues = BTreeMap<String, String>;

/// MCP host/client configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpSettings {
    /// MCP servers exposed as dynamic Anda tools.
    #[serde(default)]
    pub servers: Vec<McpServerSettings>,
    /// Problems found while reading mcp.json. The entries they name are left
    /// out of `servers`: MCP is optional, so a broken entry must cost neither
    /// the other servers nor the daemon its start.
    #[serde(skip)]
    pub diagnostics: Vec<McpDiagnostic>,
}

/// A problem with mcp.json as a whole or with one of its entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpDiagnostic {
    /// The entry it concerns, or `None` for the whole file.
    pub server_id: Option<String>,
    pub message: String,
}

impl McpDiagnostic {
    fn file(message: impl Into<String>) -> Self {
        Self {
            server_id: None,
            message: message.into(),
        }
    }

    fn server(id: &str, problem: impl fmt::Display) -> Self {
        Self {
            server_id: Some(id.to_string()),
            message: format!("MCP server {id:?} in mcp.json was skipped: {problem}"),
        }
    }
}

impl fmt::Display for McpDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl McpSettings {
    pub fn file_path(home_dir: &Path) -> PathBuf {
        home_dir.join(MCP_CONFIG_FILE_NAME)
    }

    /// Parses the contents of the mcp.json at `path`. Never fails: a file that
    /// cannot be parsed becomes a diagnostic and loads no servers.
    pub fn from_file_contents(path: &Path, content: &str) -> Self {
        Self::from_json_contents(content).unwrap_or_else(|err| Self::unreadable(path, err))
    }

    /// The settings of an mcp.json that could not be read: no servers, and a
    /// diagnostic that says why.
    pub fn unreadable(path: &Path, err: impl fmt::Display) -> Self {
        Self {
            servers: Vec::new(),
            diagnostics: vec![McpDiagnostic::file(format!(
                "{} was not loaded: {err}",
                path.display()
            ))],
        }
    }

    /// Parses one entry written the way mcp.json writes it under
    /// `mcpServers`, for a server named `id`.
    pub fn parse_entry(id: &str, entry: &Value) -> Result<McpServerSettings, BoxError> {
        serde_json::from_value::<McpJsonServer>(entry.clone())?.into_settings(id.to_string())
    }

    /// Parses mcp.json contents. Only a file that is not a JSON object is an
    /// error, since nothing in it can be read or edited; an entry that cannot
    /// be used becomes a diagnostic instead.
    pub fn from_json_contents(content: &str) -> Result<Self, BoxError> {
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        let Value::Object(root) = serde_json::from_str(content)? else {
            return Err("mcp.json root must be an object".into());
        };
        let mut settings = Self::default();
        for key in ["mcpServers", "servers"] {
            settings.read_servers(key, root.get(key));
        }
        settings.drop_unusable_servers();
        Ok(settings)
    }

    /// Whether mcp.json declares `id`, including an entry skipped as invalid.
    pub fn declares(&self, id: &str) -> bool {
        self.servers.iter().any(|server| server.id.trim() == id)
            || self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.server_id.as_deref().map(str::trim) == Some(id))
    }

    /// The diagnostic of an entry declared as `id` that was skipped as
    /// invalid, when no usable entry has that id.
    pub fn skipped(&self, id: &str) -> Option<&McpDiagnostic> {
        if self.servers.iter().any(|server| server.id.trim() == id) {
            return None;
        }
        self.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.server_id.as_deref().map(str::trim) == Some(id))
    }

    /// Builds the engine configs for the enabled servers. An entry that cannot
    /// be built (one naming an unset environment variable or secret, say) is
    /// reported in the returned diagnostics and left out.
    pub fn server_configs(
        &self,
        home_dir: &Path,
        default_cwd: Option<&Path>,
        secrets: &McpSecretValues,
    ) -> (Vec<McpServerConfig>, Vec<McpDiagnostic>) {
        let mut configs = Vec::new();
        let mut diagnostics = Vec::new();
        for server in self.servers.iter().filter(|server| !server.disabled) {
            match server.server_config(home_dir, default_cwd, secrets) {
                Ok(config) => configs.push(config),
                Err(err) => diagnostics.push(McpDiagnostic::server(server.id.trim(), err)),
            }
        }
        (configs, diagnostics)
    }

    fn read_servers(&mut self, key: &str, value: Option<&Value>) {
        match value {
            None | Some(Value::Null) => {}
            Some(Value::Object(entries)) => {
                for (id, entry) in entries {
                    match Self::parse_entry(id, entry) {
                        Ok(server) => self.servers.push(server),
                        Err(err) => self.diagnostics.push(McpDiagnostic::server(id, err)),
                    }
                }
            }
            Some(Value::Array(entries)) => {
                for (index, entry) in entries.iter().enumerate() {
                    match serde_json::from_value::<McpServerSettings>(entry.clone()) {
                        Ok(server) => self.servers.push(server),
                        Err(err) => {
                            let id = entry["id"]
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("{key}[{index}]"));
                            self.diagnostics.push(McpDiagnostic::server(&id, err));
                        }
                    }
                }
            }
            Some(_) => self.diagnostics.push(McpDiagnostic::file(format!(
                "mcp.json {key} must be an object of servers; it was ignored"
            ))),
        }
    }

    /// Moves enabled entries that cannot start (no id, an id declared twice,
    /// or a field that does not validate) into `diagnostics`. Disabled entries
    /// stay as written: nothing starts them, so nothing checks them.
    fn drop_unusable_servers(&mut self) {
        let mut seen_ids = BTreeSet::new();
        let mut ingress: Option<String> = None;
        for server in std::mem::take(&mut self.servers) {
            if server.disabled {
                self.servers.push(server);
                continue;
            }
            let id = server.id.trim().to_string();
            let issues = server.setup_issues();
            if !issues.is_empty() {
                self.diagnostics
                    .push(McpDiagnostic::server(&id, issues.join("; ")));
            } else if !seen_ids.insert(id.clone()) {
                self.diagnostics.push(McpDiagnostic::server(
                    &id,
                    "the id is declared more than once; only the first entry is used",
                ));
            } else {
                if server
                    .events
                    .as_ref()
                    .is_some_and(|events| events.webhook_ingress)
                {
                    // The server still runs; it only does not relay webhooks.
                    match &ingress {
                        Some(first) => self.diagnostics.push(McpDiagnostic {
                            server_id: Some(id.clone()),
                            message: format!(
                                "MCP server {id:?} in mcp.json does not relay webhooks: \
                                 events.webhook_ingress is set on more than one server; {first} \
                                 receives the webhooks"
                            ),
                        }),
                        None => ingress = Some(id.clone()),
                    }
                }
                self.servers.push(server);
            }
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct McpJsonServer {
    #[serde(default, rename = "type")]
    transport_type: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default, alias = "environment")]
    env: BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<McpOAuthSettings>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    include: BTreeSet<String>,
    #[serde(default)]
    exclude: BTreeSet<String>,
    // Taken as strings rather than the enums so a mistyped mode is reported
    // by name instead of as a bare serde type error.
    #[serde(default)]
    lifecycle: Option<String>,
    #[serde(default)]
    startup: Option<String>,
    #[serde(default)]
    tasks: Option<McpTasksConfig>,
    #[serde(default)]
    approval: Option<McpJsonApproval>,
    #[serde(default)]
    allow_external_users: bool,
    #[serde(default)]
    inherit_env: Option<bool>,
    #[serde(default)]
    timeouts: McpTimeoutSettings,
    #[serde(default)]
    concurrency: Option<String>,
    #[serde(default)]
    limits: McpLimitSettings,
    #[serde(default)]
    events: Option<McpEventSettings>,
    #[serde(default)]
    resources: Option<bool>,
    #[serde(default)]
    elicitation: Option<bool>,
}

impl McpJsonServer {
    fn into_settings(self, id: String) -> Result<McpServerSettings, BoxError> {
        let Self {
            transport_type,
            command,
            args,
            env,
            cwd,
            url,
            bearer_token,
            headers,
            oauth,
            enabled,
            disabled,
            include,
            exclude,
            lifecycle,
            startup,
            tasks,
            approval,
            allow_external_users,
            inherit_env,
            timeouts,
            concurrency,
            limits,
            events,
            resources,
            elicitation,
        } = self;

        let disabled = disabled || enabled == Some(false);
        let approval = approval
            .map(McpJsonApproval::into_settings)
            .transpose()?
            .unwrap_or_default();
        let lifecycle = match normalized_mode(lifecycle).as_deref() {
            None => None,
            Some("auto") => Some(McpLifecycle::Auto),
            Some("discover") => Some(McpLifecycle::Discover),
            Some("initialize") => Some(McpLifecycle::Initialize),
            Some(other) => {
                return Err(format!(
                    "lifecycle has unsupported value {other:?}, expected auto, discover, or initialize"
                )
                .into());
            }
        };
        let startup = match normalized_mode(startup).as_deref() {
            None => None,
            Some("background") => Some(McpStartup::Background),
            Some("eager") => Some(McpStartup::Eager),
            Some(other) => {
                return Err(format!(
                    "startup has unsupported value {other:?}, expected background or eager"
                )
                .into());
            }
        };
        let concurrency = match normalized_mode(concurrency).as_deref() {
            None => None,
            Some("serial") => Some(McpConcurrency::Serial),
            Some("read_only_parallel") | Some("read-only-parallel") => {
                Some(McpConcurrency::ReadOnlyParallel)
            }
            Some("parallel") => Some(McpConcurrency::Parallel),
            Some(other) => {
                return Err(format!(
                    "concurrency has unsupported value {other:?}, expected serial, read_only_parallel, or parallel"
                )
                .into());
            }
        };
        let is_set =
            |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
        let stdio = match normalized_mode(transport_type).as_deref() {
            Some("stdio") => true,
            Some("http") | Some("streamable_http") | Some("streamable-http") => false,
            Some("sse") => {
                return Err(
                    "type \"sse\" is not supported: the SSE transport is deprecated; \
                     use type \"http\" if the server also offers Streamable HTTP"
                        .into(),
                );
            }
            Some(other) => return Err(format!("type has unsupported transport {other:?}").into()),
            None if is_set(&command) => true,
            None if is_set(&url) => false,
            None => return Err("type is missing and neither command nor url is set".into()),
        };
        let transport = if stdio {
            McpTransportSettings::Stdio(McpStdioSettings {
                command: command.unwrap_or_default(),
                args,
                env,
                cwd,
                inherit_env,
            })
        } else {
            if inherit_env.is_some() {
                return Err("inherit_env is only for local (stdio) servers".into());
            }
            McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                url: url.unwrap_or_default(),
                bearer_token,
                headers,
                oauth,
            })
        };

        Ok(McpServerSettings {
            id,
            disabled,
            transport,
            include,
            exclude,
            lifecycle,
            startup,
            tasks,
            approval,
            allow_external_users,
            timeouts,
            concurrency,
            limits,
            events,
            resources,
            elicitation,
        })
    }
}

fn normalized_mode(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(normalize_string)
        .map(|value| value.to_ascii_lowercase())
}

/// One MCP server entry.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpServerSettings {
    /// Stable server id used in local tool names and audit output.
    #[serde(default)]
    pub id: String,
    /// Temporarily skip this server without deleting the entry.
    #[serde(default)]
    pub disabled: bool,
    /// Server transport.
    #[serde(default)]
    pub transport: McpTransportSettings,
    /// Optional remote tool allowlist. Empty means all tools except excluded.
    #[serde(default)]
    pub include: BTreeSet<String>,
    /// Optional remote tool denylist.
    #[serde(default)]
    pub exclude: BTreeSet<String>,
    /// How the session negotiates the MCP protocol revision: `auto` (default,
    /// probes `server/discover` and falls back to the legacy handshake),
    /// `discover`, or `initialize` to pin a server that mishandles unknown
    /// methods. Left out of the file unless the operator set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<McpLifecycle>,
    /// When the daemon discovers this server's tools: `background` (default)
    /// after the daemon is up, or `eager` before it reports ready. Either way
    /// a server that cannot be reached is skipped rather than failing the
    /// daemon. Left out of the file unless the operator set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<McpStartup>,
    /// SEP-2663 tasks extension. Absent leaves it undeclared, so the server
    /// must answer `tools/call` inline; declaring it lets a long-running tool
    /// hand back a task the provider polls to completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<McpTasksConfig>,
    /// When the agent asks before it calls one of the server's tools. Only
    /// the call gate reads it, so changing it never reconnects the server.
    #[serde(default, skip_serializing_if = "McpApprovalSettings::is_empty")]
    pub approval: McpApprovalSettings,
    /// Whether runs for external IM users may call the server's tools. Off,
    /// they can only use the owner's servers that allow it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_external_users: bool,
    /// Deadlines that override the engine's, in seconds.
    #[serde(default, skip_serializing_if = "McpTimeoutSettings::is_empty")]
    pub timeouts: McpTimeoutSettings,
    /// Which of the server's tools may run at the same time. Absent runs
    /// them one at a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<McpConcurrency>,
    /// Bounds that override the engine's.
    #[serde(default, skip_serializing_if = "McpLimitSettings::is_empty")]
    pub limits: McpLimitSettings,
    /// How the server takes part in MCP event automations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<McpEventSettings>,
    /// Whether the agent and the apps may list and read the server's
    /// resources. Absent means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<bool>,
    /// Whether the server may ask the user for input while a call runs
    /// (elicitation). Absent means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<bool>,
}

/// An entry's `events` settings.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpEventSettings {
    /// This server (dMsg) receives webhooks for event automations whose
    /// events are delivered only by webhook, and relays them to Anda. Only
    /// one server can be the ingress.
    #[serde(default)]
    pub webhook_ingress: bool,
}

/// The longest a timeout may be: a day, as the engine allows.
const MAX_TIMEOUT_SECS: u64 = 86_400;
/// The bounds of `limits.output_text_bytes`: the engine needs at least 256
/// bytes, and more than a megabyte of text in one result floods the model.
const MIN_OUTPUT_TEXT_BYTES: usize = 256;
const MAX_OUTPUT_TEXT_BYTES: usize = 1024 * 1024;

/// Timeouts in seconds; each one left out keeps the engine's default.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpTimeoutSettings {
    /// Connecting, including sign-in and protocol negotiation (90).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_secs: Option<u64>,
    /// Listing the server's tools (30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_secs: Option<u64>,
    /// One request within a tool call (180).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_secs: Option<u64>,
    /// A whole tool call, including waiting for a task (600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_secs: Option<u64>,
    /// One elicitation answer (300): how long the user has to answer a
    /// server's request for input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation_secs: Option<u64>,
}

impl McpTimeoutSettings {
    /// The JSON keys of the timeouts, as mcp.json writes them.
    pub const FIELDS: [&'static str; 5] = [
        "setup_secs",
        "list_secs",
        "request_secs",
        "call_secs",
        "elicitation_secs",
    ];

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn values(&self) -> [Option<u64>; 5] {
        [
            self.setup_secs,
            self.list_secs,
            self.request_secs,
            self.call_secs,
            self.elicitation_secs,
        ]
    }

    fn to_timeouts(&self) -> McpTimeouts {
        let defaults = McpTimeouts::default();
        McpTimeouts {
            setup_secs: self.setup_secs.unwrap_or(defaults.setup_secs),
            list_secs: self.list_secs.unwrap_or(defaults.list_secs),
            request_secs: self.request_secs.unwrap_or(defaults.request_secs),
            call_secs: self.call_secs.unwrap_or(defaults.call_secs),
            elicitation_secs: self.elicitation_secs.unwrap_or(defaults.elicitation_secs),
        }
    }
}

/// Bounds on what the server sends; each one left out keeps the engine's.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpLimitSettings {
    /// Text the model gets from one tool result, in bytes (32 KiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_text_bytes: Option<usize>,
}

impl McpLimitSettings {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn to_limits(&self) -> McpLimits {
        let mut limits = McpLimits::default();
        if let Some(bytes) = self.output_text_bytes {
            limits.output_text_bytes = bytes;
        }
        limits
    }
}

/// The advanced settings of a server, as the MCP page and `anda mcp options`
/// set them together. Each one left out takes its default.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<McpStartup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<McpLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<McpConcurrency>,
    #[serde(default, skip_serializing_if = "McpTimeoutSettings::is_empty")]
    pub timeouts: McpTimeoutSettings,
    #[serde(default, skip_serializing_if = "McpLimitSettings::is_empty")]
    pub limits: McpLimitSettings,
    /// Local servers only: whether the process gets the daemon's whole
    /// environment (the default) or only the platform's essentials and `env`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit_env: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<McpTasksConfig>,
    /// Resources the agent and the apps may read; absent means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<bool>,
    /// Input the server may ask the user for; absent means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<bool>,
}

/// When the agent asks before it calls a tool.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpApproval {
    /// Ask unless the session runs with full access, or the tool is a
    /// read-only one whose definition was reviewed.
    #[default]
    Auto,
    /// Always ask, even with full access.
    Ask,
    /// Never ask while the tool's definition is the reviewed one.
    Allow,
}

impl McpApproval {
    fn parse(value: &str, field: &str) -> Result<Self, BoxError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "ask" => Ok(Self::Ask),
            "allow" => Ok(Self::Allow),
            other => Err(format!(
                "{field} has unsupported value {other:?}, expected auto, ask, or allow"
            )
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
            Self::Allow => "allow",
        }
    }
}

/// A server's approval policy: a default, and overrides by remote tool name.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpApprovalSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<McpApproval>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, McpApproval>,
}

impl McpApprovalSettings {
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.tools.is_empty()
    }

    /// The policy of `tool`: its own, else the server's, else `auto`.
    pub fn for_tool(&self, tool: &str) -> McpApproval {
        self.tools
            .get(tool)
            .or(self.default.as_ref())
            .copied()
            .unwrap_or_default()
    }
}

/// `approval` as mcp.json writes it, read as strings so a mistyped policy is
/// reported by name.
#[derive(Clone, Debug, Default, Deserialize)]
struct McpJsonApproval {
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    tools: BTreeMap<String, String>,
}

impl McpJsonApproval {
    fn into_settings(self) -> Result<McpApprovalSettings, BoxError> {
        let default = self
            .default
            .as_deref()
            .map(|value| McpApproval::parse(value, "approval.default"))
            .transpose()?;
        let mut tools = BTreeMap::new();
        for (tool, value) in self.tools {
            let approval = McpApproval::parse(&value, &format!("approval.tools.{tool}"))?;
            let tool = tool.trim();
            if tool.is_empty() {
                return Err("approval.tools has an empty tool name".into());
            }
            tools.insert(tool.to_string(), approval);
        }
        Ok(McpApprovalSettings { default, tools })
    }
}

impl McpServerSettings {
    /// Problems that keep this entry from starting, named by their mcp.json
    /// fields. Environment references are checked against the daemon's own
    /// environment.
    pub fn setup_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.id.trim().is_empty() {
            issues.push("id is empty".to_string());
        }
        self.transport
            .setup_issues(&McpExpansionVars::validation(), &mut issues);
        for (field, value) in McpTimeoutSettings::FIELDS
            .iter()
            .zip(self.timeouts.values())
        {
            if value.is_some_and(|secs| !(1..=MAX_TIMEOUT_SECS).contains(&secs)) {
                issues.push(format!(
                    "timeouts.{field} must be between 1 and {MAX_TIMEOUT_SECS} seconds"
                ));
            }
        }
        if self
            .limits
            .output_text_bytes
            .is_some_and(|bytes| !(MIN_OUTPUT_TEXT_BYTES..=MAX_OUTPUT_TEXT_BYTES).contains(&bytes))
        {
            issues.push(format!(
                "limits.output_text_bytes must be between {MIN_OUTPUT_TEXT_BYTES} and {MAX_OUTPUT_TEXT_BYTES}"
            ));
        }
        issues
    }

    /// The advanced settings of this server.
    pub fn options(&self) -> McpServerOptions {
        McpServerOptions {
            startup: self.startup,
            lifecycle: self.lifecycle,
            concurrency: self.concurrency,
            timeouts: self.timeouts.clone(),
            limits: self.limits.clone(),
            inherit_env: match &self.transport {
                McpTransportSettings::Stdio(stdio) => stdio.inherit_env,
                McpTransportSettings::StreamableHttp(_) => None,
            },
            tasks: self.tasks.clone(),
            resources: self.resources,
            elicitation: self.elicitation,
        }
    }

    /// Replaces the advanced settings. `inherit_env` is refused for a remote
    /// server, which runs no process.
    pub fn set_options(&mut self, options: McpServerOptions) -> Result<(), BoxError> {
        let McpServerOptions {
            startup,
            lifecycle,
            concurrency,
            timeouts,
            limits,
            inherit_env,
            tasks,
            resources,
            elicitation,
        } = options;
        match &mut self.transport {
            McpTransportSettings::Stdio(stdio) => stdio.inherit_env = inherit_env,
            McpTransportSettings::StreamableHttp(_) if inherit_env.is_some() => {
                return Err("inherit_env is only for local (stdio) servers".into());
            }
            McpTransportSettings::StreamableHttp(_) => {}
        }
        self.startup = startup;
        self.lifecycle = lifecycle;
        self.concurrency = concurrency;
        self.timeouts = timeouts;
        self.limits = limits;
        self.tasks = tasks;
        self.resources = resources;
        self.elicitation = elicitation;
        Ok(())
    }

    /// Builds the engine config, expanding environment and secret references.
    pub fn server_config(
        &self,
        home_dir: &Path,
        default_cwd: Option<&Path>,
        secrets: &McpSecretValues,
    ) -> Result<McpServerConfig, BoxError> {
        let vars = McpExpansionVars::new(home_dir, default_cwd, secrets);
        Ok(McpServerConfig {
            id: self.id.trim().to_string(),
            transport: self.transport.to_transport_config(&vars, default_cwd)?,
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            lifecycle: self.lifecycle.unwrap_or_default(),
            tasks: self.tasks.clone(),
            limits: self.limits.to_limits(),
            timeouts: self.timeouts.to_timeouts(),
            concurrency: self.concurrency.unwrap_or_default(),
            // Never `required`: an unreachable server must not stop the daemon.
            required: false,
            // Discovery waits until after startup unless the entry asks for
            // it, so a slow server (or an `npx -y` download) does not hold the
            // daemon back.
            startup: self.startup.unwrap_or(McpStartup::Background),
            // Both on unless the entry turns them off. The manager turns
            // elicitation off on a provider that has nobody to ask.
            elicitation: self.elicitation.unwrap_or(true),
            resources: self.resources.unwrap_or(true),
        })
    }
}

impl McpServerSettings {
    /// The secrets this entry references as `${secret:NAME}`.
    pub fn secret_names(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        let mut scan = |value: &str| secret_references(value, &mut names);
        match &self.transport {
            McpTransportSettings::Stdio(stdio) => {
                scan(&stdio.command);
                stdio.args.iter().for_each(|arg| scan(arg));
                stdio.env.values().for_each(|value| scan(value));
                if let Some(cwd) = &stdio.cwd {
                    scan(cwd);
                }
            }
            McpTransportSettings::StreamableHttp(http) => {
                scan(&http.url);
                if let Some(token) = &http.bearer_token {
                    scan(token);
                }
                http.headers.values().for_each(|value| scan(value));
            }
        }
        names
    }
}

/// MCP transport configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum McpTransportSettings {
    /// stdio child process transport.
    #[serde(rename = "stdio")]
    Stdio(McpStdioSettings),
    /// Streamable HTTP transport.
    #[serde(rename = "http", alias = "streamable_http")]
    StreamableHttp(McpStreamableHttpSettings),
}

impl McpTransportSettings {
    fn setup_issues(&self, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        match self {
            Self::Stdio(stdio) => stdio.setup_issues(vars, issues),
            Self::StreamableHttp(http) => http.setup_issues(vars, issues),
        }
    }

    fn to_transport_config(
        &self,
        vars: &McpExpansionVars,
        default_cwd: Option<&Path>,
    ) -> Result<McpTransportConfig, BoxError> {
        match self {
            Self::Stdio(stdio) => stdio
                .to_transport(vars, default_cwd)
                .map(McpTransportConfig::Stdio),
            Self::StreamableHttp(http) => http
                .to_transport(vars)
                .map(McpTransportConfig::StreamableHttp),
        }
    }
}

impl Default for McpTransportSettings {
    fn default() -> Self {
        Self::Stdio(McpStdioSettings::default())
    }
}

/// stdio MCP server transport.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpStdioSettings {
    /// Executable to spawn. It is passed directly, not through a shell.
    #[serde(default)]
    pub command: String,
    /// Command arguments. These are passed without shell interpolation.
    #[serde(default)]
    pub args: Vec<String>,
    /// Additional environment variables for the child process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Optional working directory. Relative paths are rooted under ANDA_HOME.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Whether the process gets the daemon's whole environment, which is the
    /// default. Off, it gets only the platform's essentials (PATH, HOME and
    /// the like) and `env`, so no other secret in the daemon's environment
    /// reaches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit_env: Option<bool>,
}

impl McpStdioSettings {
    fn setup_issues(&self, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        if self.command.trim().is_empty() {
            issues.push("command is empty".to_string());
        }
        push_expansion_issues(&self.command, "command", vars, issues);
        for (arg_index, arg) in self.args.iter().enumerate() {
            push_expansion_issues(arg, &format!("args[{arg_index}]"), vars, issues);
        }
        for (name, value) in &self.env {
            if name.trim().is_empty() {
                issues.push("env has an empty variable name".to_string());
            }
            push_expansion_issues(value, &format!("env.{name}"), vars, issues);
        }
        if let Some(cwd) = &self.cwd {
            push_expansion_issues(cwd, "cwd", vars, issues);
        }
    }

    fn to_transport(
        &self,
        vars: &McpExpansionVars,
        default_cwd: Option<&Path>,
    ) -> Result<McpStdioTransport, BoxError> {
        let cwd = match self.cwd.as_deref().and_then(normalize_string) {
            Some(cwd) => Some(resolve_config_path(
                &expand_config_string(&cwd, vars, "cwd")?,
                vars.home_dir,
            )),
            None => default_cwd.map(Path::to_path_buf),
        };
        let mut env = self
            .env
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.trim().to_string(),
                    expand_config_string(value, vars, &format!("env.{key}"))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, BoxError>>()?;
        // A daemon started by launchd or systemd has a minimal PATH, which
        // usually lacks `npx`, `uvx` and the like: give the child the same
        // tool directories the shell tool gets.
        if let Some(path) = command_path(env.get("PATH").map(String::as_str)) {
            env.insert("PATH".to_string(), path);
        }

        Ok(McpStdioTransport {
            command: expand_config_string(self.command.trim(), vars, "command")?,
            args: self
                .args
                .iter()
                .enumerate()
                .map(|(arg_index, arg)| {
                    expand_config_string(arg, vars, &format!("args[{arg_index}]"))
                })
                .collect::<Result<_, _>>()?,
            env,
            // On unless the entry turns it off, as before Anda had the
            // setting: `env` then holds overrides, not the whole environment.
            inherit_env: self.inherit_env.unwrap_or(true),
            cwd,
        })
    }
}

/// Streamable HTTP MCP server transport.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpStreamableHttpSettings {
    /// MCP endpoint URL.
    #[serde(default)]
    pub url: String,
    /// Bearer token value, without the `Bearer ` prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    /// Custom HTTP headers sent with every request.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Present when the server authorizes via the OAuth Authorization Code
    /// flow (onboarded through `connect_mcp_server`). Mutually exclusive with
    /// `bearer_token`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthSettings>,
}

/// OAuth Authorization Code marker for an HTTP MCP server.
///
/// Tokens are never stored in mcp.json: access and refresh tokens live in the
/// MCP credential store (`mcp_credentials/` under ANDA_HOME). This section only
/// records that the server authenticates via OAuth so the daemon reconnects it
/// from stored credentials after a restart; when those credentials are gone the
/// server needs a new interactive `connect_mcp_server` authorization.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpOAuthSettings {
    /// Pre-registered public client id. Omit to register dynamically (RFC 7591)
    /// during authorization; the issued id is persisted with the credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// OAuth scopes to request during (re-)authorization. Empty means the
    /// scopes the server advertises.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

/// Placeholder redirect URI stored in reconnect configs. Reconnection uses the
/// persisted credentials and never visits this URI; an interactive
/// (re-)authorization first replaces it with the gateway's callback URI
/// (`mcp_oauth::CALLBACK_PATH`).
pub const MCP_OAUTH_REDIRECT_PLACEHOLDER: &str = "http://127.0.0.1/callback";

impl McpOAuthSettings {
    /// Builds the engine-side authorization config for reconnecting from
    /// stored credentials.
    pub fn to_auth_config(&self) -> McpOAuthConfig {
        McpOAuthConfig::AuthorizationCode(OAuthAuthorizationCodeConfig {
            redirect_uri: MCP_OAUTH_REDIRECT_PLACEHOLDER.to_string(),
            scopes: self.scopes.clone(),
            client_name: None,
            client_id: self.client_id.clone(),
        })
    }
}

impl McpStreamableHttpSettings {
    fn setup_issues(&self, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        if self.url.trim().is_empty() {
            issues.push("url is empty".to_string());
        }
        push_expansion_issues(&self.url, "url", vars, issues);
        if let Some(token) = &self.bearer_token {
            push_expansion_issues(token, "bearer_token", vars, issues);
            if self.oauth.is_some() {
                issues.push("bearer_token cannot be combined with oauth".to_string());
            }
        }
        for (name, value) in &self.headers {
            let field = format!("headers.{name}");
            if HeaderName::from_bytes(name.as_bytes()).is_err() {
                issues.push(format!("{field} is not a valid header name"));
            }
            match expand_config_string(value, vars, &field) {
                Ok(expanded) => {
                    if HeaderValue::from_str(&expanded).is_err() {
                        issues.push(format!("{field} is not a valid header value"));
                    }
                }
                Err(err) => issues.push(err.to_string()),
            }
        }
    }

    fn to_transport(
        &self,
        vars: &McpExpansionVars,
    ) -> Result<McpStreamableHttpTransport, BoxError> {
        Ok(McpStreamableHttpTransport {
            url: expand_config_string(self.url.trim(), vars, "url")?,
            bearer_token: self
                .bearer_token
                .as_deref()
                .and_then(normalize_string)
                .map(|token| expand_config_string(&token, vars, "bearer_token"))
                .transpose()?,
            headers: self
                .headers
                .iter()
                .map(|(key, value)| {
                    HeaderName::from_bytes(key.as_bytes())?;
                    let value = expand_config_string(value, vars, &format!("headers.{key}"))?;
                    HeaderValue::from_str(&value)?;
                    Ok((key.clone(), value))
                })
                .collect::<Result<_, BoxError>>()?,
            auth: self.oauth.as_ref().map(McpOAuthSettings::to_auth_config),
        })
    }
}

struct McpExpansionVars<'a> {
    home_dir: &'a Path,
    default_cwd: Option<&'a Path>,
    /// `None` while validating: the store is not read then, so any secret
    /// name passes.
    secrets: Option<&'a McpSecretValues>,
    validate_only: bool,
}

impl<'a> McpExpansionVars<'a> {
    fn new(
        home_dir: &'a Path,
        default_cwd: Option<&'a Path>,
        secrets: &'a McpSecretValues,
    ) -> Self {
        Self {
            home_dir,
            default_cwd,
            secrets: Some(secrets),
            validate_only: false,
        }
    }

    fn validation() -> Self {
        Self {
            home_dir: Path::new(""),
            default_cwd: None,
            secrets: None,
            validate_only: true,
        }
    }

    fn secret(&self, name: &str) -> Option<String> {
        match self.secrets {
            Some(secrets) => secrets.get(name).cloned(),
            None => Some(String::new()),
        }
    }

    fn get(&self, name: &str) -> Option<String> {
        match name {
            // Validation runs before the paths are known.
            "ANDA_HOME" | "ANDA_WORKSPACE" if self.validate_only => Some(String::new()),
            "ANDA_HOME" => Some(self.home_dir.to_string_lossy().into_owned()),
            "ANDA_WORKSPACE" => self
                .default_cwd
                .map(|path| path.to_string_lossy().into_owned()),
            _ => std::env::var(name).ok(),
        }
    }
}

fn push_expansion_issues(
    value: &str,
    field: &str,
    vars: &McpExpansionVars<'_>,
    issues: &mut Vec<String>,
) {
    if let Err(err) = expand_config_string(value, vars, field) {
        issues.push(err.to_string());
    }
}

fn expand_config_string(
    value: &str,
    vars: &McpExpansionVars<'_>,
    field: &str,
) -> Result<String, BoxError> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(dollar) = rest.find('$') {
        out.push_str(&rest[..dollar]);
        let after = &rest[dollar + 1..];
        if let Some(braced) = after.strip_prefix('{') {
            let end = braced
                .find('}')
                .ok_or_else(|| format!("{field} contains an unterminated environment reference"))?;
            out.push_str(&expand_env_reference(&braced[..end], vars, field)?);
            rest = &braced[end + 1..];
        } else if after.starts_with(is_env_name_start) {
            let end = after
                .find(|c: char| !is_env_name_char(c))
                .unwrap_or(after.len());
            out.push_str(&expand_env_reference(&after[..end], vars, field)?);
            rest = &after[end..];
        } else {
            // A lone `$` is literal.
            out.push('$');
            rest = after;
        }
    }

    out.push_str(rest);
    Ok(out)
}

/// Expands one reference: `NAME` (an environment variable) or
/// `secret:NAME`, either one optionally followed by `:-default`, which is
/// used when the variable or secret is not set.
fn expand_env_reference(
    reference: &str,
    vars: &McpExpansionVars<'_>,
    field: &str,
) -> Result<String, BoxError> {
    let (name, default) = match reference.split_once(":-") {
        Some((name, default)) => (name, Some(default)),
        None => (reference, None),
    };
    if let Some(secret) = name.strip_prefix(SECRET_PREFIX) {
        if !is_secret_name(secret) {
            return Err(format!("{field} contains an invalid secret reference").into());
        }
        return vars
            .secret(secret)
            .or_else(|| default.map(str::to_string))
            .ok_or_else(|| {
                format!(
                    "{field} references secret {secret}, which is not set; set it in the MCP settings or with `anda mcp secret set {secret}`"
                )
                .into()
            });
    }
    if !is_secret_name(name) {
        return Err(format!("{field} contains an invalid environment reference").into());
    }
    vars.get(name)
        .or_else(|| default.map(str::to_string))
        .ok_or_else(|| format!("{field} references missing environment variable {name}").into())
}

const SECRET_PREFIX: &str = "secret:";

/// Whether `name` can name an environment variable or a secret.
pub fn is_secret_name(name: &str) -> bool {
    name.chars().next().is_some_and(is_env_name_start) && name.chars().all(is_env_name_char)
}

/// Adds the names of the `${secret:NAME}` references in `value` to `names`.
pub fn secret_references(value: &str, names: &mut BTreeSet<String>) {
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let braced = &rest[start + 2..];
        let Some(end) = braced.find('}') else {
            return;
        };
        let reference = &braced[..end];
        let name = reference
            .split_once(":-")
            .map_or(reference, |(name, _)| name);
        if let Some(secret) = name.strip_prefix(SECRET_PREFIX)
            && is_secret_name(secret)
        {
            names.insert(secret.to_string());
        }
        rest = &braced[end + 1..];
    }
}

fn is_env_name_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_env_name_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn resolve_config_path(path: &str, home_dir: &Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        home_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MCP_TEST_ENV_VARS: &[&str] = &["ANDA_MCP_TEST_TOKEN", "ANDA_MCP_TEST_PATH"];

    struct EnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new() -> Self {
            static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
            let lock = LOCK
                .get_or_init(|| std::sync::Mutex::new(()))
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let saved = MCP_TEST_ENV_VARS
                .iter()
                .map(|&name| (name, std::env::var_os(name)))
                .collect();
            for &name in MCP_TEST_ENV_VARS {
                unsafe { std::env::remove_var(name) };
            }
            Self { saved, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for &name in MCP_TEST_ENV_VARS {
                unsafe { std::env::remove_var(name) };
            }
            for (name, value) in &self.saved {
                if let Some(value) = value {
                    unsafe { std::env::set_var(name, value) };
                }
            }
        }
    }

    fn server_ids(settings: &McpSettings) -> Vec<&str> {
        settings
            .servers
            .iter()
            .map(|server| server.id.as_str())
            .collect()
    }

    fn diagnostic_for<'a>(settings: &'a McpSettings, id: &str) -> &'a McpDiagnostic {
        settings
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.server_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("no diagnostic for {id}: {:?}", settings.diagnostics))
    }

    #[test]
    fn mcp_json_oauth_section_round_trips_into_authorization_code_config() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "alink": {
                  "type": "http",
                  "url": "https://api.al.ink/mcp",
                  "oauth": {
                    "scopes": ["events:read", "handles:read"]
                  }
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.servers.len(), 1);
        assert!(settings.diagnostics.is_empty());
        let http = match &settings.servers[0].transport {
            McpTransportSettings::StreamableHttp(http) => http,
            _ => panic!("expected HTTP"),
        };
        let oauth = http.oauth.as_ref().expect("oauth section");
        assert_eq!(oauth.client_id, None);
        assert_eq!(oauth.scopes, vec!["events:read", "handles:read"]);

        let (configs, issues) =
            settings.server_configs(Path::new("/tmp/anda-home"), None, &McpSecretValues::new());
        assert!(issues.is_empty());
        match &configs[0].transport {
            McpTransportConfig::StreamableHttp(http) => match &http.auth {
                Some(McpOAuthConfig::AuthorizationCode(ac)) => {
                    assert_eq!(ac.redirect_uri, MCP_OAUTH_REDIRECT_PLACEHOLDER);
                    assert_eq!(ac.scopes, vec!["events:read", "handles:read"]);
                    assert_eq!(ac.client_id, None);
                }
                other => panic!("expected authorization_code auth, got {other:?}"),
            },
            _ => panic!("expected HTTP transport"),
        }
    }

    #[test]
    fn mcp_json_skips_bearer_token_combined_with_oauth() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "alink": {
                  "type": "http",
                  "url": "https://api.al.ink/mcp",
                  "bearer_token": "static-token",
                  "oauth": {}
                }
              }
            }"#,
        )
        .unwrap();

        assert!(settings.servers.is_empty());
        let diagnostic = diagnostic_for(&settings, "alink");
        assert!(
            diagnostic
                .message
                .contains("bearer_token cannot be combined with oauth"),
            "{diagnostic}"
        );
        // The skipped entry still owns its id.
        assert!(settings.declares("alink"));
    }

    #[test]
    fn mcp_json_accepts_mcp_servers_root_and_standard_http() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "github": {
                  "type": "stdio",
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-github"],
                  "environment": {
                    "GITHUB_TOKEN": "ghp_xxx"
                  }
                },
                "remote-server": {
                  "type": "http",
                  "url": "https://mcp.example.com/api",
                  "headers": {
                    "Authorization": "Bearer token"
                  }
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.servers.len(), 2);
        assert_eq!(settings.servers[0].id, "github");
        match &settings.servers[0].transport {
            McpTransportSettings::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(
                    stdio.env.get("GITHUB_TOKEN").map(String::as_str),
                    Some("ghp_xxx")
                );
            }
            _ => panic!("expected stdio"),
        }

        assert_eq!(settings.servers[1].id, "remote-server");
        match &settings.servers[1].transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.com/api");
                assert_eq!(
                    http.headers.get("Authorization").map(String::as_str),
                    Some("Bearer token")
                );
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn mcp_json_lifecycle_startup_and_tasks_reach_the_server_config() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "legacy": {
                  "type": "http",
                  "url": "https://legacy.example.com/mcp",
                  "lifecycle": "initialize",
                  "startup": "eager"
                },
                "slow": {
                  "type": "http",
                  "url": "https://slow.example.com/mcp",
                  "tasks": { "max_wait_secs": 900 }
                },
                "plain": {
                  "type": "http",
                  "url": "https://plain.example.com/mcp"
                }
              }
            }"#,
        )
        .unwrap();

        let (configs, issues) =
            settings.server_configs(Path::new("/tmp"), None, &McpSecretValues::new());
        assert!(issues.is_empty());
        let by_id = |id: &str| {
            configs
                .iter()
                .find(|config| config.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        assert_eq!(by_id("legacy").lifecycle, McpLifecycle::Initialize);
        assert_eq!(by_id("legacy").startup, McpStartup::Eager);
        assert!(by_id("legacy").tasks.is_none());
        assert_eq!(by_id("slow").tasks.as_ref().unwrap().max_wait_secs, 900);
        // An entry that says nothing probes the 2026-07-28 lifecycle, leaves
        // the tasks extension undeclared, and is discovered after startup so
        // it cannot hold the daemon back. None may fail the daemon.
        let plain = by_id("plain");
        assert_eq!(plain.lifecycle, McpLifecycle::Auto);
        assert!(plain.tasks.is_none());
        assert_eq!(plain.startup, McpStartup::Background);
        assert!(configs.iter().all(|config| !config.required));
    }

    #[test]
    fn mcp_json_skips_unknown_modes_by_name() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "broken": {
                  "type": "http",
                  "url": "https://example.com/mcp",
                  "lifecycle": "handshake"
                },
                "late": {
                  "type": "http",
                  "url": "https://example.com/mcp",
                  "startup": "lazy"
                },
                "fine": {
                  "type": "http",
                  "url": "https://example.com/mcp"
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(server_ids(&settings), ["fine"]);
        assert!(
            diagnostic_for(&settings, "broken")
                .message
                .contains("lifecycle has unsupported value \"handshake\"")
        );
        assert!(
            diagnostic_for(&settings, "late")
                .message
                .contains("startup has unsupported value \"lazy\"")
        );
    }

    #[test]
    fn mcp_json_reads_approval_policies_and_external_users() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "github": {
                  "type": "http",
                  "url": "https://example.com/mcp",
                  "approval": { "default": "Allow", "tools": { "merge_pull_request": "ask" } },
                  "allow_external_users": true
                },
                "plain": { "type": "http", "url": "https://example.com/mcp" },
                "typo": {
                  "type": "http",
                  "url": "https://example.com/mcp",
                  "approval": { "tools": { "delete": "never" } }
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(server_ids(&settings), ["github", "plain"]);
        let github = &settings.servers[0];
        assert!(github.allow_external_users);
        assert_eq!(github.approval.for_tool("create_issue"), McpApproval::Allow);
        assert_eq!(
            github.approval.for_tool("merge_pull_request"),
            McpApproval::Ask
        );
        let plain = &settings.servers[1];
        assert!(!plain.allow_external_users);
        assert_eq!(plain.approval.for_tool("anything"), McpApproval::Auto);
        assert!(
            diagnostic_for(&settings, "typo")
                .message
                .contains("approval.tools.delete has unsupported value \"never\"")
        );
        // A policy changes nothing about the connection.
        let home = Path::new("/tmp/anda-home");
        assert_eq!(
            serde_json::to_value(
                github
                    .server_config(home, None, &McpSecretValues::new())
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(
                McpServerSettings {
                    approval: McpApprovalSettings::default(),
                    allow_external_users: false,
                    ..github.clone()
                }
                .server_config(home, None, &McpSecretValues::new())
                .unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn mcp_json_accepts_servers_root_and_infers_transport() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "servers": {
                "filesystem": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-filesystem", "$ANDA_WORKSPACE"]
                },
                "remote": {
                  "url": "https://mcp.example.test/mcp",
                  "enabled": false
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.servers.len(), 2);
        assert_eq!(settings.servers[0].id, "filesystem");
        assert!(!settings.servers[0].disabled);
        assert_eq!(settings.servers[1].id, "remote");
        assert!(settings.servers[1].disabled);
        match &settings.servers[1].transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn mcp_json_explains_the_sse_transport() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "legacy": {
                  "type": "sse",
                  "url": "https://mcp.example.test/sse"
                }
              }
            }"#,
        )
        .unwrap();

        assert!(settings.servers.is_empty());
        let message = &diagnostic_for(&settings, "legacy").message;
        assert!(message.contains("SSE transport is deprecated"), "{message}");
    }

    #[test]
    fn a_broken_entry_does_not_cost_the_others() {
        let _env = EnvGuard::new();
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "good": { "command": "good-mcp" },
                "unset-env": {
                  "command": "gh-mcp",
                  "env": { "GITHUB_TOKEN": "${ANDA_MCP_TEST_TOKEN}" }
                },
                "bad-shape": { "command": "x", "args": "--not-a-list" },
                "no-transport": {},
                "": { "command": "nameless" },
                "off": { "command": "${ANDA_MCP_TEST_TOKEN}", "enabled": false }
              },
              "servers": {
                "good": { "url": "https://twice.example.test/mcp" }
              }
            }"#,
        )
        .unwrap();

        // The disabled entry is kept as written: nothing starts it.
        assert_eq!(server_ids(&settings), ["good", "off"]);
        assert!(
            diagnostic_for(&settings, "unset-env")
                .message
                .contains("env.GITHUB_TOKEN references missing environment variable"),
        );
        diagnostic_for(&settings, "bad-shape");
        assert!(
            diagnostic_for(&settings, "no-transport")
                .message
                .contains("neither command nor url")
        );
        assert!(
            diagnostic_for(&settings, "")
                .message
                .contains("id is empty")
        );
        let duplicates: Vec<_> = settings
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.server_id.as_deref() == Some("good"))
            .collect();
        assert_eq!(duplicates.len(), 1, "{duplicates:?}");
        assert!(duplicates[0].message.contains("declared more than once"));
        // Skipped entries still own their ids, so nothing is persisted over them.
        for id in ["good", "unset-env", "bad-shape", "no-transport", "off"] {
            assert!(settings.declares(id), "{id}");
        }
        assert!(!settings.declares("missing"));
    }

    #[test]
    fn mcp_json_reports_a_root_of_the_wrong_shape() {
        let settings = McpSettings::from_json_contents(
            r#"{ "mcpServers": 5, "servers": { "ok": { "command": "ok-mcp" } } }"#,
        )
        .unwrap();

        assert_eq!(server_ids(&settings), ["ok"]);
        assert_eq!(settings.diagnostics.len(), 1);
        assert_eq!(settings.diagnostics[0].server_id, None);
        assert!(settings.diagnostics[0].message.contains("mcpServers"));

        assert!(McpSettings::from_json_contents("[]").is_err());
        assert!(McpSettings::from_json_contents("{").is_err());
        assert!(
            McpSettings::from_json_contents(" \n")
                .unwrap()
                .servers
                .is_empty()
        );
    }

    #[test]
    fn mcp_json_legacy_lists_report_entries_by_id() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "servers": [
                { "id": "ok", "transport": { "type": "stdio", "command": "ok-mcp" } },
                { "id": "bad", "transport": { "type": "carrier-pigeon" } },
                { "transport": 7 }
              ]
            }"#,
        )
        .unwrap();

        assert_eq!(server_ids(&settings), ["ok"]);
        diagnostic_for(&settings, "bad");
        diagnostic_for(&settings, "servers[2]");
    }

    #[test]
    fn file_contents_never_fail_to_load() {
        let path = Path::new("/tmp/anda-home/mcp.json");
        let empty = McpSettings::from_file_contents(path, "");
        assert!(empty.servers.is_empty() && empty.diagnostics.is_empty());

        let unparsable = McpSettings::from_file_contents(path, "{ not json");
        assert!(unparsable.servers.is_empty());
        assert_eq!(unparsable.diagnostics.len(), 1);
        assert_eq!(unparsable.diagnostics[0].server_id, None);
        assert!(
            unparsable.diagnostics[0].message.contains("was not loaded"),
            "{}",
            unparsable.diagnostics[0]
        );
    }

    #[test]
    fn mcp_server_configs_expand_env_and_default_cwd() {
        let _env = EnvGuard::new();
        unsafe { std::env::set_var("ANDA_MCP_TEST_TOKEN", "token-1") };
        unsafe { std::env::set_var("ANDA_MCP_TEST_PATH", "project-a") };

        let settings = McpSettings {
            servers: vec![
                McpServerSettings {
                    id: "fs".to_string(),
                    transport: McpTransportSettings::Stdio(McpStdioSettings {
                        command: "npx".to_string(),
                        args: vec![
                            "-y".to_string(),
                            "@modelcontextprotocol/server-filesystem".to_string(),
                            "$ANDA_HOME/$ANDA_MCP_TEST_PATH".to_string(),
                        ],
                        env: BTreeMap::from([(
                            "TOKEN".to_string(),
                            "${ANDA_MCP_TEST_TOKEN}".to_string(),
                        )]),
                        cwd: None,
                        inherit_env: None,
                    }),
                    ..Default::default()
                },
                McpServerSettings {
                    id: "remote".to_string(),
                    transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                        url: "https://mcp.example.test/mcp".to_string(),
                        bearer_token: Some("$ANDA_MCP_TEST_TOKEN".to_string()),
                        headers: BTreeMap::from([(
                            "x-anda-home".to_string(),
                            "$ANDA_HOME".to_string(),
                        )]),
                        oauth: None,
                    }),
                    include: BTreeSet::from(["search".to_string()]),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert!(
            settings
                .servers
                .iter()
                .all(|server| server.setup_issues().is_empty())
        );

        let home = Path::new("/tmp/anda-home");
        let workspace = home.join("workspace");
        let (servers, issues) =
            settings.server_configs(home, Some(&workspace), &McpSecretValues::new());
        assert!(issues.is_empty());
        assert_eq!(servers.len(), 2);

        match &servers[0].transport {
            McpTransportConfig::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(stdio.args[2], "/tmp/anda-home/project-a");
                assert_eq!(stdio.env.get("TOKEN").map(String::as_str), Some("token-1"));
                assert!(stdio.inherit_env);
                assert_eq!(stdio.cwd.as_deref(), Some(workspace.as_path()));
            }
            _ => panic!("expected stdio transport"),
        }

        match &servers[1].transport {
            McpTransportConfig::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
                assert_eq!(http.bearer_token.as_deref(), Some("token-1"));
                assert_eq!(
                    http.headers.get("x-anda-home").map(String::as_str),
                    Some("/tmp/anda-home")
                );
            }
            _ => panic!("expected streamable HTTP transport"),
        }
        assert_eq!(servers[1].include, BTreeSet::from(["search".to_string()]));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn stdio_children_get_the_shell_tool_path() {
        let server = |env: BTreeMap<String, String>| McpServerSettings {
            id: "fs".to_string(),
            transport: McpTransportSettings::Stdio(McpStdioSettings {
                command: "npx".to_string(),
                env,
                ..Default::default()
            }),
            ..Default::default()
        };
        let path_of = |server: McpServerSettings| match server
            .server_config(Path::new("/tmp/anda-home"), None, &McpSecretValues::new())
            .unwrap()
            .transport
        {
            McpTransportConfig::Stdio(stdio) => stdio.env["PATH"].clone(),
            _ => panic!("expected stdio transport"),
        };

        let inherited = path_of(server(BTreeMap::new()));
        assert!(std::env::split_paths(&inherited).any(|dir| dir == Path::new("/usr/bin")));

        // An explicit PATH keeps its order and only gains the tool directories.
        let explicit = path_of(server(BTreeMap::from([(
            "PATH".to_string(),
            "/custom/bin".to_string(),
        )])));
        let dirs: Vec<_> = std::env::split_paths(&explicit).collect();
        assert_eq!(dirs[0], Path::new("/custom/bin"));
        assert!(dirs.iter().any(|dir| dir == Path::new("/usr/bin")));
    }

    #[test]
    fn expand_config_string_keeps_literal_dollars() {
        let _env = EnvGuard::new();
        unsafe { std::env::set_var("ANDA_MCP_TEST_TOKEN", "t") };
        let secrets = McpSecretValues::from([("GITHUB_PAT".to_string(), "ghp_x".to_string())]);
        let vars = McpExpansionVars::new(Path::new("/h"), None, &secrets);
        let expand = |value: &str| expand_config_string(value, &vars, "field");

        assert_eq!(
            expand("a$ é$1 $$ANDA_MCP_TEST_TOKEN-${ANDA_MCP_TEST_TOKEN}$").unwrap(),
            "a$ é$1 $t-t$"
        );
        assert_eq!(expand("$ANDA_HOME/x").unwrap(), "/h/x");
        assert!(expand("${ANDA_MCP_TEST_TOKEN").is_err());
        assert!(expand("${}").is_err());
        assert!(expand("$ANDA_WORKSPACE").is_err());

        let validation = McpExpansionVars::validation();
        assert_eq!(
            expand_config_string("$ANDA_HOME:$ANDA_WORKSPACE", &validation, "field").unwrap(),
            ":"
        );
    }

    #[test]
    fn one_server_receives_the_webhooks_of_event_automations() {
        let settings = McpSettings::from_json_contents(
            &serde_json::json!({"mcpServers": {
                "dmsg": {"command": "dmsg", "args": ["mcp"], "events": {"webhook_ingress": true}},
                "relay2": {"command": "relay", "events": {"webhook_ingress": true}},
                "docs": {"command": "docs-mcp"}
            }})
            .to_string(),
        )
        .unwrap();
        let ingress: Vec<_> = settings
            .servers
            .iter()
            .filter(|server| server.events.as_ref().is_some_and(|e| e.webhook_ingress))
            .map(|server| server.id.as_str())
            .collect();
        // Both stay usable servers; the second is told it is not the ingress.
        assert_eq!(ingress, ["dmsg", "relay2"]);
        let message = &diagnostic_for(&settings, "relay2").message;
        assert!(message.contains("dmsg receives the webhooks"), "{message}");
        assert!(!message.contains("skipped"), "{message}");
        let dmsg = settings.servers.iter().find(|s| s.id == "dmsg").unwrap();
        assert_eq!(
            crate::engine::mcp::config_store::entry_json(dmsg)["events"],
            serde_json::json!({"webhook_ingress": true})
        );
    }

    #[test]
    fn advanced_settings_parse_check_and_reach_the_engine_config() {
        let server = McpSettings::parse_entry(
            "docs",
            &serde_json::json!({
                "command": "docs-mcp",
                "inherit_env": false,
                "timeouts": { "call_secs": 900 },
                "concurrency": "read-only-parallel",
                "limits": { "output_text_bytes": 4096 }
            }),
        )
        .unwrap();
        assert!(server.setup_issues().is_empty());
        let config = server
            .server_config(Path::new("/anda"), None, &McpSecretValues::new())
            .unwrap();
        assert_eq!(
            (config.timeouts.call_secs, config.timeouts.setup_secs),
            (900, 90)
        );
        assert_eq!(config.concurrency, McpConcurrency::ReadOnlyParallel);
        assert_eq!(config.limits.output_text_bytes, 4096);
        let McpTransportConfig::Stdio(stdio) = config.transport else {
            panic!("docs runs a command");
        };
        assert!(!stdio.inherit_env);

        // Left out, they keep the engine's defaults and the whole environment.
        let plain = McpSettings::parse_entry("x", &serde_json::json!({ "command": "x" }))
            .unwrap()
            .server_config(Path::new("/anda"), None, &McpSecretValues::new())
            .unwrap();
        assert_eq!(plain.concurrency, McpConcurrency::Serial);
        assert_eq!(
            plain.limits.output_text_bytes,
            McpLimits::default().output_text_bytes
        );
        assert!(matches!(plain.transport, McpTransportConfig::Stdio(stdio) if stdio.inherit_env));

        for (entry, message) in [
            (
                serde_json::json!({ "command": "x", "concurrency": "many" }),
                "concurrency has unsupported value",
            ),
            (
                serde_json::json!({ "url": "https://x.test/mcp", "inherit_env": true }),
                "only for local",
            ),
        ] {
            let err = McpSettings::parse_entry("x", &entry).unwrap_err();
            assert!(err.to_string().contains(message), "{err}");
        }
        for (entry, issue) in [
            (
                serde_json::json!({ "command": "x", "timeouts": { "setup_secs": 0 } }),
                "timeouts.setup_secs must be between 1 and 86400 seconds",
            ),
            (
                serde_json::json!({ "command": "x", "timeouts": { "call_secs": 86401 } }),
                "timeouts.call_secs must be between 1 and 86400 seconds",
            ),
            (
                serde_json::json!({ "command": "x", "limits": { "output_text_bytes": 100 } }),
                "limits.output_text_bytes must be between 256 and 1048576",
            ),
        ] {
            let issues = McpSettings::parse_entry("x", &entry)
                .unwrap()
                .setup_issues();
            assert_eq!(issues, [issue]);
        }
    }

    #[test]
    fn secrets_and_defaults_expand_and_missing_secrets_say_how_to_set_them() {
        let _env = EnvGuard::new();
        let secrets = McpSecretValues::from([("GITHUB_PAT".to_string(), "ghp_x".to_string())]);
        let vars = McpExpansionVars::new(Path::new("/h"), None, &secrets);
        let expand = |value: &str| expand_config_string(value, &vars, "headers.Authorization");

        assert_eq!(
            expand("Bearer ${secret:GITHUB_PAT}").unwrap(),
            "Bearer ghp_x"
        );
        assert_eq!(
            expand("${ANDA_MCP_TEST_TOKEN:-fallback}").unwrap(),
            "fallback"
        );
        assert_eq!(expand("${ANDA_MCP_TEST_TOKEN:-}").unwrap(), "");
        assert_eq!(expand("${secret:OTHER:-none}").unwrap(), "none");
        let missing = expand("${secret:LINEAR_KEY}").unwrap_err().to_string();
        assert!(
            missing.contains("headers.Authorization references secret LINEAR_KEY")
                && missing.contains("anda mcp secret set LINEAR_KEY"),
            "{missing}"
        );
        assert!(expand("${secret:bad-name}").is_err());
        assert!(expand("${secret:}").is_err());

        // Validation does not read the store: any well-formed name passes.
        let validation = McpExpansionVars::validation();
        assert!(expand_config_string("${secret:LINEAR_KEY}", &validation, "f").is_ok());

        let server = McpSettings::parse_entry(
            "github",
            &serde_json::json!({
                "url": "https://api.test/${secret:HOST:-mcp}",
                "headers": {
                    "Authorization": "Bearer ${secret:GITHUB_PAT}",
                    "X-Env": "${ANDA_MCP_TEST_TOKEN}"
                }
            }),
        )
        .unwrap();
        assert_eq!(
            server.secret_names().into_iter().collect::<Vec<_>>(),
            ["GITHUB_PAT", "HOST"]
        );
    }

    #[test]
    fn mcp_setup_issues_name_the_mcp_json_fields() {
        let _env = EnvGuard::new();
        let stdio = McpServerSettings {
            transport: McpTransportSettings::Stdio(McpStdioSettings {
                command: "${ANDA_MCP_TEST_TOKEN}".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            stdio.setup_issues(),
            [
                "id is empty",
                "command references missing environment variable ANDA_MCP_TEST_TOKEN",
            ]
        );

        let http = McpServerSettings {
            id: "remote".to_string(),
            transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                url: String::new(),
                headers: BTreeMap::from([("bad header".to_string(), "ok".to_string())]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            http.setup_issues(),
            [
                "url is empty",
                "headers.bad header is not a valid header name"
            ]
        );
    }

    #[test]
    fn mcp_disabled_servers_are_ignored() {
        let settings = McpSettings {
            servers: vec![McpServerSettings {
                disabled: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        let (configs, issues) =
            settings.server_configs(Path::new("/tmp/anda-home"), None, &McpSecretValues::new());
        assert!(configs.is_empty() && issues.is_empty());
    }
}
