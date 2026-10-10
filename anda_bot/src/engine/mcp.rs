//! MCP server management.
//!
//! [`McpManager`] owns mcp.json at runtime: it registers the servers the file
//! asks for with the engine's provider, keeps them in step as the file or a
//! runtime-only server changes, and remembers how each connection went. The
//! model tools, the owner API (WebSocket `mcp_*` and `POST /daemon/mcp/v1`),
//! the `anda mcp` CLI and the OAuth callback all change servers through it.

use anda_core::BoxError;
use std::fmt;

mod api;
pub(crate) mod config_store;
mod credentials;
mod manager;
mod model_tools;
mod oauth;
mod redact;
mod state;
mod view;

pub(crate) use api::{McpApiState, is_write_method, mcp_route};
pub(crate) use credentials::{FileMcpCredentialStore, MCP_CREDENTIALS_DIR_NAME};
pub(crate) use manager::{McpChange, McpManager, McpManagerConfig};
pub(crate) use model_tools::{ManageMcpServerTool, McpConnectTool, McpServerTool};
pub(crate) use oauth::{CALLBACK_PATH, mcp_oauth_callback, open_in_browser};
pub(crate) use view::{McpServerView, offline_snapshot};

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
