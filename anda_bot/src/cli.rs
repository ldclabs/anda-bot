use anda_core::BoxError;

pub mod agent;
pub mod channel;
pub mod session;
pub mod updater;
pub mod user;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod voice;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "cli/voice_unsupported.rs"]
pub mod voice;

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
