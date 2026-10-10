//! `anda mcp`: manage the MCP servers in mcp.json.
//!
//! With the daemon running, every command goes through its MCP API, so a
//! change applies at once. Without it, the commands that only edit mcp.json
//! (and `logout`, which deletes a stored sign-in) work on the files directly
//! and take effect when the daemon starts; the others need the daemon.

use anda_core::BoxError;
use anda_engine::extension::mcp::McpCredentialStore;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio::sync::Mutex;

use crate::{
    config::{McpApproval, McpSettings},
    daemon::Daemon,
    engine::mcp::{
        FileMcpCredentialStore, MCP_CREDENTIALS_DIR_NAME,
        config_store::{self, McpFileEdit},
        offline_snapshot, open_in_browser,
    },
    gateway,
};

/// How long `login` waits for the browser sign-in to finish.
const LOGIN_WAIT: Duration = Duration::from_secs(600);
const LOGIN_POLL: Duration = Duration::from_secs(2);

#[derive(Args)]
pub struct McpCommand {
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: McpSubcommand,
}

#[derive(Subcommand)]
enum McpSubcommand {
    /// List the configured servers and their state.
    List,
    /// Show one server and its tools.
    Get { id: String },
    /// Add a server to mcp.json: a local command after `--`, or a remote `--url`.
    ///
    ///   anda mcp add context7 -- npx -y @upstash/context7-mcp
    ///   anda mcp add github --url https://api.githubcopilot.com/mcp/ --header 'Authorization: Bearer ${GITHUB_PAT}'
    #[command(verbatim_doc_comment)]
    Add {
        id: String,
        /// The remote MCP endpoint (Streamable HTTP).
        #[arg(long)]
        url: Option<String>,
        /// An HTTP header, as `Name: value`. Repeatable.
        #[arg(long = "header", value_name = "NAME: VALUE")]
        headers: Vec<String>,
        /// An environment variable for a local server, as `KEY=VALUE`. Repeatable.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// The working directory of a local server.
        #[arg(long)]
        cwd: Option<String>,
        /// Save the server without starting it.
        #[arg(long)]
        disabled: bool,
        /// The local command and its arguments, after `--`.
        #[arg(last = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// Add a server from an mcp.json entry, as other MCP clients write one.
    ///
    ///   anda mcp add-json docs '{"type":"http","url":"https://docs.example.com/mcp"}'
    #[command(verbatim_doc_comment)]
    AddJson {
        id: String,
        #[arg(value_name = "JSON")]
        entry: String,
    },
    /// Remove a server from mcp.json, with its stored sign-in.
    Remove {
        id: String,
        /// Keep the stored OAuth sign-in.
        #[arg(long)]
        keep_credentials: bool,
    },
    /// Start a disabled server.
    Enable { id: String },
    /// Stop a server, keeping its mcp.json entry.
    Disable { id: String },
    /// Connect a server again, or every failed one.
    Reconnect { id: Option<String> },
    /// Sign in to an HTTP server that uses OAuth.
    Login {
        id: String,
        /// The endpoint, to sign in to a server that is not configured yet.
        #[arg(long)]
        url: Option<String>,
        /// OAuth scopes to request, comma separated. Defaults to the server's.
        #[arg(long, value_delimiter = ',')]
        scopes: Vec<String>,
        /// Sign out first and ask for consent again.
        #[arg(long)]
        reauthorize: bool,
        /// Print the sign-in URL instead of opening a browser.
        #[arg(long)]
        no_browser: bool,
        /// The address the browser ended on, when it could not reach the daemon.
        #[arg(long)]
        redirect_url: Option<String>,
    },
    /// Delete a server's stored OAuth sign-in.
    Logout { id: String },
    /// List a server's tools.
    Tools { id: String },
    /// Hide one of a server's tools from the agent, or show it again.
    #[command(group(clap::ArgGroup::new("visibility").required(true).args(["hide", "show"])))]
    Tool {
        id: String,
        tool: String,
        #[arg(long)]
        hide: bool,
        #[arg(long)]
        show: bool,
    },
    /// Set when the agent asks before calling a server's tools.
    ///
    ///   anda mcp approval github allow                  every tool of github
    ///   anda mcp approval github ask --tool merge_pull_request
    ///   anda mcp approval github inherit --tool create_issue
    ///
    /// `auto` asks unless the session has full access or the tool is
    /// read-only and unchanged since review; `ask` always asks; `allow` never
    /// asks while the tool is unchanged since review. `inherit` clears it.
    #[command(verbatim_doc_comment)]
    Approval {
        id: String,
        #[arg(value_enum)]
        approval: ApprovalArg,
        /// One tool, by the server's name for it, instead of the server.
        #[arg(long)]
        tool: Option<String>,
    },
    /// Let runs for external IM users call a server's tools, or not (the default).
    ExternalUsers {
        id: String,
        #[arg(value_enum)]
        setting: Switch,
    },
    /// Accept the current definitions of a server's tools as reviewed: all
    /// of them, or the ones named.
    Review { id: String, tools: Vec<String> },
    /// Show what changed in a tool since it was reviewed.
    Diff { id: String, tool: String },
    /// Apply mcp.json after editing it by hand.
    Reload,
}

#[derive(Clone, Copy, ValueEnum)]
enum ApprovalArg {
    Auto,
    Ask,
    Allow,
    /// Clear it: a tool follows the server, the server falls back to auto.
    Inherit,
}

impl ApprovalArg {
    fn policy(self) -> Option<McpApproval> {
        match self {
            Self::Auto => Some(McpApproval::Auto),
            Self::Ask => Some(McpApproval::Ask),
            Self::Allow => Some(McpApproval::Allow),
            Self::Inherit => None,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    On,
    Off,
}

impl McpCommand {
    /// Whether the command talks to the daemon, so needs an owner client.
    /// Offline edits do not: they only touch files in ANDA_HOME.
    pub fn needs_client(&self, running: bool) -> bool {
        running || matches!(self.command, McpSubcommand::Login { .. })
    }
}

/// Runs `cmd`. `client` is present when [`McpCommand::needs_client`] said so.
pub async fn run(
    daemon: &Daemon,
    client: Option<&gateway::Client>,
    cmd: McpCommand,
) -> Result<(), BoxError> {
    let running = daemon.running_pid().await?.is_some();
    // Present when the daemon is running: changes go through it then.
    let live = client.filter(|_| running);
    let json = cmd.json;
    match cmd.command {
        McpSubcommand::List => {
            let snapshot = match live {
                Some(client) => client.mcp("mcp_list", json!({})).await?,
                None => json!(offline_snapshot(&daemon.home).await),
            };
            if json {
                print_json(&snapshot)?;
            } else {
                print_list(&snapshot);
            }
        }
        McpSubcommand::Get { id } => {
            let detail = match live {
                Some(client) => client.mcp("mcp_get", json!({ "id": id })).await?,
                None => offline_server(&daemon.home, &id).await?,
            };
            if json {
                print_json(&detail)?;
            } else {
                print_detail(&detail);
            }
        }
        McpSubcommand::Add {
            id,
            url,
            headers,
            env,
            cwd,
            disabled,
            command,
        } => {
            let entry = entry_from_flags(url, headers, env, cwd, disabled, command)?;
            add(daemon, live, json, id, entry).await?;
        }
        McpSubcommand::AddJson { id, entry } => {
            let entry: Value =
                serde_json::from_str(&entry).map_err(|err| format!("invalid JSON: {err}"))?;
            if !entry.is_object() {
                return Err("the JSON must be one mcp.json entry, an object".into());
            }
            add(daemon, live, json, id, entry).await?;
        }
        McpSubcommand::Remove {
            id,
            keep_credentials,
        } => {
            let change = json!({ "op": "remove", "id": id, "keep_credentials": keep_credentials });
            if let Some(client) = live {
                report(
                    json,
                    &apply(client, change).await?,
                    &format!("Removed {id}."),
                )?;
            } else {
                edit_offline(&daemon.home, McpFileEdit::Remove(&id)).await?;
                if !keep_credentials {
                    credential_store(&daemon.home).clear(&id).await?;
                }
                report_offline(json, &format!("Removed {id} from mcp.json."))?;
            }
        }
        McpSubcommand::Enable { id } => set_enabled(daemon, live, json, id, true).await?,
        McpSubcommand::Disable { id } => set_enabled(daemon, live, json, id, false).await?,
        McpSubcommand::Reconnect { id } => {
            let receipt = require_running(live)?
                .mcp("mcp_reconnect", json!({ "id": id }))
                .await?;
            report(json, &receipt, "Nothing to reconnect.")?;
        }
        McpSubcommand::Login {
            id,
            url,
            scopes,
            reauthorize,
            no_browser,
            redirect_url,
        } => {
            let client = client.ok_or("an owner client is needed to sign in")?;
            if !running {
                client.ensure_daemon_running(daemon).await?;
            }
            let params = json!({
                "id": id,
                "url": url,
                "scopes": scopes,
                "reauthorize": reauthorize,
                "redirect_url": redirect_url,
            });
            let result: Value = client.mcp("mcp_sign_in", params).await?;
            login(client, json, &id, result, no_browser).await?;
        }
        McpSubcommand::Logout { id } => {
            if let Some(client) = live {
                let result: Value = client.mcp("mcp_sign_out", json!({ "id": id })).await?;
                report(json, &result, &format!("Signed out of {id}."))?;
            } else {
                credential_store(&daemon.home).clear(&id).await?;
                report_offline(json, &format!("Deleted the stored sign-in of {id}."))?;
            }
        }
        McpSubcommand::Tools { id } => {
            let detail: Value = require_running(live)?
                .mcp("mcp_get", json!({ "id": id }))
                .await?;
            if json {
                print_json(&detail["tools"])?;
            } else {
                print_tools(&detail);
            }
        }
        McpSubcommand::Tool {
            id,
            tool,
            hide,
            show: _,
        } => {
            let visible = !hide;
            let done = format!("{} {tool} on {id}.", if visible { "Showed" } else { "Hid" });
            if let Some(client) = live {
                let change =
                    json!({ "op": "set_tool_visible", "id": id, "tool": tool, "visible": visible });
                report(json, &apply(client, change).await?, &done)?;
            } else {
                edit_offline(
                    &daemon.home,
                    McpFileEdit::SetToolVisible {
                        id: &id,
                        tool: &tool,
                        visible,
                    },
                )
                .await?;
                report_offline(json, &done)?;
            }
        }
        McpSubcommand::Approval { id, approval, tool } => {
            let policy = approval.policy();
            let target = match &tool {
                Some(tool) => format!("{tool} on {id}"),
                None => format!("the tools of {id}"),
            };
            let done = match policy {
                Some(policy) => format!("Approval for {target} is {}.", policy.as_str()),
                None => format!("Cleared the approval policy of {target}."),
            };
            if let Some(client) = live {
                let change = json!({
                    "op": "set_approval", "id": id, "tool": tool, "approval": policy,
                });
                report(json, &apply(client, change).await?, &done)?;
            } else {
                edit_offline(
                    &daemon.home,
                    McpFileEdit::SetApproval {
                        id: &id,
                        tool: tool.as_deref(),
                        approval: policy,
                    },
                )
                .await?;
                report_offline(json, &done)?;
            }
        }
        McpSubcommand::ExternalUsers { id, setting } => {
            let allowed = matches!(setting, Switch::On);
            let done = if allowed {
                format!("External IM users' runs may call the tools of {id}.")
            } else {
                format!("External IM users' runs may not call the tools of {id}.")
            };
            if let Some(client) = live {
                let change = json!({ "op": "set_external_users", "id": id, "allowed": allowed });
                report(json, &apply(client, change).await?, &done)?;
            } else {
                edit_offline(&daemon.home, McpFileEdit::SetExternalUsers(&id, allowed)).await?;
                report_offline(json, &done)?;
            }
        }
        McpSubcommand::Review { id, tools } => {
            let change = json!({ "op": "mark_reviewed", "id": id, "tools": tools });
            let receipt = apply(require_running(live)?, change).await?;
            if json {
                print_json(&receipt)?;
            } else {
                let reviewed = receipt["reviewed"].as_array().map_or(0, Vec::len);
                println!("Reviewed {reviewed} tools of {id}.");
            }
        }
        McpSubcommand::Diff { id, tool } => {
            let diff: Value = require_running(live)?
                .mcp("mcp_tool_diff", json!({ "id": id, "tool": tool }))
                .await?;
            if json {
                print_json(&diff)?;
            } else {
                print_diff(&diff);
            }
        }
        McpSubcommand::Reload => {
            let Some(client) = live else {
                return Err(
                    "The daemon is not running; it reads mcp.json when it starts (`anda start`)."
                        .into(),
                );
            };
            let receipt = client.mcp("mcp_reload", json!({})).await?;
            report(json, &receipt, "mcp.json is applied; nothing changed.")?;
        }
    }
    Ok(())
}

async fn add(
    daemon: &Daemon,
    live: Option<&gateway::Client>,
    json: bool,
    id: String,
    entry: Value,
) -> Result<(), BoxError> {
    if let Some(client) = live {
        let mut server = entry;
        server["id"] = json!(id);
        let change = json!({ "op": "add", "server": server, "persist": true });
        let receipt = apply(client, change).await?;
        report(
            json,
            &receipt,
            &format!("Added {id} to mcp.json. See how it connects with `anda mcp get {id}`."),
        )?;
        return Ok(());
    }
    let server =
        McpSettings::parse_entry(&id, &entry).map_err(|err| format!("MCP server {id}: {err}"))?;
    // Checked against this shell's environment, which is not the daemon's:
    // said, but not refused.
    let issues = server.setup_issues();
    edit_offline(&daemon.home, McpFileEdit::Add(&server)).await?;
    report_offline(json, &format!("Added {id} to mcp.json."))?;
    if !json && !issues.is_empty() {
        println!("Note: {}", issues.join("; "));
    }
    Ok(())
}

async fn set_enabled(
    daemon: &Daemon,
    live: Option<&gateway::Client>,
    json: bool,
    id: String,
    enabled: bool,
) -> Result<(), BoxError> {
    let done = format!("{} {id}.", if enabled { "Enabled" } else { "Disabled" });
    if let Some(client) = live {
        let change = json!({ "op": "set_enabled", "id": id, "enabled": enabled });
        report(json, &apply(client, change).await?, &done)
    } else {
        edit_offline(&daemon.home, McpFileEdit::SetEnabled(&id, enabled)).await?;
        report_offline(json, &done)
    }
}

async fn apply(client: &gateway::Client, change: Value) -> Result<Value, BoxError> {
    client.mcp("mcp_apply", json!({ "change": change })).await
}

async fn edit_offline(home: &Path, edit: McpFileEdit<'_>) -> Result<(), BoxError> {
    // The daemon is not running, so nothing else writes the file now.
    config_store::edit(&McpSettings::file_path(home), &Mutex::new(()), None, edit).await?;
    Ok(())
}

fn credential_store(home: &Path) -> FileMcpCredentialStore {
    FileMcpCredentialStore::new(home.join(MCP_CREDENTIALS_DIR_NAME))
}

/// One server from mcp.json, without a daemon to say how it is doing.
async fn offline_server(home: &Path, id: &str) -> Result<Value, BoxError> {
    let snapshot = json!(offline_snapshot(home).await);
    let server = snapshot["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|server| server["id"] == id)
        .cloned()
        .ok_or_else(|| format!("MCP server {id} is not configured"))?;
    // Which tools it offers is known only once it connects; the hidden ones
    // are in mcp.json.
    let hidden: Vec<Value> = server["settings"]["exclude"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| json!({ "remote_name": tool, "hidden": true }))
        .collect();
    let mut server = server;
    server["tools"] = json!(hidden);
    Ok(server)
}

fn require_running(live: Option<&gateway::Client>) -> Result<&gateway::Client, BoxError> {
    live.ok_or_else(|| "The daemon is not running; start it with `anda start`.".into())
}

/// Builds an mcp.json entry from `anda mcp add` flags.
fn entry_from_flags(
    url: Option<String>,
    headers: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    disabled: bool,
    command: Vec<String>,
) -> Result<Value, BoxError> {
    let mut entry = Map::new();
    match (url, command.split_first()) {
        (Some(url), None) => {
            if !env.is_empty() || cwd.is_some() {
                return Err("--env and --cwd are for local servers".into());
            }
            entry.insert("type".into(), json!("http"));
            entry.insert("url".into(), json!(url));
            let mut values = BTreeMap::new();
            for header in headers {
                let (name, value) = header
                    .split_once(':')
                    .ok_or_else(|| format!("--header {header:?} must be `Name: value`"))?;
                values.insert(name.trim().to_string(), value.trim().to_string());
            }
            if !values.is_empty() {
                entry.insert("headers".into(), json!(values));
            }
        }
        (None, Some((program, args))) => {
            if !headers.is_empty() {
                return Err("--header is for remote servers".into());
            }
            entry.insert("type".into(), json!("stdio"));
            entry.insert("command".into(), json!(program));
            if !args.is_empty() {
                entry.insert("args".into(), json!(args));
            }
            let mut values = BTreeMap::new();
            for pair in env {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--env {pair:?} must be `KEY=VALUE`"))?;
                values.insert(key.to_string(), value.to_string());
            }
            if !values.is_empty() {
                entry.insert("env".into(), json!(values));
            }
            if let Some(cwd) = cwd {
                entry.insert("cwd".into(), json!(cwd));
            }
        }
        (Some(_), Some(_)) => {
            return Err("give either --url or a command after --, not both".into());
        }
        (None, None) => {
            return Err(
                "give a remote --url, or a local command after -- (e.g. `-- npx -y pkg`)".into(),
            );
        }
    }
    if disabled {
        entry.insert("enabled".into(), json!(false));
    }
    Ok(Value::Object(entry))
}

async fn login(
    client: &gateway::Client,
    json: bool,
    id: &str,
    result: Value,
    no_browser: bool,
) -> Result<(), BoxError> {
    if result["status"] != "authorization_required" {
        if json {
            return print_json(&result);
        }
        let tools = result["tools"].as_array().map_or(0, Vec::len);
        println!("{id} is connected, with {tools} tools.");
        return Ok(());
    }

    let auth_url = result["authorization_url"].as_str().unwrap_or_default();
    let redirect_uri = result["redirect_uri"].as_str().unwrap_or_default();
    if !no_browser && open_in_browser(auth_url).await.is_ok() {
        eprintln!("Opened the browser to sign in to {id}.");
    } else {
        eprintln!("Open this address in a browser to sign in to {id}:\n\n  {auth_url}\n");
    }
    eprintln!(
        "Waiting for the sign-in to finish (Ctrl+C stops waiting; the sign-in still completes).\n\
         The browser returns to {redirect_uri}. If it cannot reach this daemon, copy the address \
         it ends on and run:\n  anda mcp login {id} --redirect-url '<address>'"
    );

    let deadline = tokio::time::Instant::now() + LOGIN_WAIT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(LOGIN_POLL).await;
        let Ok(detail) = client.mcp::<Value>("mcp_get", json!({ "id": id })).await else {
            continue;
        };
        if detail["status"] == "ready" {
            if json {
                return print_json(&detail);
            }
            let tools = detail["tools"].as_array().map_or(0, Vec::len);
            println!("{id} is connected, with {tools} tools.");
            return Ok(());
        }
    }
    Err(format!("{id} did not finish signing in; run `anda mcp login {id}` to try again").into())
}

fn report(json: bool, result: &Value, nothing: &str) -> Result<(), BoxError> {
    if json {
        return print_json(result);
    }
    let names = |key: &str| -> Option<String> {
        let list = result[key].as_array()?;
        (!list.is_empty()).then(|| {
            list.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    let mut said = false;
    for (key, label) in [
        ("added", "Started"),
        ("rebuilt", "Restarted"),
        ("removed", "Stopped"),
        ("connected", "Connected"),
    ] {
        if let Some(names) = names(key) {
            println!("{label}: {names}");
            said = true;
        }
    }
    for failure in result["failed"].as_array().into_iter().flatten() {
        println!(
            "Failed: {}: {}",
            failure["id"].as_str().unwrap_or_default(),
            failure["message"].as_str().unwrap_or_default()
        );
        said = true;
    }
    for warning in result["warnings"].as_array().into_iter().flatten() {
        println!("Warning: {}", warning.as_str().unwrap_or_default());
    }
    if !said {
        println!("{nothing}");
    }
    Ok(())
}

fn report_offline(json: bool, done: &str) -> Result<(), BoxError> {
    if json {
        return print_json(&json!({ "applied": false }));
    }
    println!("{done} The daemon is not running; this takes effect when it starts.");
    Ok(())
}

fn print_json(value: &Value) -> Result<(), BoxError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn print_list(snapshot: &Value) {
    if snapshot["running"] == false {
        println!(
            "The daemon is not running; showing mcp.json, checked against this shell's environment."
        );
    } else if snapshot["config_changed_on_disk"] == true {
        println!("mcp.json changed on disk; `anda mcp reload` applies it.");
    }
    for diagnostic in snapshot["diagnostics"].as_array().into_iter().flatten() {
        println!("! {}", diagnostic.as_str().unwrap_or_default());
    }
    let servers = snapshot["servers"].as_array().cloned().unwrap_or_default();
    if servers.is_empty() {
        println!(
            "No MCP servers in {}. Add one with `anda mcp add`.",
            text(snapshot, "config_path")
        );
        return;
    }
    let width = |key: &str, header: &str| {
        servers
            .iter()
            .map(|server| text(server, key).len())
            .chain([header.len()])
            .max()
            .unwrap_or_default()
    };
    let (id_width, status_width) = (width("id", "ID"), width("status", "STATUS"));
    println!(
        "{:id_width$}  {:status_width$}  {:>5}  {:9}  SUMMARY",
        "ID", "STATUS", "TOOLS", "TRANSPORT"
    );
    for server in &servers {
        println!(
            "{:id_width$}  {:status_width$}  {:>5}  {:9}  {}",
            text(server, "id"),
            text(server, "status"),
            server["tools"]["total"].as_u64().unwrap_or_default(),
            text(server, "transport"),
            text(server, "summary"),
        );
        if let Some(error) = server["last_error"]["message"].as_str() {
            println!("{:id_width$}  └ {error}", "");
        }
        for diagnostic in server["diagnostics"].as_array().into_iter().flatten() {
            println!(
                "{:id_width$}  └ {}",
                "",
                diagnostic.as_str().unwrap_or_default()
            );
        }
    }
}

fn print_detail(detail: &Value) {
    let id = text(detail, "id");
    match detail["title"].as_str() {
        Some(title) => println!("{id} — {title} ({})", text(detail, "status")),
        None => println!("{id} ({})", text(detail, "status")),
    }
    println!(
        "  {} {}",
        text(detail, "transport"),
        text(detail, "summary")
    );
    println!(
        "  {} · {} · source: {} · startup: {} · auth: {}",
        if detail["enabled"] == true {
            "enabled"
        } else {
            "disabled"
        },
        if detail["persisted"] == true {
            "in mcp.json"
        } else {
            "this daemon only"
        },
        text(detail, "source"),
        text(detail, "startup"),
        text(detail, "auth"),
    );
    println!(
        "  approval: {} · external IM users: {}",
        text(detail, "approval"),
        if detail["allow_external_users"] == true {
            "allowed"
        } else {
            "not allowed"
        },
    );
    if let Some(calls) = detail["usage"]["calls"].as_u64() {
        println!(
            "  {calls} calls, {} failed",
            detail["usage"]["errors"].as_u64().unwrap_or_default()
        );
    }
    if let Some(error) = detail["last_error"]["message"].as_str() {
        println!("  last error: {error}");
    }
    for diagnostic in detail["diagnostics"].as_array().into_iter().flatten() {
        println!("  ! {}", diagnostic.as_str().unwrap_or_default());
    }
    print_tools(detail);
}

fn print_tools(detail: &Value) {
    let tools = detail["tools"].as_array().cloned().unwrap_or_default();
    if tools.is_empty() {
        println!("  no tools");
        return;
    }
    let width = tools
        .iter()
        .map(|tool| text(tool, "remote_name").len())
        .max()
        .unwrap_or_default();
    println!("  tools:");
    for tool in &tools {
        // `auto` is the default: only a deliberate policy is worth a mark.
        let approval = match text(tool, "approval") {
            "" | "auto" => String::new(),
            approval => format!("  [{approval}]"),
        };
        if tool["hidden"] == true {
            println!(
                "    {:width$}  (hidden){approval}",
                text(tool, "remote_name")
            );
        } else {
            let review = match text(tool, "review") {
                "new" => "  (new: not reviewed)",
                "changed" => "  (changed since review)",
                _ => "",
            };
            println!(
                "    {:width$}  {}{approval}{review}",
                text(tool, "remote_name"),
                tool["title"].as_str().unwrap_or_else(|| text(tool, "name"))
            );
        }
    }
    if tools
        .iter()
        .any(|tool| matches!(text(tool, "review"), "new" | "changed"))
    {
        println!(
            "  Review with `anda mcp diff {id} <tool>`, then accept with `anda mcp review {id}`.",
            id = text(detail, "id")
        );
    }
}

fn print_diff(diff: &Value) {
    let (id, tool) = (text(diff, "server_id"), text(diff, "tool"));
    match text(diff, "review") {
        "trusted" => {
            println!("{tool} on {id} is unchanged since it was reviewed.");
            return;
        }
        "new" => println!("{tool} on {id} is new since the server was reviewed."),
        _ => println!("{tool} on {id} changed since it was reviewed."),
    }
    for change in diff["changes"].as_array().into_iter().flatten() {
        let show = |value: &Value| match value {
            Value::Null => "(none)".to_string(),
            Value::String(text) => text.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_default(),
        };
        println!("\n{}:", text(change, "field"));
        if !change["before"].is_null() {
            println!("- {}", show(&change["before"]).replace('\n', "\n- "));
        }
        println!("+ {}", show(&change["after"]).replace('\n', "\n+ "));
    }
    println!("\nAccept it with `anda mcp review {id} {tool}`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_flags_build_mcp_json_entries() {
        let entry = entry_from_flags(
            None,
            Vec::new(),
            vec!["TOKEN=${CONTEXT7_KEY}".into()],
            Some("work".into()),
            false,
            vec!["npx".into(), "-y".into(), "@upstash/context7-mcp".into()],
        )
        .unwrap();
        assert_eq!(
            entry,
            json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@upstash/context7-mcp"],
                "env": { "TOKEN": "${CONTEXT7_KEY}" },
                "cwd": "work"
            })
        );

        let entry = entry_from_flags(
            Some("https://gh.test/mcp".into()),
            vec!["Authorization: Bearer ${GITHUB_PAT}".into()],
            Vec::new(),
            None,
            true,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(entry["headers"]["Authorization"], "Bearer ${GITHUB_PAT}");
        assert_eq!(entry["enabled"], false);
        McpSettings::parse_entry("github", &entry).unwrap();

        for (url, command) in [
            (Some("https://x.test".to_string()), vec!["x".to_string()]),
            (None, Vec::new()),
        ] {
            assert!(entry_from_flags(url, Vec::new(), Vec::new(), None, false, command).is_err());
        }
    }

    #[tokio::test]
    async fn offline_edits_change_mcp_json_and_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let entry = entry_from_flags(
            None,
            Vec::new(),
            Vec::new(),
            None,
            false,
            vec!["docs-mcp".into()],
        )
        .unwrap();
        let server = McpSettings::parse_entry("docs", &entry).unwrap();
        edit_offline(home, McpFileEdit::Add(&server)).await.unwrap();
        edit_offline(home, McpFileEdit::SetEnabled("docs", false))
            .await
            .unwrap();
        edit_offline(
            home,
            McpFileEdit::SetToolVisible {
                id: "docs",
                tool: "delete",
                visible: false,
            },
        )
        .await
        .unwrap();

        edit_offline(
            home,
            McpFileEdit::SetApproval {
                id: "docs",
                tool: None,
                approval: ApprovalArg::Allow.policy(),
            },
        )
        .await
        .unwrap();
        edit_offline(home, McpFileEdit::SetExternalUsers("docs", true))
            .await
            .unwrap();

        let server = offline_server(home, "docs").await.unwrap();
        assert_eq!(server["status"], "disabled");
        assert_eq!(server["persisted"], true);
        assert_eq!(server["settings"]["exclude"], json!(["delete"]));
        assert_eq!(server["approval"], "allow");
        assert_eq!(server["allow_external_users"], true);
        assert!(ApprovalArg::Inherit.policy().is_none());

        edit_offline(home, McpFileEdit::Remove("docs"))
            .await
            .unwrap();
        let err = offline_server(home, "docs").await.unwrap_err();
        assert!(err.to_string().contains("not configured"), "{err}");
    }
}
