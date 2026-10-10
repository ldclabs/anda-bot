//! What the owner API, the CLI and `manage_mcp_server` show about servers.
//!
//! Views are built from mcp.json as written, so an entry that was skipped as
//! invalid is listed with the reason rather than missing. Building one never
//! starts a connection, and every secret is redacted.

use anda_engine::extension::mcp::{McpServerConfig, McpServerStatus, McpStartup, McpToolRoute};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use super::{
    config_store::{self, McpConfigFile},
    redact::{display_url, redact_args, redact_entry},
    review::{self, McpReview},
    state::{MCP_STATE_FILE_NAME, McpErrorRecord, McpSource, McpStateStore, McpUsage},
};
use crate::config::{
    McpApproval, McpServerOptions, McpServerSettings, McpSettings, McpTransportSettings,
};

/// A server's state as one word.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpStatus {
    /// `enabled: false` in mcp.json.
    Disabled,
    /// The entry cannot be used; `diagnostics` says why.
    Invalid,
    Connecting,
    Ready,
    /// Waiting for the user to sign in.
    NeedsAuth,
    /// The last connection attempt failed; it is retried with backoff.
    Failed,
    /// Not connected now; the next call or reconnect connects it.
    Disconnected,
    /// The daemon is not running.
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpToolCounts {
    /// Tools the model can use.
    pub total: usize,
    /// Tools hidden through `exclude`.
    pub hidden: usize,
    /// Tools that are new or changed since the server was reviewed.
    pub needs_review: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpServerView {
    pub id: String,
    /// The server's own name and description, once it has connected. Both
    /// come from the server, so they are untrusted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `stdio`, `http`, or `unknown` for an entry that does not parse.
    pub transport: &'static str,
    /// The command line or URL, redacted.
    pub summary: String,
    pub enabled: bool,
    /// Whether it is in mcp.json; otherwise it was added for this daemon only.
    pub persisted: bool,
    pub startup: McpStartup,
    pub source: McpSource,
    /// The file an imported server came from, or the Registry entry of an
    /// installed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    pub status: McpStatus,
    /// The approval policy of tools that have none of their own.
    pub approval: McpApproval,
    /// Whether runs for external IM users may call its tools.
    pub allow_external_users: bool,
    /// `none`, `bearer`, `headers` or `oauth`.
    pub auth: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<McpErrorRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ready_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    pub tools: McpToolCounts,
    /// The server's instructions differ from the reviewed ones.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub instructions_changed: bool,
    #[serde(skip_serializing_if = "McpUsage::is_empty")]
    pub usage: McpUsage,
    /// The mcp.json entry, redacted.
    pub settings: Value,
    /// The advanced settings, for an entry that parses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<McpServerOptions>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct McpToolHints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpToolView {
    /// The name the model calls it by. Hidden tools have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The server's name for it.
    pub remote_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Hints from the server. They are untrusted and grant nothing.
    pub annotations: McpToolHints,
    pub hidden: bool,
    /// The approval policy that applies: its own, else the server's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<McpApproval>,
    /// Whether its definition is the reviewed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<McpReview>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpServerDetail {
    #[serde(flatten)]
    pub server: McpServerView,
    /// What the server tells the model about itself. Untrusted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The reviewed instructions, when they differ from `instructions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_instructions: Option<String>,
    pub tools: Vec<McpToolView>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpSnapshot {
    pub config_path: String,
    /// The revision of mcp.json the servers were applied from.
    pub revision: String,
    /// mcp.json was changed on disk since; `reload` applies the change.
    pub config_changed_on_disk: bool,
    pub running: bool,
    /// Problems with mcp.json as a whole.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    pub servers: Vec<McpServerView>,
}

/// The running daemon's side of a view.
pub(super) struct LiveState {
    pub statuses: BTreeMap<String, McpServerStatus>,
    pub registered: BTreeSet<String>,
    /// When a failed server is retried next.
    pub retries: BTreeMap<String, u64>,
    /// Servers waiting for an authorization to finish.
    pub pending: Vec<McpServerConfig>,
    pub routes: Vec<McpToolRoute>,
    /// Title, description and instructions by server, once connected.
    pub meta: BTreeMap<String, ServerMeta>,
}

#[derive(Default)]
pub(super) struct ServerMeta {
    pub title: Option<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
}

/// What views are built from.
pub(super) struct ViewSource<'a> {
    pub root: &'a Value,
    pub settings: &'a McpSettings,
    /// Enabled entries whose config could not be built, with the reason.
    pub build_errors: &'a BTreeMap<String, String>,
    /// Servers added for this daemon only.
    pub runtime: Vec<&'a McpServerSettings>,
    pub state: &'a McpStateStore,
    /// Absent when the daemon is not running.
    pub live: Option<&'a LiveState>,
}

impl ViewSource<'_> {
    pub fn snapshot(&self, config_path: &Path, revision: String, changed: bool) -> McpSnapshot {
        let servers = self.server_views();
        let shown: BTreeSet<&str> = servers.iter().map(|server| server.id.as_str()).collect();
        // Problems that no listed server can carry: the file's own, and
        // entries without a usable id.
        let diagnostics = self
            .settings
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic
                    .server_id
                    .as_deref()
                    .is_none_or(|id| !shown.contains(id.trim()))
            })
            .map(|diagnostic| diagnostic.message.clone())
            .collect();
        McpSnapshot {
            config_path: config_path.display().to_string(),
            revision,
            config_changed_on_disk: changed,
            running: self.live.is_some(),
            diagnostics,
            servers,
        }
    }

    pub fn server_views(&self) -> Vec<McpServerView> {
        let mut views = Vec::new();
        let mut seen = BTreeSet::new();
        for (id, raw) in config_store::raw_entries(self.root) {
            if !seen.insert(id.clone()) {
                continue;
            }
            let declared = || self.settings.servers.iter().filter(|s| s.id.trim() == id);
            // The entry that runs, when the id is declared more than once.
            let settings = declared()
                .find(|server| !server.disabled)
                .or_else(|| declared().next());
            views.push(self.server_view(&id, Some(raw), settings));
        }
        for server in &self.runtime {
            if seen.insert(server.id.clone()) {
                views.push(self.server_view(&server.id, None, Some(server)));
            }
        }
        if let Some(live) = self.live {
            for config in &live.pending {
                if seen.insert(config.id.clone()) {
                    views.push(self.pending_view(config));
                }
            }
        }
        views
    }

    pub fn detail(&self, view: McpServerView) -> McpServerDetail {
        let id = view.id.as_str();
        let settings = self.settings_of(id);
        let approval =
            |tool: &str| settings.map_or_else(McpApproval::default, |s| s.approval.for_tool(tool));
        let reviews = self.reviews(id);
        let mut tools: Vec<McpToolView> = self
            .live
            .into_iter()
            .flat_map(|live| &live.routes)
            .filter(|route| route.server_id == id)
            .map(|route| McpToolView {
                approval: Some(approval(&route.remote_name)),
                review: reviews.get(&route.remote_name).copied(),
                ..tool_view(route)
            })
            .collect();
        tools.sort_by(|a, b| a.remote_name.cmp(&b.remote_name));
        let hidden = settings
            .map(|server| server.exclude.clone())
            .unwrap_or_default();
        tools.extend(hidden.into_iter().map(|remote_name| McpToolView {
            name: None,
            approval: Some(approval(&remote_name)),
            remote_name,
            title: None,
            description: None,
            annotations: McpToolHints::default(),
            hidden: true,
            review: None,
        }));
        let instructions = self
            .live
            .and_then(|live| live.meta.get(id))
            .and_then(|meta| meta.instructions.clone());
        let reviewed_instructions = if view.instructions_changed {
            self.state
                .read(id, |state| state?.instructions.as_ref()?.text.clone())
        } else {
            None
        };
        McpServerDetail {
            server: view,
            instructions,
            reviewed_instructions,
            tools,
        }
    }

    /// The settings of `id`: its enabled mcp.json entry, else any entry or
    /// runtime server of that id.
    fn settings_of(&self, id: &str) -> Option<&McpServerSettings> {
        let declared = || {
            self.settings
                .servers
                .iter()
                .chain(self.runtime.iter().copied())
                .filter(move |server| server.id.trim() == id)
        };
        declared()
            .find(|server| !server.disabled)
            .or_else(|| declared().next())
    }

    /// How each tool `id` offers compares with its reviewed definition. A
    /// server not pinned yet is trusted as it is: it will be on first use.
    fn reviews(&self, id: &str) -> BTreeMap<String, McpReview> {
        let Some(live) = self.live else {
            return BTreeMap::new();
        };
        let routes = live.routes.iter().filter(|route| route.server_id == id);
        self.state.read(id, |state| {
            let pinned = state.filter(|state| state.reviewed_at.is_some());
            routes
                .map(|route| {
                    let review = match pinned {
                        None => McpReview::Trusted,
                        Some(state) => review::review(
                            state.tools.get(&route.remote_name),
                            &review::digest(&review::tool_definition(&route.tool)),
                        ),
                    };
                    (route.remote_name.clone(), review)
                })
                .collect()
        })
    }

    fn server_view(
        &self,
        id: &str,
        raw: Option<&Value>,
        settings: Option<&McpServerSettings>,
    ) -> McpServerView {
        let state = self.state.get(id);
        let build_error = self.build_errors.get(id);
        let mut diagnostics: Vec<String> = self
            .settings
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.server_id.as_deref().map(str::trim) == Some(id))
            .map(|diagnostic| diagnostic.message.clone())
            .collect();
        diagnostics.extend(build_error.cloned());

        let status = match (settings, self.live) {
            (None, _) => McpStatus::Invalid,
            (Some(server), _) if server.disabled => McpStatus::Disabled,
            (Some(_), _) if build_error.is_some() => McpStatus::Invalid,
            (Some(_), None) => McpStatus::Unknown,
            (Some(_), Some(live)) if live.pending.iter().any(|config| config.id == id) => {
                McpStatus::NeedsAuth
            }
            (Some(_), Some(live)) if live.registered.contains(id) => live
                .statuses
                .get(id)
                .copied()
                .map(live_status)
                .unwrap_or(McpStatus::Disconnected),
            // Declared but not registered: the engine refused it, and
            // `last_error` says why.
            (Some(_), Some(_)) => McpStatus::Failed,
        };

        let (transport, summary, auth) = match settings {
            Some(server) => describe(&server.transport),
            None => raw
                .map(describe_raw)
                .unwrap_or(("unknown", String::new(), "none")),
        };
        let enabled = match settings {
            Some(server) => !server.disabled,
            None => raw.is_some_and(|raw| {
                raw.get("enabled") != Some(&json!(false))
                    && raw.get("disabled") != Some(&json!(true))
            }),
        };
        let entry = match (raw, settings) {
            (Some(raw), _) => redact_entry(raw),
            (None, Some(server)) => redact_entry(&Value::Object(config_store::entry_json(server))),
            (None, None) => json!({}),
        };
        let meta = self.live.and_then(|live| live.meta.get(id));
        let instructions_changed = meta.is_some_and(|meta| {
            state
                .instructions
                .as_ref()
                .is_some_and(|pin| pin.text != meta.instructions)
        });
        let needs_review = self
            .reviews(id)
            .values()
            .filter(|review| **review != McpReview::Trusted)
            .count();
        McpServerView {
            id: id.to_string(),
            title: meta.and_then(|meta| meta.title.clone()),
            description: meta.and_then(|meta| meta.description.clone()),
            transport,
            summary,
            enabled,
            persisted: raw.is_some(),
            startup: settings
                .and_then(|server| server.startup)
                .unwrap_or(McpStartup::Background),
            source: state.source,
            source_ref: state.source_ref.clone(),
            status,
            approval: settings
                .and_then(|server| server.approval.default)
                .unwrap_or_default(),
            allow_external_users: settings.is_some_and(|server| server.allow_external_users),
            auth,
            last_error: state.last_error.filter(|_| status != McpStatus::Ready),
            last_ready_at: state.last_ready_at,
            next_retry_at: self
                .live
                .filter(|_| status == McpStatus::Failed)
                .and_then(|live| live.retries.get(id).copied()),
            diagnostics,
            tools: McpToolCounts {
                total: self.live.map_or(0, |live| {
                    live.routes
                        .iter()
                        .filter(|route| route.server_id == id)
                        .count()
                }),
                hidden: match settings {
                    Some(server) => server.exclude.len(),
                    None => raw
                        .and_then(|raw| raw.get("exclude"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len),
                },
                needs_review,
            },
            instructions_changed,
            usage: state.usage,
            settings: entry,
            options: settings.map(McpServerSettings::options),
        }
    }

    /// A server that exists only as an authorization in progress.
    fn pending_view(&self, config: &McpServerConfig) -> McpServerView {
        let state = self.state.get(&config.id);
        let summary = match &config.transport {
            anda_engine::extension::mcp::McpTransportConfig::StreamableHttp(http) => {
                display_url(&http.url)
            }
            _ => String::new(),
        };
        McpServerView {
            id: config.id.clone(),
            title: None,
            description: None,
            transport: "http",
            summary: summary.clone(),
            enabled: true,
            persisted: false,
            startup: McpStartup::Background,
            source: state.source,
            source_ref: state.source_ref.clone(),
            status: McpStatus::NeedsAuth,
            approval: McpApproval::default(),
            allow_external_users: false,
            auth: "oauth",
            last_error: None,
            last_ready_at: None,
            next_retry_at: None,
            diagnostics: Vec::new(),
            tools: McpToolCounts {
                total: 0,
                hidden: 0,
                needs_review: 0,
            },
            instructions_changed: false,
            usage: state.usage,
            settings: json!({ "type": "http", "url": summary }),
            options: None,
        }
    }
}

