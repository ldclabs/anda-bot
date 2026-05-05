use anda_core::BoxError;

pub mod channel;
pub mod updater;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod voice;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "cli/voice_unsupported.rs"]
pub mod voice;

use crate::{daemon, gateway, tui};

pub struct Cli {
    client: gateway::Client,
    daemon: daemon::Daemon,
}

impl Cli {
    pub fn new(client: gateway::Client, daemon: daemon::Daemon) -> Self {
        Self { client, daemon }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        tui::run(self.daemon, self.client).await
    }
}
