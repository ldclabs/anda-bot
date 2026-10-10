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
use std::{
    collections::BTreeMap,
    io::IsTerminal,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;

use crate::{
    config::{McpApproval, McpServerOptions, McpSettings},
    daemon::Daemon,
    engine::mcp::{
        FileMcpCredentialStore, MCP_CREDENTIALS_DIR_NAME, MCP_SECRETS_FILE_NAME,
        MCP_STATE_FILE_NAME, McpOrigin, McpSecretStore, McpSource, McpStateStore,
        config_store::{self, McpConfigFile, McpFileEdit},
        import::{
            self, McpImportContext, McpImportRequest, McpImportScan, McpImportSource,
            McpImportTarget, McpKnownServer,
        },
        offline_snapshot, open_in_browser, orphaned_secrets, secret_views, secrets_in_use,
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
    /// of them and the server's instructions, or the tools named.
    Review { id: String, tools: Vec<String> },
    /// Show what changed in a tool since it was reviewed.
    Diff { id: String, tool: String },
    /// Manage the secrets that mcp.json references as `${secret:NAME}`.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Show a server's advanced settings, or change them. A flag given
    /// `default` clears that setting.
    ///
    ///   anda mcp options github --call-timeout 900 --concurrency read-only-parallel
    ///   anda mcp options context7 --inherit-env off
    #[command(verbatim_doc_comment)]
    Options {
        id: String,
        #[command(flatten)]
        flags: OptionFlags,
    },
    /// Import servers from Claude Desktop, Claude Code, Cursor, VS Code,
    /// Windsurf or Codex. Their files are only read. Plaintext tokens move to
    /// secrets, and local servers run without the daemon's whole environment.
    ///
    ///   anda mcp import --dry-run                what each server would become
    ///   anda mcp import                          every server new to Anda
    ///   anda mcp import github notes=work-notes --from cursor
    #[command(verbatim_doc_comment)]
    Import {
        /// Servers to import, by their name there; `NAME=ID` imports one
        /// under another id. Default: every server new to Anda.
        servers: Vec<String>,
        /// Read only this client. Repeatable.
        #[arg(long = "from", value_enum)]
        sources: Vec<McpImportSource>,
        /// A project directory to read besides the current one. Repeatable.
        #[arg(long = "workspace", value_name = "DIR")]
        workspaces: Vec<PathBuf>,
        /// Show what would be imported, without importing it.
        #[arg(long)]
        dry_run: bool,
        /// Leave plaintext tokens in mcp.json instead of moving them to secrets.
        #[arg(long)]
        keep_plaintext: bool,
    },
    /// Apply mcp.json after editing it by hand.
    Reload,
    /// List the events a server can report (MCP Events) and the automations
    /// that run on them.
    Events { id: String },
    /// Manage MCP event automations: agent runs on a server's events, with
    /// results in a conversation of your own. The daemon must be running.
    ///
    ///   anda mcp triggers                                   every automation
    ///   anda mcp triggers add github issue.opened --args '{"repo":"o/r"}' \
    ///       --instructions "Label each new issue"
    ///   anda mcp triggers get 3                             with its latest runs
    ///   anda mcp triggers pause 3 | resume 3 | delete 3
    #[command(verbatim_doc_comment)]
    Triggers {
        #[command(subcommand)]
        action: Option<TriggerAction>,
    },
}

#[derive(Subcommand)]
enum TriggerAction {
    /// List the automations.
    List,
    /// Show one automation with its latest runs and events.
    Get { id: u64 },
    /// Create an automation.
    Add {
        server: String,
        event: String,
        /// What to do with the events.
        #[arg(long)]
        instructions: String,
        /// Subscription arguments, a JSON object.
        #[arg(long = "args", value_name = "JSON")]
        arguments: Option<String>,
        #[arg(long)]
        name: Option<String>,
        /// Seconds to collect events into one run (default 30).
        #[arg(long = "batch-window", value_name = "SECS")]
        batch_window_secs: Option<u64>,
        /// Runs within an hour after which it pauses itself (default 12).
        #[arg(long)]
        max_runs_per_hour: Option<u64>,
        /// auto (default), push, poll or webhook.
        #[arg(long)]
        delivery: Option<String>,
    },
    /// Stop an automation; it keeps its settings.
    Pause { id: u64 },
    /// Start a paused or ended automation again.
    Resume { id: u64 },
    /// Delete an automation and its waiting events.
    Delete { id: u64 },
}

/// The settings `anda mcp options` changes; each takes `default` to clear it.
#[derive(Args, Default)]
struct OptionFlags {
    /// When the server's tools are discovered: background (default) or eager.
    #[arg(long, value_name = "MODE")]
    startup: Option<String>,
    /// How the protocol is negotiated: auto (default), discover or initialize.
    #[arg(long, value_name = "MODE")]
    lifecycle: Option<String>,
    /// Which tools may run at once: serial (default), read-only-parallel or parallel.
    #[arg(long, value_name = "MODE")]
    concurrency: Option<String>,
    /// Seconds to connect (default 90).
    #[arg(long, value_name = "SECS")]
    setup_timeout: Option<String>,
    /// Seconds to list the tools (default 30).
    #[arg(long, value_name = "SECS")]
    list_timeout: Option<String>,
    /// Seconds for one request within a call (default 180).
    #[arg(long, value_name = "SECS")]
    request_timeout: Option<String>,
    /// Seconds for a whole tool call (default 600).
    #[arg(long, value_name = "SECS")]
    call_timeout: Option<String>,
    /// Bytes of text the agent gets from one result (default 32768).
    #[arg(long, value_name = "BYTES")]
    output_limit: Option<String>,
    /// A local server's environment: on (default) gives it the daemon's
    /// whole environment, off only the essentials and its own env.
    #[arg(long, value_name = "on|off")]
    inherit_env: Option<String>,
    /// Long-running tasks: off (default), on, or the longest wait in seconds.
    #[arg(long, value_name = "on|off|SECS")]
    tasks: Option<String>,
}

impl OptionFlags {
    fn is_empty(&self) -> bool {
        [
            &self.startup,
            &self.lifecycle,
            &self.concurrency,
            &self.setup_timeout,
            &self.list_timeout,
            &self.request_timeout,
            &self.call_timeout,
            &self.output_limit,
            &self.inherit_env,
            &self.tasks,
        ]
        .iter()
        .all(|flag| flag.is_none())
    }

    /// Applies the flags to `options`, the server's settings as the API
    /// shows them.
    fn apply(&self, options: &mut Value) -> Result<McpServerOptions, BoxError> {
        fn set(options: &mut Value, path: &[&str], value: Option<Value>) {
            let (last, parents) = path.split_last().expect("a path");
            let mut object = options;
            for key in parents {
                if !object[*key].is_object() {
                    object[*key] = json!({});
                }
                object = &mut object[*key];
            }
            match value {
                Some(value) => object[*last] = value,
                None => {
                    if let Some(map) = object.as_object_mut() {
                        map.remove(*last);
                    }
                }
            }
        }
        let number = |flag: &str, value: &str| -> Result<Value, BoxError> {
            value
                .parse::<u64>()
                .map(|n| json!(n))
                .map_err(|_| format!("--{flag} takes a number or `default`, not {value:?}").into())
        };
        if !options.is_object() {
            *options = json!({});
        }
        for (key, value) in [
            ("startup", &self.startup),
            ("lifecycle", &self.lifecycle),
            ("concurrency", &self.concurrency),
        ] {
            if let Some(value) = value {
                let value = value.trim().to_ascii_lowercase().replace('-', "_");
                set(options, &[key], (value != "default").then(|| json!(value)));
            }
        }
        for (flag, path, value) in [
            (
                "setup-timeout",
                ["timeouts", "setup_secs"],
                &self.setup_timeout,
            ),
            (
                "list-timeout",
                ["timeouts", "list_secs"],
                &self.list_timeout,
            ),
            (
                "request-timeout",
                ["timeouts", "request_secs"],
                &self.request_timeout,
            ),
            (
                "call-timeout",
                ["timeouts", "call_secs"],
                &self.call_timeout,
            ),
            (
                "output-limit",
                ["limits", "output_text_bytes"],
                &self.output_limit,
            ),
        ] {
            if let Some(value) = value.as_deref().map(str::trim) {
                let value = match value {
                    "default" => None,
                    value => Some(number(flag, value)?),
                };
                set(options, &path, value);
            }
        }
        if let Some(value) = self.inherit_env.as_deref() {
            let value = match value.trim() {
                "on" | "true" => Some(json!(true)),
                "off" | "false" => Some(json!(false)),
                "default" => None,
                other => {
                    return Err(
                        format!("--inherit-env takes on, off or default, not {other:?}").into(),
                    );
                }
            };
            set(options, &["inherit_env"], value);
        }
        if let Some(value) = self.tasks.as_deref() {
            let value = match value.trim() {
                "off" | "default" => None,
                "on" => Some(json!({})),
                secs => Some(json!({ "max_wait_secs": number("tasks", secs)? })),
            };
            set(options, &["tasks"], value);
        }
        for key in ["timeouts", "limits"] {
            if options[key].as_object().is_some_and(Map::is_empty) {
                options.as_object_mut().map(|map| map.remove(key));
            }
        }
        serde_json::from_value(options.clone())
            .map_err(|err| format!("invalid settings: {err}").into())
    }
}

#[derive(Subcommand)]
enum SecretAction {
    /// List the secrets that are set or referenced, never their values.
    List,
    /// Set a secret. The value is read from the terminal without echo, or
    /// from standard input when it is piped:
    ///
    ///   anda mcp secret set GITHUB_PAT
    ///   gh auth token | anda mcp secret set GITHUB_PAT
    #[command(verbatim_doc_comment)]
    Set { name: String },
    /// Delete a secret.
    Unset { name: String },
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
                let used_before = offline_secrets_in_use(&daemon.home).await?;
                edit_offline(&daemon.home, McpFileEdit::Remove(&id)).await?;
                let mut done = format!("Removed {id} from mcp.json.");
                if !keep_credentials {
                    credential_store(&daemon.home).clear(&id).await?;
                    // Its secrets go too, unless another server uses them.
                    let store = secret_store(&daemon.home).await;
                    let orphaned = orphaned_secrets(
                        &id,
                        &used_before,
                        &offline_secrets_in_use(&daemon.home).await?,
                        &store.names(),
                    );
                    for name in &orphaned {
                        store.set(name, None).await?;
                    }
                    if !orphaned.is_empty() {
                        done.push_str(&format!(" Deleted secrets: {}.", orphaned.join(", ")));
                    }
                }
                report_offline(json, &done)?;
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
        McpSubcommand::Secret { action } => secret(daemon, live, json, action).await?,
        McpSubcommand::Options { id, flags } => options(daemon, live, json, id, flags).await?,
        McpSubcommand::Import {
            servers,
            sources,
            workspaces,
            dry_run,
            keep_plaintext,
        } => {
            let cwd = std::env::current_dir()?;
            let mut workspaces: Vec<PathBuf> =
                workspaces.into_iter().map(|dir| cwd.join(dir)).collect();
            workspaces.push(cwd);
            let flags = ImportFlags {
                servers,
                sources,
                workspaces,
                dry_run,
                keep_plaintext,
            };
            import_servers(daemon, live, json, flags).await?;
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
        McpSubcommand::Events { id } => {
            let events: Value = require_running(live)?
                .mcp("mcp_events_list", json!({ "id": id }))
                .await?;
            if json {
                print_json(&events)?;
            } else {
                print_events(&id, &events);
            }
        }
        McpSubcommand::Triggers { action } => {
            triggers(
                require_running(live)?,
                json,
                action.unwrap_or(TriggerAction::List),
            )
            .await?
        }
    }
    Ok(())
}

async fn triggers(
    client: &gateway::Client,
    json: bool,
    action: TriggerAction,
) -> Result<(), BoxError> {
    let apply = |change: Value| async move {
        client
            .mcp::<Value>("mcp_trigger_apply", json!({ "change": change }))
            .await
    };
    let detail = match action {
        TriggerAction::List => {
            let triggers: Value = client.mcp("mcp_triggers_list", json!({})).await?;
            if json {
                return print_json(&triggers);
            }
            let triggers = triggers.as_array().cloned().unwrap_or_default();
            if triggers.is_empty() {
                println!(
                    "No MCP event automations. `anda mcp events <server>` lists what a server reports."
                );
            }
            for trigger in &triggers {
                print_trigger_line(trigger);
            }
            return Ok(());
        }
        TriggerAction::Get { id } => client.mcp("mcp_trigger_get", json!({ "id": id })).await?,
        TriggerAction::Add {
            server,
            event,
            instructions,
            arguments,
            name,
            batch_window_secs,
            max_runs_per_hour,
            delivery,
        } => {
            let arguments = match arguments.as_deref() {
                None => json!({}),
                Some(text) => match serde_json::from_str::<Value>(text) {
                    Ok(value @ Value::Object(_)) => value,
                    _ => return Err("--args must be a JSON object".into()),
                },
            };
            let mut trigger = json!({
                "server_id": server, "event": event, "instructions": instructions,
                "arguments": arguments,
            });
            for (key, value) in [
                ("name", name.map(Value::from)),
                ("batch_window_secs", batch_window_secs.map(Value::from)),
                ("max_runs_per_hour", max_runs_per_hour.map(Value::from)),
                ("delivery", delivery.map(Value::from)),
            ] {
                if let Some(value) = value {
                    trigger[key] = value;
                }
            }
            apply(json!({ "op": "create", "trigger": trigger })).await?
        }
        TriggerAction::Pause { id } | TriggerAction::Resume { id } => {
            let enabled = matches!(action, TriggerAction::Resume { .. });
            apply(json!({ "op": "set_enabled", "id": id, "enabled": enabled })).await?
        }
        TriggerAction::Delete { id } => {
            let result = apply(json!({ "op": "delete", "id": id })).await?;
            if json {
                return print_json(&result);
            }
            println!("Deleted automation {id}.");
            return Ok(());
        }
    };
    if json {
        print_json(&detail)
    } else {
        print_trigger(&detail);
        Ok(())
    }
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

async fn options(
    daemon: &Daemon,
    live: Option<&gateway::Client>,
    json: bool,
    id: String,
    flags: OptionFlags,
) -> Result<(), BoxError> {
    let detail = match live {
        Some(client) => client.mcp("mcp_get", json!({ "id": id })).await?,
        None => offline_server(&daemon.home, &id).await?,
    };
    let Some(current) = detail.get("options") else {
        return Err(format!("MCP server {id} has errors in mcp.json; fix them first").into());
    };
    if flags.is_empty() {
        if json {
            return print_json(current);
        }
        print_options(&id, text(&detail, "transport"), current);
        return Ok(());
    }
    let mut next = current.clone();
    let options = flags.apply(&mut next)?;
    let done = format!("Saved the settings of {id}.");
    if let Some(client) = live {
        let change = json!({ "op": "set_options", "id": id, "options": next });
        return report(json, &apply(client, change).await?, &done);
    }
    // Checked as the daemon would, against the entry as it is now.
    let path = McpSettings::file_path(&daemon.home);
    let file = McpConfigFile::read(&path).await?;
    let mut server = McpSettings::from_file_contents(&path, file.text())
        .servers
        .into_iter()
        .find(|server| server.id.trim() == id)
        .ok_or_else(|| format!("MCP server {id} is not configured"))?;
    server.set_options(options.clone())?;
    let issues = server.setup_issues();
    if !issues.is_empty() {
        return Err(format!("invalid settings: {}", issues.join("; ")).into());
    }
    edit_offline(&daemon.home, McpFileEdit::SetOptions(&id, &options)).await?;
    report_offline(json, &done)
}

struct ImportFlags {
    servers: Vec<String>,
    sources: Vec<McpImportSource>,
    workspaces: Vec<PathBuf>,
    dry_run: bool,
    keep_plaintext: bool,
}

async fn import_servers(
    daemon: &Daemon,
    live: Option<&gateway::Client>,
    json: bool,
    flags: ImportFlags,
) -> Result<(), BoxError> {
    let ctx = McpImportContext::detect(flags.workspaces.clone())?;
    let scan = match live {
        Some(client) => {
            let params = json!({ "sources": flags.sources, "workspaces": flags.workspaces });
            client.mcp("mcp_import_scan", params).await?
        }
        None => json!(offline_import_scan(&daemon.home, &ctx, &flags.sources).await?),
    };
    let picks = select_candidates(&scan, &flags.servers)?;
    if flags.dry_run || picks.is_empty() {
        if json {
            return print_json(&scan);
        }
        print_import_scan(&scan);
        if !flags.dry_run {
            println!("Nothing new to import.");
        }
        return Ok(());
    }

    // Secrets the servers need: asked for here, or left to set later.
    let mut needed = BTreeMap::new();
    for (candidate, _) in &picks {
        for secret in candidate["needs_secrets"].as_array().into_iter().flatten() {
            needed.insert(
                text(secret, "name").to_string(),
                text(secret, "description").to_string(),
            );
        }
    }
    let mut values = BTreeMap::new();
    if !json && std::io::stdin().is_terminal() {
        for (name, description) in &needed {
            let value = read_secret(&format!("{name} ({description}); Enter to set it later: "))?;
            if !value.trim().is_empty() {
                values.insert(name.clone(), value);
            }
        }
    }
    let items: Vec<Value> = picks
        .iter()
        .map(|(candidate, id)| json!({ "key": candidate["key"], "id": id }))
        .collect();
    let request = json!({
        "items": items,
        "secrets": values,
        "store_secrets": !flags.keep_plaintext,
        "workspaces": flags.workspaces,
    });
    let receipt = match live {
        Some(client) => client.mcp("mcp_import", request).await?,
        None => offline_import(&daemon.home, &ctx, serde_json::from_value(request)?).await?,
    };
    if json {
        return print_json(&receipt);
    }
    let imported: Vec<&str> = receipt["imported"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    println!("Imported {}.", imported.join(", "));
    let later: Vec<&String> = needed
        .keys()
        .filter(|name| !values.contains_key(*name))
        .collect();
    for name in later {
        println!("Set the secret {name} before its server can start: anda mcp secret set {name}");
    }
    if live.is_none() {
        println!("The daemon is not running; this takes effect when it starts.");
    }
    Ok(())
}

/// The candidates to import, with the ids asked for: those named, as
/// `NAME` or `NAME=ID`, or every one new to Anda.
fn select_candidates(
    scan: &Value,
    selectors: &[String],
) -> Result<Vec<(Value, Option<String>)>, BoxError> {
    let candidates = scan["candidates"].as_array().cloned().unwrap_or_default();
    if selectors.is_empty() {
        return Ok(candidates
            .into_iter()
            .filter(|candidate| matches!(text(candidate, "status"), "new" | "renamed"))
            .map(|candidate| (candidate, None))
            .collect());
    }
    let mut picks = Vec::new();
    for selector in selectors {
        let (name, id) = match selector.split_once('=') {
            Some((name, id)) => (name.trim(), Some(id.trim().to_string())),
            None => (selector.trim(), None),
        };
        let named: Vec<&Value> = candidates
            .iter()
            .filter(|candidate| text(candidate, "name") == name || text(candidate, "key") == name)
            .collect();
        let usable: Vec<&Value> = named
            .iter()
            .copied()
            .filter(|candidate| !matches!(text(candidate, "status"), "invalid" | "exists"))
            .collect();
        match (usable.as_slice(), named.first()) {
            ([candidate], _) => picks.push(((*candidate).clone(), id)),
            ([], None) => {
                return Err(format!(
                    "no server named {name} was found; see `anda mcp import --dry-run`"
                )
                .into());
            }
            ([], Some(candidate)) => {
                let reason = match text(candidate, "status") {
                    "exists" => format!(
                        "it is configured already as {}",
                        text(candidate, "existing_id")
                    ),
                    _ => text(candidate, "error").to_string(),
                };
                return Err(format!("{name} cannot be imported: {reason}").into());
            }
            (several, _) => {
                let sources: Vec<&str> = several
                    .iter()
                    .map(|candidate| text(candidate, "source"))
                    .collect();
                return Err(format!(
                    "{name} is in {}; pick one with --from, or use its key from `anda mcp import --dry-run --json`",
                    sources.join(", ")
                )
                .into());
            }
        }
    }
    Ok(picks)
}

/// A scan without the daemon: Anda's servers come from mcp.json, and
/// variables are checked against this shell's environment.
async fn offline_import_scan(
    home: &Path,
    ctx: &McpImportContext,
    sources: &[McpImportSource],
) -> Result<McpImportScan, BoxError> {
    let path = McpSettings::file_path(home);
    let file = McpConfigFile::read(&path).await?;
    let settings = McpSettings::from_file_contents(&path, file.text());
    let mut known: Vec<McpKnownServer> = settings.servers.iter().map(McpKnownServer::of).collect();
    known.extend(
        settings
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.server_id.as_deref())
            .map(|id| McpKnownServer {
                id: id.trim().to_string(),
                endpoint: None,
            }),
    );
    let secrets = secret_store(home).await.names().into_keys().collect();
    let has_env = |name: &str| std::env::var_os(name).is_some();
    let target = McpImportTarget {
        known: &known,
        secrets: &secrets,
        has_env: &has_env,
    };
    Ok(import::scan(ctx, sources, &target).await)
}

/// An import without the daemon, straight into the files.
async fn offline_import(
    home: &Path,
    ctx: &McpImportContext,
    request: McpImportRequest,
) -> Result<Value, BoxError> {
    let scan = offline_import_scan(home, ctx, &[]).await?;
    let path = McpSettings::file_path(home);
    let file = McpConfigFile::read(&path).await?;
    let declared = McpSettings::from_file_contents(&path, file.text());
    let store = secret_store(home).await;
    let plan = import::plan(
        &scan,
        &request.items,
        &request.secrets,
        request.store_secrets,
        |id| declared.declares(id),
        &store.names().into_keys().collect(),
    )?;
    let servers: Vec<_> = plan
        .servers
        .iter()
        .map(|(server, _)| server.clone())
        .collect();
    import::store_and_write(
        &plan,
        &store,
        edit_offline(home, McpFileEdit::AddAll(&servers)),
    )
    .await?;
    let state = McpStateStore::open(home.join(MCP_STATE_FILE_NAME)).await;
    for (server, path) in &plan.servers {
        let origin = McpOrigin {
            source: McpSource::Import,
            reference: Some(path.clone()),
        };
        state.record_added(&server.id, &origin).await;
    }
    Ok(json!({
        "applied": false,
        "imported": servers.iter().map(|server| server.id.as_str()).collect::<Vec<_>>(),
    }))
}

async fn secret(
    daemon: &Daemon,
    live: Option<&gateway::Client>,
    json: bool,
    action: SecretAction,
) -> Result<(), BoxError> {
    let (name, value) = match action {
        SecretAction::List => {
            let secrets = match live {
                Some(client) => client.mcp("mcp_secrets", json!({})).await?,
                None => json!(secret_views(
                    offline_secrets_in_use(&daemon.home).await?,
                    &secret_store(&daemon.home).await.names(),
                )),
            };
            if json {
                print_json(&secrets)?;
            } else {
                print_secrets(&secrets);
            }
            return Ok(());
        }
        SecretAction::Set { name } => {
            let value = read_secret(&format!("Value for {name}: "))?;
            (name, Some(value))
        }
        SecretAction::Unset { name } => (name, None),
    };
    let done = match value {
        Some(_) => format!("Set the secret {name}."),
        None => format!("Deleted the secret {name}."),
    };
    if let Some(client) = live {
        let change = json!({ "op": "set_secret", "name": name, "value": value });
        report(json, &apply(client, change).await?, &done)
    } else {
        secret_store(&daemon.home)
            .await
            .set(&name, value.as_deref())
            .await?;
        report_offline(json, &done)
    }
}

/// Reads a secret without echoing it: from the terminal, or piped in.
fn read_secret(prompt: &str) -> Result<String, BoxError> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::io::{IsTerminal, Read, Write};

    if !std::io::stdin().is_terminal() {
        let mut value = String::new();
        std::io::stdin().read_to_string(&mut value)?;
        return Ok(value.trim_end_matches(['\r', '\n']).to_string());
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    crossterm::terminal::enable_raw_mode()?;
    let read = || -> Result<String, BoxError> {
        let mut value = String::new();
        loop {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter => return Ok(value),
                KeyCode::Esc => return Err("cancelled".into()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err("cancelled".into());
                }
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(ch) => value.push(ch),
                _ => {}
            }
        }
    };
    let result = read();
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    result
}

async fn secret_store(home: &Path) -> McpSecretStore {
    McpSecretStore::open(home.join(MCP_SECRETS_FILE_NAME)).await
}

/// The secrets mcp.json references, by the ids that reference them.
async fn offline_secrets_in_use(
    home: &Path,
) -> Result<BTreeMap<String, std::collections::BTreeSet<String>>, BoxError> {
    let file = McpConfigFile::read(&McpSettings::file_path(home)).await?;
    Ok(secrets_in_use(&file.root(), []))
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
    if let Some(names) = names("secrets_removed") {
        println!("Deleted secrets: {names}");
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

fn print_secrets(secrets: &Value) {
    let secrets = secrets.as_array().map(Vec::as_slice).unwrap_or_default();
    if secrets.is_empty() {
        println!(
            "No secrets. Reference one in mcp.json as ${{secret:NAME}}, then `anda mcp secret set NAME`."
        );
        return;
    }
    for secret in secrets {
        let used_by: Vec<&str> = secret["used_by"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        println!(
            "{:<24} {:<8} {}",
            text(secret, "name"),
            if secret["is_set"] == true {
                "set"
            } else {
                "not set"
            },
            if used_by.is_empty() {
                "unused".to_string()
            } else {
                format!("used by {}", used_by.join(", "))
            }
        );
    }
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
    if let Some(reference) = detail["source_ref"].as_str() {
        println!("  from: {reference}");
    }
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
    if detail["instructions_changed"] == true {
        println!(
            "  ! the server's instructions changed since review; accept them with `anda mcp review {id}`"
        );
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

fn print_options(id: &str, transport: &str, options: &Value) {
    let or_default = |value: &Value, default: &str| match value {
        Value::Null => format!("{default} (default)"),
        Value::String(text) => text.replace('_', "-"),
        other => other.to_string(),
    };
    println!("{id}");
    println!(
        "  startup:      {}",
        or_default(&options["startup"], "background")
    );
    println!(
        "  lifecycle:    {}",
        or_default(&options["lifecycle"], "auto")
    );
    println!(
        "  concurrency:  {}",
        or_default(&options["concurrency"], "serial")
    );
    let timeouts = &options["timeouts"];
    println!(
        "  timeouts:     setup {}s, list {}s, request {}s, call {}s",
        timeouts["setup_secs"].as_u64().unwrap_or(90),
        timeouts["list_secs"].as_u64().unwrap_or(30),
        timeouts["request_secs"].as_u64().unwrap_or(180),
        timeouts["call_secs"].as_u64().unwrap_or(600),
    );
    println!(
        "  output limit: {} bytes",
        options["limits"]["output_text_bytes"]
            .as_u64()
            .unwrap_or(32 * 1024)
    );
    if transport == "stdio" {
        let inherit = match options["inherit_env"].as_bool() {
            None => "on (default): the daemon's whole environment",
            Some(true) => "on: the daemon's whole environment",
            Some(false) => "off: only the essentials and its own env",
        };
        println!("  inherit env:  {inherit}");
    }
    match options["tasks"]["max_wait_secs"].as_u64() {
        _ if options["tasks"].is_null() => println!("  tasks:        off (default)"),
        Some(secs) => println!("  tasks:        on, waiting up to {secs}s"),
        None => println!("  tasks:        on"),
    }
}

fn print_import_scan(scan: &Value) {
    let candidates = scan["candidates"].as_array().cloned().unwrap_or_default();
    let files = scan["files"].as_array().cloned().unwrap_or_default();
    if files.is_empty() {
        println!("No MCP configuration from other clients was found.");
        return;
    }
    for file in &files {
        let path = text(file, "path");
        let source = serde_json::from_value::<McpImportSource>(file["source"].clone())
            .map(McpImportSource::label)
            .unwrap_or("Another client");
        println!("{source}  {path}");
        if let Some(error) = file["error"].as_str() {
            println!("  ! could not be read: {error}");
            continue;
        }
        // Each file is read once, so its path says which servers are its.
        for candidate in candidates
            .iter()
            .filter(|candidate| text(candidate, "path") == path)
        {
            let name = text(candidate, "name");
            let status = match text(candidate, "status") {
                "new" => "new".to_string(),
                "renamed" => format!("as {}", text(candidate, "id")),
                "exists" => format!("already in Anda as {}", text(candidate, "existing_id")),
                "duplicate" => match candidate["existing_id"].as_str() {
                    Some(id) => format!("same server as {id} in Anda"),
                    None => "same server as one above".to_string(),
                },
                _ => "cannot be imported".to_string(),
            };
            println!("  {name:<20} {status:<28} {}", text(candidate, "summary"));
            if let Some(error) = candidate["error"].as_str() {
                println!("    ! {error}");
            }
            let moved = candidate["plaintext"].as_array().map_or(0, Vec::len);
            if moved > 0 {
                println!("    {moved} plaintext value(s) move to secrets");
            }
            for secret in candidate["needs_secrets"].as_array().into_iter().flatten() {
                println!(
                    "    needs the secret {} ({})",
                    text(secret, "name"),
                    text(secret, "description")
                );
            }
            for warning in candidate["warnings"].as_array().into_iter().flatten() {
                println!("    note: {}", warning.as_str().unwrap_or_default());
            }
        }
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

fn print_events(id: &str, view: &Value) {
    if view["supported"] != true {
        match view["error"].as_str() {
            Some(error) => println!("{id}: events could not be listed: {error}"),
            None => println!("{id} does not report events (MCP Events)."),
        }
        return;
    }
    let events = view["events"].as_array().cloned().unwrap_or_default();
    if events.is_empty() {
        println!("{id} reports no events.");
    }
    for event in &events {
        let delivery: Vec<&str> = event["delivery"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        println!("{}  [{}]", text(event, "name"), delivery.join(", "));
        if let Some(description) = event["description"].as_str() {
            println!("    {description}");
        }
        if event["webhook_only"] == true && view["ingress"]["available"] != true {
            println!(
                "    needs dMsg to receive it: {}",
                text(&view["ingress"], "reason")
            );
        }
    }
    let triggers = view["triggers"].as_array().cloned().unwrap_or_default();
    if !triggers.is_empty() {
        println!();
        for trigger in &triggers {
            print_trigger_line(trigger);
        }
    }
}

fn print_trigger_line(trigger: &Value) {
    println!(
        "{:>4}  {}  {} on {}  {}{}",
        trigger["id"],
        text(trigger, "name"),
        text(trigger, "event"),
        text(trigger, "server_id"),
        text(trigger, "state").replace('_', " "),
        trigger["last_error"]
            .as_str()
            .map(|error| format!(" ({error})"))
            .unwrap_or_default(),
    );
}

fn print_trigger(trigger: &Value) {
    print_trigger_line(trigger);
    println!("  instructions: {}", text(trigger, "instructions"));
    println!("  arguments:    {}", trigger["arguments"]);
    println!(
        "  delivery:     {}{}",
        text(trigger, "delivery"),
        trigger["mode"]
            .as_str()
            .map(|mode| format!(" (using {mode})"))
            .unwrap_or_default()
    );
    println!(
        "  batching:     {}s window, at most {} runs an hour",
        trigger["batch_window_secs"], trigger["max_runs_per_hour"]
    );
    println!(
        "  events:       {} received, {} waiting; {} runs",
        trigger["events_received"], trigger["pending"], trigger["runs"]
    );
    if trigger["missed_events_at"].is_u64() {
        println!("  ! the server reported lost events");
    }
    for run in trigger["runs_recent"].as_array().into_iter().flatten() {
        let outcome = run["error"]
            .as_str()
            .map(|error| format!("failed: {error}"))
            .unwrap_or_else(|| "ok".to_string());
        println!("  run {}: {} events, {}", run["id"], run["events"], outcome);
    }
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

    #[test]
    fn trigger_commands_parse() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            mcp: McpCommand,
        }
        let cli = Cli::try_parse_from([
            "anda",
            "triggers",
            "add",
            "github",
            "issue.opened",
            "--instructions",
            "Label each new issue",
            "--args",
            r#"{"repo":"o/r"}"#,
            "--batch-window",
            "60",
        ])
        .unwrap();
        let McpSubcommand::Triggers {
            action:
                Some(TriggerAction::Add {
                    server,
                    event,
                    arguments,
                    batch_window_secs,
                    ..
                }),
        } = cli.mcp.command
        else {
            panic!("expected triggers add");
        };
        assert_eq!(
            (server.as_str(), event.as_str()),
            ("github", "issue.opened")
        );
        assert_eq!(arguments.as_deref(), Some(r#"{"repo":"o/r"}"#));
        assert_eq!(batch_window_secs, Some(60));
        let cli = Cli::try_parse_from(["anda", "triggers"]).unwrap();
        assert!(matches!(
            cli.mcp.command,
            McpSubcommand::Triggers { action: None }
        ));
        let cli = Cli::try_parse_from(["anda", "events", "github", "--json"]).unwrap();
        assert!(cli.mcp.json && matches!(cli.mcp.command, McpSubcommand::Events { .. }));
    }

    #[test]
    fn option_flags_set_and_clear_settings() {
        let flags = OptionFlags {
            call_timeout: Some("900".into()),
            concurrency: Some("read-only-parallel".into()),
            startup: Some("default".into()),
            tasks: Some("120".into()),
            inherit_env: Some("off".into()),
            ..Default::default()
        };
        let mut options = json!({ "startup": "eager", "timeouts": { "setup_secs": 10 } });
        let parsed = flags.apply(&mut options).unwrap();
        assert_eq!(
            options,
            json!({
                "timeouts": { "setup_secs": 10, "call_secs": 900 },
                "concurrency": "read_only_parallel",
                "inherit_env": false,
                "tasks": { "max_wait_secs": 120 }
            })
        );
        assert_eq!(parsed.timeouts.call_secs, Some(900));

        let clear = OptionFlags {
            setup_timeout: Some("default".into()),
            call_timeout: Some("default".into()),
            tasks: Some("off".into()),
            ..Default::default()
        };
        clear.apply(&mut options).unwrap();
        assert!(options.get("timeouts").is_none() && options.get("tasks").is_none());

        for flags in [
            OptionFlags {
                call_timeout: Some("soon".into()),
                ..Default::default()
            },
            OptionFlags {
                concurrency: Some("many".into()),
                ..Default::default()
            },
            OptionFlags {
                inherit_env: Some("maybe".into()),
                ..Default::default()
            },
        ] {
            assert!(flags.apply(&mut json!({})).is_err());
        }
    }

    #[test]
    fn import_picks_the_new_servers_or_the_ones_named() {
        let scan = json!({ "candidates": [
            { "key": "a#github", "source": "claude_desktop", "name": "github", "status": "new" },
            { "key": "b#github", "source": "cursor", "name": "github", "status": "duplicate" },
            { "key": "b#notes", "source": "cursor", "name": "notes", "status": "renamed", "id": "notes-cursor" },
            { "key": "c#linear", "source": "claude_code", "name": "linear", "status": "exists", "existing_id": "linear" },
            { "key": "c#events", "source": "claude_code", "name": "events", "status": "invalid", "error": "it uses the SSE transport" }
        ]});
        let keys = |picks: Vec<(Value, Option<String>)>| {
            picks
                .into_iter()
                .map(|(candidate, id)| (text(&candidate, "key").to_string(), id))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(select_candidates(&scan, &[]).unwrap()),
            [
                ("a#github".to_string(), None),
                ("b#notes".to_string(), None)
            ]
        );
        assert_eq!(
            keys(select_candidates(&scan, &["notes=work".into(), "b#github".into()]).unwrap()),
            [
                ("b#notes".to_string(), Some("work".to_string())),
                ("b#github".to_string(), None)
            ]
        );
        for (selector, message) in [
            ("github", "pick one with --from"),
            ("linear", "configured already as linear"),
            ("events", "SSE"),
            ("nope", "no server named nope"),
        ] {
            let err = select_candidates(&scan, &[selector.into()]).unwrap_err();
            assert!(err.to_string().contains(message), "{err}");
        }
    }

    #[tokio::test]
    async fn offline_import_writes_mcp_json_secrets_and_sources() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let fixture = crate::engine::mcp::import::tests::fixture().await;
        let scan = json!(offline_import_scan(home, &fixture.ctx, &[]).await.unwrap());
        let picks = select_candidates(&scan, &["search".into(), "wiki=docs-wiki".into()]).unwrap();
        let items: Vec<Value> = picks
            .iter()
            .map(|(candidate, id)| json!({ "key": candidate["key"], "id": id }))
            .collect();
        let request =
            serde_json::from_value(json!({ "items": items, "secrets": { "SEARCH_KEY": "key-1" } }))
                .unwrap();
        let receipt = offline_import(home, &fixture.ctx, request).await.unwrap();
        assert_eq!(receipt["imported"], json!(["search", "docs-wiki"]));

        let config: Value = serde_json::from_str(
            &tokio::fs::read_to_string(McpSettings::file_path(home))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(config["mcpServers"]["search"]["inherit_env"], false);
        assert_eq!(
            config["mcpServers"]["docs-wiki"]["url"],
            "https://wiki.example/mcp"
        );
        let values = secret_store(home).await.values();
        assert_eq!(
            (
                values["SEARCH_KEY"].as_str(),
                values["SEARCH_SEARCH_TOKEN"].as_str()
            ),
            ("key-1", "tok-plain")
        );
        let state = McpStateStore::open(home.join(MCP_STATE_FILE_NAME)).await;
        assert_eq!(state.get("docs-wiki").source, McpSource::Import);
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

    #[tokio::test]
    async fn offline_secrets_list_their_users_and_go_with_the_last_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        for (id, token) in [("docs", "${secret:SHARED}"), ("wiki", "${secret:WIKI}")] {
            let entry = json!({
                "url": format!("https://{id}.test/mcp"),
                "headers": {
                    "Authorization": format!("Bearer {token}"),
                    "X-Also": "${secret:SHARED}"
                }
            });
            let server = McpSettings::parse_entry(id, &entry).unwrap();
            edit_offline(home, McpFileEdit::Add(&server)).await.unwrap();
        }
        let store = secret_store(home).await;
        store.set("SHARED", Some("s")).await.unwrap();
        store.set("WIKI", Some("w")).await.unwrap();

        let views = json!(secret_views(
            offline_secrets_in_use(home).await.unwrap(),
            &store.names()
        ));
        assert_eq!(views[0]["name"], "SHARED");
        assert_eq!(views[0]["used_by"], json!(["docs", "wiki"]));
        assert!(!views.to_string().contains("\"s\""), "{views}");

        let before = offline_secrets_in_use(home).await.unwrap();
        edit_offline(home, McpFileEdit::Remove("wiki"))
            .await
            .unwrap();
        let after = offline_secrets_in_use(home).await.unwrap();
        assert_eq!(
            orphaned_secrets("wiki", &before, &after, &store.names()),
            ["WIKI"]
        );
    }
}
