use anda_core::BoxError;
use clap::{Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationTrack {
    Memory,
    Agent,
}

#[derive(Subcommand)]
pub enum EvaluationCommand {
    /// Freeze a synthetic comparison plan without calling any model (requires mib).
    Plan {
        #[arg(long)]
        model_config: PathBuf,
        #[arg(long, default_value = "MIB_MODEL_API_KEY")]
        api_key_env: String,
        #[arg(long, value_enum, default_value = "memory")]
        track: EvaluationTrack,
        #[arg(long, default_value_t = 1)]
        repeats: u32,
        #[arg(long, default_value_t = 180)]
        operation_seconds: u64,
        #[arg(long, default_value_t = 1800)]
        evaluation_seconds: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Explicitly run a frozen plan; model calls may incur costs (requires mib).
    Run {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Rebuild a report from recorded results without invoking a model (requires mib).
    Report { directory: PathBuf },
}

pub async fn run(command: &EvaluationCommand) -> Result<(), BoxError> {
    #[cfg(feature = "mib")]
    {
        Box::pin(crate::mib::evaluator::run(command)).await
    }
    #[cfg(not(feature = "mib"))]
    {
        let _ = command;
        Err("This build does not include memory evaluation. Build with --features mib; everyday memory does not require it. / 此构建不含评测组件，日常记忆无需启用它。".into())
    }
}