/// Reads the configured servers without a running daemon, for the CLI.
pub(crate) async fn offline_snapshot(home_dir: &Path) -> McpSnapshot {
    let path = McpSettings::file_path(home_dir);
    let (root, settings, revision) = match McpConfigFile::read(&path).await {
        Ok(file) => (
            file.root(),
            McpSettings::from_file_contents(&path, file.text()),
            file.revision,
        ),
        Err(err) => (
            json!({}),
            McpSettings::unreadable(&path, err),
            String::new(),
        ),
    };
    let state = McpStateStore::open(home_dir.join(MCP_STATE_FILE_NAME)).await;
    let source = ViewSource {
        root: &root,
        settings: &settings,
        build_errors: &BTreeMap::new(),
        runtime: Vec::new(),
        state: &state,
        live: None,
    };
    source.snapshot(&path, revision, false)
}

fn live_status(status: McpServerStatus) -> McpStatus {
    match status {
        McpServerStatus::Disconnected => McpStatus::Disconnected,
        McpServerStatus::Connecting | McpServerStatus::Connected => McpStatus::Connecting,
        McpServerStatus::Ready => McpStatus::Ready,
        McpServerStatus::AuthorizationRequired => McpStatus::NeedsAuth,
        McpServerStatus::Failed => McpStatus::Failed,
    }
}

