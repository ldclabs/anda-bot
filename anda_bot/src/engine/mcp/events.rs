//! MCP event automations.
//!
//! An automation (a trigger) runs the agent on the owner's instructions when
//! an MCP server reports an event (the experimental MCP Events extension, see
//! `anda_engine::extension::mcp`). [`McpEventRuntime`] keeps each one
//! subscribed, stores the events, and hands them to unattended agent runs on
//! the route the automation was created from, the way cron runs are. Event
//! data is untrusted: those runs are not given full access, so anything that
//! needs the owner's approval is refused. Events delivered only by webhook
//! are received through dMsg ([`ingress`]).

mod ingress;
mod manage;
mod runtime;
mod store;
mod tools;

pub(crate) use manage::{TriggerInput, TriggerPatch};
pub(crate) use runtime::{McpEventRuntime, McpEventRuntimeConfig};
pub(crate) use store::TriggerStore;
pub(crate) use tools::{CreateEventTriggerTool, ListMcpEventsTool, ManageEventTriggerTool};
