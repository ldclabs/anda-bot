use crate::{brain::product::MEMORY_GUIDE, gateway};
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

pub async fn run(client: &gateway::Client, cmd: &MemoryCommand) -> Result<(), BoxError> {
    cmd.validate()?;
    match &cmd.command {
        Some(MemorySubcommand::Guide) => cmd.print_guide(),
        Some(MemorySubcommand::Evaluate(command)) => return super::memory_eval::run(command).await,
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