/// Transport, redacted summary and auth kind of a parsed entry.
pub(super) fn describe(transport: &McpTransportSettings) -> (&'static str, String, &'static str) {
    match transport {
        McpTransportSettings::Stdio(stdio) => {
            ("stdio", command_line(&stdio.command, &stdio.args), "none")
        }
        McpTransportSettings::StreamableHttp(http) => {
            let auth = if http.oauth.is_some() {
                "oauth"
            } else if http.bearer_token.is_some()
                || http
                    .headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization"))
            {
                "bearer"
            } else if !http.headers.is_empty() {
                "headers"
            } else {
                "none"
            };
            ("http", display_url(&http.url), auth)
        }
    }
}

/// The same for an entry that did not parse, read as far as it goes.
fn describe_raw(raw: &Value) -> (&'static str, String, &'static str) {
    let raw = raw
        .get("transport")
        .filter(|t| t.is_object())
        .unwrap_or(raw);
    let text = |key: &str| raw.get(key).and_then(Value::as_str);
    if let Some(command) = text("command") {
        let args: Vec<String> = raw
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        ("stdio", command_line(command, &args), "none")
    } else if let Some(url) = text("url") {
        let headers = raw.get("headers").and_then(Value::as_object);
        let auth = if raw.get("oauth").is_some() {
            "oauth"
        } else if raw.get("bearer_token").is_some()
            || headers.is_some_and(|headers| {
                headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization"))
            })
        {
            "bearer"
        } else if headers.is_some_and(|headers| !headers.is_empty()) {
            "headers"
        } else {
            "none"
        };
        ("http", display_url(url), auth)
    } else {
        ("unknown", String::new(), "none")
    }
}

fn command_line(command: &str, args: &[String]) -> String {
    std::iter::once(command.to_string())
        .chain(redact_args(args))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn tool_view(route: &McpToolRoute) -> McpToolView {
    let annotations = route.tool.annotations.as_ref();
    McpToolView {
        name: Some(route.name.clone()),
        remote_name: route.remote_name.clone(),
        title: route
            .tool
            .title
            .clone()
            .or_else(|| annotations.and_then(|hints| hints.title.clone())),
        description: route.tool.description.as_deref().map(str::to_string),
        annotations: McpToolHints {
            read_only: annotations.and_then(|hints| hints.read_only_hint),
            destructive: annotations.and_then(|hints| hints.destructive_hint),
            idempotent: annotations.and_then(|hints| hints.idempotent_hint),
            open_world: annotations.and_then(|hints| hints.open_world_hint),
        },
        hidden: false,
        approval: None,
        review: None,
    }
}
