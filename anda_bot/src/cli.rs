use anda_core::BoxError;

use crate::{
    daemon, gateway, tui,
    util::{
        http_client::build_http_client,
        key::{ClaimsSetBuilder, Ed25519Key, iana},
    },
};

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
        tui::run(self.daemon, self.client).await
    }
}
