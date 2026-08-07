//! Wire contract between the `anda` daemon CLI and the `anda_launcher` tray app.
//!
//! Both binaries compile this same source file — `main.rs` declares
//! `mod daemon_protocol;` and `anda_launcher.rs` includes it via `#[path]` —
//! so the JSON one side prints and the other side parses cannot drift.
//!
//! Compatibility rules: the two binaries are replaced together by the update
//! flow, but an already-running launcher keeps driving an updated `anda`
//! binary until it restarts. Add fields (covered by `#[serde(default)]`)
//! rather than renaming or retyping them, and let new [`AutoUpdateStatus`]
//! values degrade to [`AutoUpdateStatus::Unknown`] on old parsers.
//! [`AutoUpdateState`] is also persisted in AndaDB (`anda_auto_update`), so
//! the same rules protect stored state across versions.

use serde::{Deserialize, Serialize};

/// Daemon liveness as reported by `anda status --json`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatusState {
    Running,
    GatewayRunning,
    ProcessUnresponsive,
    #[default]
    NotRunning,
}

/// Report printed by `anda status --json` and parsed by the launcher.
// Each binary reads only the fields it displays; unused ones are still wire.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonStatusReport {
    pub state: DaemonStatusState,
    pub summary: String,
    pub pid: Option<u32>,
    pub pid_file: Option<String>,
    pub gateway_url: Option<String>,
    pub log_file: Option<String>,
    pub conversations: Option<u64>,
    pub memory_nodes: Option<u64>,
    pub memory_links: Option<u64>,
}

/// Lifecycle of the daemon-side auto updater.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateStatus {
    #[default]
    Idle,
    Checking,
    Current,
    Downloading,
    Downloaded,
    Failed,
    Installed,
    /// A status written by a different anda version that this binary does not
    /// know. Treat it like "nothing actionable".
    #[serde(other)]
    Unknown,
}

/// Auto-update state persisted by the daemon and printed by
/// `anda update --check[-if-due] --json`.
// Each binary reads only the fields it displays; unused ones are still wire.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoUpdateState {
    pub status: AutoUpdateStatus,
    pub current_tag: String,
    pub latest_tag: Option<String>,
    pub last_checked_ms: Option<u64>,
    pub downloaded_at_ms: Option<u64>,
    pub installed_at_ms: Option<u64>,
    pub target: Option<String>,
    pub asset_name: Option<String>,
    pub downloaded_path: Option<String>,
    pub sha256: Option<String>,
    pub checksum_verified: bool,
    pub error: Option<String>,
}

impl Default for AutoUpdateState {
    fn default() -> Self {
        Self {
            status: AutoUpdateStatus::Idle,
            current_tag: current_version_tag(),
            latest_tag: None,
            last_checked_ms: None,
            downloaded_at_ms: None,
            installed_at_ms: None,
            target: None,
            asset_name: None,
            downloaded_path: None,
            sha256: None,
            checksum_verified: false,
            error: None,
        }
    }
}

/// Output of `anda browser token --json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserTokenReport {
    pub gateway_url: String,
    pub token: String,
    pub extension_dir: String,
}

/// The version tag this binary reports for itself (`v<CARGO_PKG_VERSION>`).
pub fn current_version_tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}
