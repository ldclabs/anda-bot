use clap::Args;

#[derive(Args)]
pub struct VoiceCommand {
    /// Agent name. Empty value uses the default agent.
    #[arg(long, default_value = "")]
    pub(super) name: String,
    /// Recording duration in seconds for each voice turn.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub(super) record_secs: u64,
    /// Do not play returned speech audio artifacts.
    #[arg(long)]
    pub(super) no_playback: bool,
    /// Optional request metadata as a JSON object.
    #[arg(long)]
    pub(super) meta: Option<String>,
}
