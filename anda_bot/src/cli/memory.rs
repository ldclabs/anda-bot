use crate::{
    brain::{
        agents::{AgentGrant, AgentScope, DEFAULT_GRANT_DAYS, GrantRequest, IssuedGrant},
        product::MEMORY_GUIDE,
    },
    gateway,
};
use anda_core::BoxError;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct MemoryCommand {
    /// Print the versioned status envelope as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<MemorySubcommand>,
}

#[derive(Subcommand)]
enum MemorySubcommand {
    /// Read the attention inbox or prepare its minimal owner configuration.
    Inbox {
        /// Continue an inbox page using its next_cursor.
        #[arg(long)]
        cursor: Option<String>,
        #[command(subcommand)]
        command: Option<InboxCommand>,
    },
    /// Isolated synthetic memory comparisons; not required for everyday memory.
    #[command(subcommand)]
    Evaluate(super::memory_eval::EvaluationCommand),
    /// Read the guide offline without loading configuration or identity.
    Guide,
    /// Check the running service without starting it or invoking a model.
    Status,
    /// Inspect recorded processing activity, optionally for one conversation.
    Activity {
        #[arg(long)]
        conversation: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Share memory with other local agents (Claude Code, Codex, …) over MCP.
    Agents {
        #[command(subcommand)]
        command: Option<AgentsCommand>,
    },
}

#[derive(Subcommand)]
enum AgentsCommand {
    /// List the agents' grants.
    List,
    /// Create a token for an agent and print how to connect it.
    Add {
        /// The agent's handle, such as claude-code or codex. Its notes are
        /// attributed to it across tokens.
        client: String,
        /// Display name; defaults to a known client's, such as "Claude Code".
        #[arg(long)]
        name: Option<String>,
        /// Also let the agent tell memory what to remember.
        #[arg(long)]
        write: bool,
        /// Days until the token expires (1-3650).
        #[arg(long, default_value_t = DEFAULT_GRANT_DAYS)]
        days: u64,
    },
    /// Revoke a grant by id, or every grant of a client.
    Revoke { target: String },
}

#[derive(Subcommand)]
enum InboxCommand {
    Setup {
        #[arg(long)]
        apply: Option<String>,
    },
}

impl MemoryCommand {
    pub fn evaluation(&self) -> Option<&super::memory_eval::EvaluationCommand> {
        match &self.command {
            Some(MemorySubcommand::Evaluate(command)) => Some(command),
            _ => None,
        }
    }
    pub fn is_guide(&self) -> bool {
        matches!(self.command, Some(MemorySubcommand::Guide))
    }
    pub fn validate(&self) -> Result<(), BoxError> {
        if matches!(
            &self.command,
            Some(MemorySubcommand::Inbox {
                cursor: Some(_),
                command: Some(_),
            })
        ) {
            return Err("--cursor cannot be combined with inbox setup".into());
        }
        if self.evaluation().is_some() && self.json {
            return Err(
                "evaluate writes its own structured artifacts; --json is not supported".into(),
            );
        }
        if self.is_guide() && self.json {
            return Err("`anda memory guide` does not support --json".into());
        }
        Ok(())
    }
    pub fn print_guide(&self) {
        println!("{MEMORY_GUIDE}");
    }
}

/// Runs the memory commands that read the daemon. `main` validates `cmd` and
/// handles the offline guide and evaluations before building a client.
pub async fn run(client: &gateway::Client, cmd: &MemoryCommand) -> Result<(), BoxError> {
    match &cmd.command {
        Some(MemorySubcommand::Guide | MemorySubcommand::Evaluate(_)) => {
            unreachable!("offline memory commands dispatch before a client is built")
        }
        Some(MemorySubcommand::Inbox {
            command: Some(InboxCommand::Setup { apply }),
            ..
        }) => {
            let view = client.memory_setup(apply.as_deref()).await?;
            if cmd.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":view}))?
                );
            } else {
                println!(
                    "Inbox setup / 记忆待办设置\n{}\n{} + {}",
                    view.state, view.config_file, view.runtime_file
                );
                if view.state == "prepared" {
                    println!(
                        "Owner inbox only; existing config is backed up and reformatted. No automatic learning or IM actions.\n仅配置 owner 的应用内待办，备份并重新格式化配置，不启用自动学习或 IM 动作。\n\nanda memory inbox setup --apply {}",
                        view.preview_digest
                    );
                } else {
                    println!("Restart to install bindings / 重启以安装绑定:\nanda restart");
                }
            }
        }
        Some(MemorySubcommand::Inbox {
            cursor,
            command: None,
        }) => {
            let page = client
                .brain()
                .attention(&crate::brain::AttentionQuery {
                    cursor: cursor.clone(),
                    limit: Some(20),
                })
                .await?;
            if cmd.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":page}))?
                );
            } else {
                println!("{}", render_inbox_page(&page));
            }
        }
        Some(MemorySubcommand::Activity {
            conversation,
            cursor,
        }) => {
            let mut page = client
                .memory_activity(&crate::brain::activity::ActivityQuery {
                    conversation: conversation.clone(),
                    cursor: cursor.clone(),
                    limit: Some(20),
                })
                .await?;
            if cmd.json {
                let cursor = page.next_cursor.take();
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"result":page,"next_cursor":cursor})
                    )?
                );
            } else {
                println!("{}", page.render());
            }
        }
        Some(MemorySubcommand::Agents { command }) => {
            run_agents(client, command.as_ref(), cmd.json).await?
        }
        None | Some(MemorySubcommand::Status) => {
            let overview = client.memory_overview().await?;
            if cmd.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":overview}))?
                );
            } else {
                println!("{}", overview.render());
            }
            if !overview.is_connected() {
                return Err("Memory status unavailable; check `anda status`. / 无法读取记忆状态，请检查 `anda status`。".into());
            }
        }
    }
    Ok(())
}

