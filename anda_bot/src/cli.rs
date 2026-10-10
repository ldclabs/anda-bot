pub mod agent;
pub mod auth;
pub mod channel;
pub mod installer;
mod launcher_retirement;
pub mod mcp;
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
