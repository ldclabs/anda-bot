use anda_core::BoxError;

pub mod agent;
pub mod channel;
pub mod memory;
pub mod memory_eval;
pub mod session;
pub mod updater;
pub mod user;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod voice;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "cli/voice_unsupported.rs"]
pub mod voice;
mod voice_args;

use crate::{daemon, gateway, tui};

pub struct Cli {
    client: gateway::Client,
    daemon: daemon::Daemon,
    full_access: bool,
}

impl Cli {
    pub fn new(client: gateway::Client, daemon: daemon::Daemon, full_access: bool) -> Self {
        Self {
            client,
            daemon,
            full_access,
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        tui::run(self.daemon, self.client, self.full_access).await
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    #[test]
    fn command_constraints_accept_supported_combinations() {
        crate::Cli::command().debug_assert();
        for args in [
            vec!["anda", "agent", "run", "--prompt", "hello"],
            vec!["anda", "agent", "run", "--prompt-file", "prompt.txt"],
            vec!["anda", "voice", "--record-secs", "1"],
            vec!["anda", "channel", "init", "--all"],
            vec!["anda", "channel", "init", "wechat"],
            vec!["anda", "update", "--check", "--json"],
            vec!["anda", "update", "--check-if-due", "--json"],
            vec!["anda", "update", "--force", "--skills"],
            vec!["anda", "memory", "inbox", "--cursor", "page-two", "--json"],
            vec!["anda", "memory", "inbox", "setup", "--json"],
        ] {
            assert!(crate::Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
    }
}