async fn run_agents(
    client: &gateway::Client,
    command: Option<&AgentsCommand>,
    json: bool,
) -> Result<(), BoxError> {
    match command {
        None | Some(AgentsCommand::List) => {
            let list = client.memory_agents().await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":list}))?
                );
            } else {
                println!("{}", render_grants(&list.items, anda_engine::unix_ms()));
            }
        }
        Some(AgentsCommand::Add {
            client: agent,
            name,
            write,
            days,
        }) => {
            let issued = client
                .memory_agent_create(&GrantRequest {
                    client: agent.clone(),
                    name: name.clone(),
                    scope: if *write {
                        AgentScope::Write
                    } else {
                        AgentScope::Read
                    },
                    days: Some(*days),
                })
                .await?;
            let url = format!("{}{}", client.base_url(), crate::engine::MEMORY_MCP_PATH);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"result":{"grant":issued.grant,"token":issued.token,"url":url}})
                    )?
                );
            } else {
                println!("{}", render_issued(&issued, &url));
            }
        }
        Some(AgentsCommand::Revoke { target }) => {
            let revoked = client.memory_agent_revoke(target).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":revoked}))?
                );
            } else {
                for grant in &revoked.revoked {
                    println!(
                        "Revoked / 已撤销: {} ({}) {}",
                        grant.name, grant.client, grant.id
                    );
                }
            }
        }
    }
    Ok(())
}

fn date(ms: u64) -> String {
    anda_engine::rfc3339_datetime(ms)
        .map(|time| time.chars().take(10).collect())
        .unwrap_or_default()
}

fn scope_label(scope: AgentScope) -> &'static str {
    match scope {
        AgentScope::Read => "read / 只读",
        AgentScope::Write => "read + write / 读写",
    }
}

fn render_grants(grants: &[AgentGrant], now_ms: u64) -> String {
    let mut text = "Memory agents / 记忆访问授权\n".to_string();
    for grant in grants {
        let state = match grant.revoked_at {
            Some(at) => format!("revoked / 已撤销 {}", date(at)),
            None if grant.is_active(now_ms) => format!("expires / 到期 {}", date(grant.expires_at)),
            None => format!("expired / 已过期 {}", date(grant.expires_at)),
        };
        let used = grant
            .last_used_at
            .map(|at| format!(" · last used / 最近使用 {}", date(at)))
            .unwrap_or_default();
        text.push_str(&format!(
            "\n{} · {} ({}) · {} · {state}{used}",
            grant.id,
            grant.name,
            grant.client,
            scope_label(grant.scope)
        ));
    }
    if grants.is_empty() {
        text.push_str("\nNo agents yet / 暂无授权\nanda memory agents add claude-code --write");
    }
    text
}

