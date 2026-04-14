use anda_core::BoxError;
use anda_engine::model::reqwest;

use crate::{daemon, tui, util::http_client::build_http_client};

pub struct Cli {
    http: reqwest::Client,
    daemon: daemon::Daemon,
}

impl Cli {
    pub fn new(daemon: daemon::Daemon) -> Self {
        Self {
            http: build_http_client(None, |client| client.no_proxy())
                .expect("failed to build HTTP client for CLI"),
            daemon,
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        tui::run(self.daemon, self.http).await
    }
}
