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
    if let Some(MemorySubcommand::Inbox { command }) = &cmd.command {
        if let Some(InboxCommand::Setup { apply }) = command {
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
        } else {
            let page = client
                .brain()
                .attention(&crate::brain::AttentionQuery {
                    cursor: None,
                    limit: Some(20),
                })
                .await?;
            if cmd.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"result":page}))?
                );
            } else {
                println!("{}", crate::brain::outbox::render(&page));
            }
        }
        return Ok(());
    }
    if let Some(MemorySubcommand::Activity {
        conversation,
        cursor,
    }) = &cmd.command
    {
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
        return Ok(());
    }
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
    Ok(())
}
