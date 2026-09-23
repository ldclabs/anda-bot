use anda_core::BoxError;

use crate::{config, gateway};

pub use super::voice_args::VoiceCommand;

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