fn render_issued(issued: &IssuedGrant, url: &str) -> String {
    let grant = &issued.grant;
    let token = &issued.token;
    format!(
        "Memory access for {name} ({client}): {scope}, expires {expires}.\n\
         已授权 {name} 访问记忆：{scope}，{expires} 到期。\n\n\
         Token, shown once / 令牌仅显示一次:\n{token}\n\n\
         Claude Code:\n  \
         claude mcp add --transport http --scope user anda-memory {url} --header \"Authorization: Bearer {token}\"\n\n\
         Codex (~/.codex/config.toml, with ANDA_MEMORY_TOKEN set to the token / 并把令牌设为 ANDA_MEMORY_TOKEN):\n  \
         [mcp_servers.anda-memory]\n  \
         url = \"{url}\"\n  \
         bearer_token_env_var = \"ANDA_MEMORY_TOKEN\"\n\n\
         Other clients / 其他客户端: Streamable HTTP {url}, header Authorization: Bearer <token>\n\
         Revoke / 撤销: anda memory agents revoke {client}",
        name = grant.name,
        client = grant.client,
        scope = scope_label(grant.scope),
        expires = date(grant.expires_at),
    )
}

fn render_inbox_page(page: &crate::brain::AttentionPage) -> String {
    let mut text = crate::brain::outbox::render(page);
    if let Some(cursor) = &page.next_cursor {
        text.push_str(&format!(
            "\nMore items may follow / 尚未读取全部事项:\nanda memory inbox --cursor {cursor}\n"
        ));
    } else if !page.complete {
        text.push_str("\nInbox snapshot is incomplete; retry `anda memory inbox`. / 待办快照不完整，请重试 anda memory inbox。\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Query, routing::get};
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc};

    #[test]
    fn agent_grants_render_their_state_and_how_to_connect() {
        let grant = AgentGrant {
            id: "d3grant".into(),
            client: "claude-code".into(),
            name: "Claude Code".into(),
            scope: AgentScope::Write,
            created_at: 0,
            expires_at: 1_790_000_000_000,
            revoked_at: None,
            last_used_at: Some(1_780_000_000_000),
        };
        let text = render_issued(
            &IssuedGrant {
                grant: grant.clone(),
                token: "anda_mem_secret".into(),
            },
            "http://127.0.0.1:8042/mcp/memory",
        );
        assert!(text.contains(
            "claude mcp add --transport http --scope user anda-memory http://127.0.0.1:8042/mcp/memory --header \"Authorization: Bearer anda_mem_secret\""
        ));
        assert!(text.contains("bearer_token_env_var = \"ANDA_MEMORY_TOKEN\""));
        assert!(text.contains("anda memory agents revoke claude-code"));

        let listed = render_grants(
            &[
                grant.clone(),
                AgentGrant {
                    revoked_at: Some(1_785_000_000_000),
                    ..grant.clone()
                },
            ],
            1_785_000_000_000,
        );
        assert!(listed.contains("d3grant · Claude Code (claude-code) · read + write"));
        assert!(listed.contains("expires / 到期 2026-09-21"));
        assert!(listed.contains("last used / 最近使用 2026-05-28"));
        assert!(listed.contains("revoked / 已撤销"));
        assert!(!listed.contains("anda_mem_"));
        assert!(render_grants(&[], 0).contains("anda memory agents add"));
    }

    #[tokio::test]
    async fn inbox_reads_next_page_and_shows_continuation() {
        let first = json!({
            "scope":{"space_id":"anda_bot","space_instance":"test"},
            "items":[],"next_cursor":"page-two","complete":false,
        });
        let mut second = first.clone();
        second["next_cursor"] = serde_json::Value::Null;
        second["complete"] = true.into();
        assert!(
            render_inbox_page(&serde_json::from_value(first.clone()).unwrap())
                .contains("anda memory inbox --cursor page-two")
        );
        assert!(
            !render_inbox_page(&serde_json::from_value(second.clone()).unwrap())
                .contains("--cursor")
        );
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed = requests.clone();
        let app = Router::new().route(
            "/v1/anda_bot/attention",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let observed = observed.clone();
                let first = first.clone();
                let second = second.clone();
                async move {
                    assert_eq!(query.get("limit").map(String::as_str), Some("20"));
                    let cursor = query.get("cursor").cloned();
                    observed.lock().push(cursor.clone());
                    let page = match cursor.as_deref() {
                        None => first,
                        Some("page-two") => second,
                        other => panic!("unexpected cursor {other:?}"),
                    };
                    Json(json!({"result":page}))
                }
            }),
        );
        let client = gateway::Client::new(
            crate::test_support::spawn_http_mock(app).await,
            "token".into(),
        );
        for json in [false, true] {
            for cursor in [None, Some("page-two".into())] {
                run(
                    &client,
                    &MemoryCommand {
                        json,
                        command: Some(MemorySubcommand::Inbox {
                            cursor,
                            command: None,
                        }),
                    },
                )
                .await
                .unwrap();
            }
        }
        assert_eq!(
            *requests.lock(),
            vec![None, Some("page-two".into()), None, Some("page-two".into())]
        );
    }
}
