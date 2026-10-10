//! MCP server management.
//!
//! [`McpManager`] owns mcp.json at runtime: it registers the servers the file
//! asks for with the engine's provider, keeps them in step as the file or a
//! runtime-only server changes, and remembers how each connection went. The
//! model tools, the owner API (WebSocket `mcp_*` and `POST /daemon/mcp/v1`),
//! the `anda mcp` CLI and the OAuth callback all change servers through it.
//!
//! [`McpGate`] stands between the agent and the servers' tools: every call
//! passes it, and it asks the owner first when the server's approval policy,
//! the session's approval mode and the tool's reviewed definition say so.
//!
//! The values mcp.json references as `${secret:NAME}` live apart, in the
//! owner-only [`McpSecretStore`], and never leave the daemon.

use anda_core::BoxError;
use std::fmt;

mod api;
pub(crate) mod config_store;
mod credentials;
mod elicitation;
mod events;
mod gate;
pub(crate) mod import;
mod manager;
mod model_tools;
mod oauth;
mod redact;
mod registry;
mod resources;
mod review;
mod secrets;
mod state;
#[cfg(test)]
mod test_server;
mod view;

pub(crate) use api::{McpApiState, is_write_method, mcp_route};
pub(crate) use credentials::{FileMcpCredentialStore, MCP_CREDENTIALS_DIR_NAME};
pub(crate) use elicitation::McpElicitations;
pub(crate) use events::{
    CreateEventTriggerTool, ListMcpEventsTool, ManageEventTriggerTool, McpEventRuntime,
    McpEventRuntimeConfig, TriggerInput, TriggerPatch, TriggerStore,
};
pub(crate) use gate::McpGate;
pub(crate) use manager::{McpChange, McpManager, McpManagerConfig};
pub(crate) use model_tools::{ManageMcpServerTool, McpConnectTool, McpServerTool};
pub(crate) use oauth::{CALLBACK_PATH, mcp_oauth_callback, open_in_browser};
pub(crate) use registry::MCP_REGISTRY_URL;
pub(crate) use resources::McpResourcesTool;
pub(crate) use secrets::{
    MCP_SECRETS_FILE_NAME, McpSecretStore, orphaned_secrets, secret_views, secrets_in_use,
};
pub(crate) use state::{MCP_STATE_FILE_NAME, McpOrigin, McpSource, McpStateStore};
pub(crate) use view::{McpServerView, McpStatus, offline_snapshot};

/// A request the caller can correct, with a stable code for the API.
pub(crate) struct McpError {
    pub code: &'static str,
    pub message: String,
}

impl McpError {
    fn boxed(code: &'static str, message: impl Into<String>) -> BoxError {
        Box::new(Self {
            code,
            message: message.into(),
        })
    }

    pub fn not_found(id: &str) -> BoxError {
        Self::boxed("not_found", format!("MCP server {id} is not configured"))
    }

    pub fn invalid(message: impl Into<String>) -> BoxError {
        Self::boxed("invalid_request", message)
    }

    /// Something other than a server was not found.
    pub fn missing(message: impl Into<String>) -> BoxError {
        Self::boxed("not_found", message)
    }

    pub fn already_exists(message: impl Into<String>) -> BoxError {
        Self::boxed("already_exists", message)
    }

    pub fn conflict(message: impl Into<String>) -> BoxError {
        Self::boxed("revision_conflict", message)
    }
}

// Shown like a plain message where errors are debug-printed (`anda` exits).
impl fmt::Debug for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.message, f)
    }
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for McpError {}
