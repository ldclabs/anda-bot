use anda_core::BoxError;
use structured_logger::{Builder, async_json::new_writer, get_env_level};

pub mod voice;

use crate::{
    daemon, gateway, tui,
    util::{
        http_client::build_http_client,
        key::{ClaimsSetBuilder, Ed25519Key, iana},
    },
};

const CLI_LOG_FILE: &str = "anda-cli.log";

pub struct Cli {
    client: gateway::Client,
    daemon: daemon::Daemon,
}

impl Cli {
    pub fn new(id_key: Ed25519Key, daemon: daemon::Daemon) -> Self {
        let gateway_token = id_key
            .sign_cwt(
                ClaimsSetBuilder::new()
                    .claim(iana::CwtClaimName::Scope, "*".into())
                    .build(),
            )
            .unwrap();
        Self {
            client: gateway::Client::new(daemon.base_url(), gateway_token)
                .with_http_client(build_http_client(None, |client| client.no_proxy()).unwrap()),
            daemon,
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        let log = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.daemon.logs_dir_path().join(CLI_LOG_FILE))
            .await?;
        // Initialize structured logging with JSON format
        Builder::with_level(&get_env_level().to_string())
            .with_target_writer("*", new_writer(log))
            .init();
        tui::run(self.daemon, self.client).await
    }
}
