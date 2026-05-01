use anda_core::BoxError;
use clap::Args;

use crate::{config, gateway};

#[derive(Args)]
pub struct VoiceCommand {
    /// Agent name. Empty value uses the default agent.
    #[arg(long, default_value = "")]
    name: String,
    /// Recording duration in seconds for each voice turn.
    #[arg(long, default_value_t = 5)]
    record_secs: u64,
    /// Do not play returned speech audio artifacts.
    #[arg(long)]
    no_playback: bool,
    /// Optional request metadata as a JSON object.
    #[arg(long)]
    meta: Option<String>,
}

pub async fn run_voice_loop(
    _client: &gateway::Client,
    _cfg: &config::Config,
    cmd: VoiceCommand,
) -> Result<(), BoxError> {
    let VoiceCommand {
        name,
        record_secs,
        no_playback,
        meta,
    } = cmd;
    let _ = (name, record_secs, no_playback, meta);

    Err("`anda voice` is only enabled on macOS and Windows builds".into())
}
