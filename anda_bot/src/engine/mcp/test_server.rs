//! A Streamable HTTP MCP server for tests, answering JSON. It offers the
//! tools in its catalog and the instructions given, which a test can change
//! to play a server that updates them, and answers every call with what it
//! was called with. It also offers two resources, `file:///notes.md` (text)
//! and `file:///logo.png` (a blob), and a template.

use axum::{Json, http::StatusCode, response::IntoResponse, routing};
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) type Catalog = Arc<RwLock<Vec<Value>>>;
pub(crate) type Instructions = Arc<RwLock<String>>;

/// A read-only tool definition named `name`.
pub(crate) fn read_only_tool(name: &str) -> Value {
    json!({
        "name": name,
        "description": format!("The {name} tool"),
        "inputSchema": { "type": "object" },
        "annotations": { "readOnlyHint": true }
    })
}

/// Serves `catalog` and returns the endpoint URL.
pub(crate) async fn serve(catalog: Catalog) -> String {
    serve_with(catalog, Arc::new(RwLock::new("Use the mock.".to_string()))).await
}

/// Serves `catalog` with `instructions`, and returns the endpoint URL.
pub(crate) async fn serve_with(catalog: Catalog, instructions: Instructions) -> String {
    let app = axum::Router::new().route(
        "/mcp",
        routing::post(move |Json(request): Json<Value>| {
            let catalog = catalog.clone();
            let instructions = instructions.clone();
            async move {
                let Some(id) = request.get("id").cloned() else {
                    return StatusCode::ACCEPTED.into_response();
                };
                let reply = match request["method"].as_str() {
                    Some("initialize") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "protocolVersion": request["params"]["protocolVersion"],
                            "capabilities": { "tools": {}, "resources": {} },
                            "serverInfo": { "name": "mock", "title": "Mock Server", "version": "1.0.0" },
                            "instructions": *instructions.read()
                        }
                    }),
                    Some("tools/list") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "tools": catalog.read().clone() }
                    }),
                    Some("tools/call") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": format!(
                                    "called {} with {}",
                                    request["params"]["name"].as_str().unwrap_or_default(),
                                    request["params"]["arguments"]
                                )
                            }]
                        }
                    }),
                    Some("resources/list") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "resources": [
                            { "uri": "file:///notes.md", "name": "notes.md",
                              "description": "Meeting notes", "mimeType": "text/markdown" },
                            { "uri": "file:///logo.png", "name": "logo.png",
                              "mimeType": "image/png", "size": 4 }
                        ] }
                    }),
                    Some("resources/templates/list") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "resourceTemplates": [
                            { "uriTemplate": "file:///issues/{id}", "name": "issue" }
                        ] }
                    }),
                    Some("resources/read") => match request["params"]["uri"].as_str() {
                        Some("file:///notes.md") => json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": { "contents": [{ "uri": "file:///notes.md",
                                "mimeType": "text/markdown", "text": "# Notes\nShip it." }] }
                        }),
                        Some("file:///logo.png") => json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": { "contents": [{ "uri": "file:///logo.png",
                                "mimeType": "image/png", "blob": "iVBORw==" }] }
                        }),
                        _ => json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32002, "message": "Resource not found" }
                        }),
                    },
                    _ => json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": { "code": -32601, "message": "Method not found" }
                    }),
                };
                Json(reply).into_response()
            }
        }),
    );
    format!("{}/mcp", crate::test_support::spawn_http_mock(app).await)
}

/// What an MCP Events mock serves, and what it was asked.
#[derive(Clone, Default)]
pub(crate) struct EventsMock {
    /// The event types `events/list` returns.
    pub events: Arc<RwLock<Vec<Value>>>,
    /// `events/poll` results, one per poll; when none are left a poll gets
    /// no events at the last cursor.
    pub pages: Arc<parking_lot::Mutex<std::collections::VecDeque<Value>>>,
    /// Structured results of the tools it offers, by name.
    pub tools: Arc<RwLock<std::collections::BTreeMap<String, Value>>>,
    /// The `events/subscribe` result.
    pub subscription: Arc<RwLock<Value>>,
    /// Every request, as `{method, params}`.
    pub requests: Arc<parking_lot::Mutex<Vec<Value>>>,
}

impl EventsMock {
    /// The params of each request for `method`, in order.
    pub(crate) fn calls(&self, method: &str) -> Vec<Value> {
        self.requests
            .lock()
            .iter()
            .filter(|request| request["method"] == method)
            .map(|request| request["params"].clone())
            .collect()
    }

    /// The arguments of each call to the tool `name`.
    pub(crate) fn tool_calls(&self, name: &str) -> Vec<Value> {
        self.calls("tools/call")
            .into_iter()
            .filter(|params| params["name"] == name)
            .map(|params| params["arguments"].clone())
            .collect()
    }
}

/// Serves an MCP server with MCP Events from `mock` and returns its URL.
pub(crate) async fn serve_events(mock: EventsMock) -> String {
    let last_cursor = Arc::new(RwLock::new(Value::Null));
    let app = axum::Router::new().route(
        "/mcp",
        routing::post(move |Json(request): Json<Value>| {
            let mock = mock.clone();
            let last_cursor = last_cursor.clone();
            async move {
                let Some(id) = request.get("id").cloned() else {
                    return StatusCode::ACCEPTED.into_response();
                };
                let params = request["params"].clone();
                mock.requests
                    .lock()
                    .push(json!({"method": request["method"], "params": params}));
                let result = match request["method"].as_str().unwrap_or_default() {
                    "initialize" => json!({
                        "protocolVersion": params["protocolVersion"],
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "events-mock", "version": "1.0.0" }
                    }),
                    "tools/list" => {
                        let tools: Vec<Value> = mock
                            .tools
                            .read()
                            .keys()
                            .map(|name| read_only_tool(name))
                            .collect();
                        json!({ "tools": tools })
                    }
                    "tools/call" => {
                        let name = params["name"].as_str().unwrap_or_default();
                        match mock.tools.read().get(name) {
                            Some(result) => json!({
                                "content": [{"type": "text", "text": result.to_string()}],
                                "structuredContent": result,
                            }),
                            None => json!({
                                "content": [{"type": "text", "text": format!("no tool {name}")}],
                                "isError": true,
                            }),
                        }
                    }
                    "events/list" => json!({ "events": mock.events.read().clone() }),
                    "events/poll" => match mock.pages.lock().pop_front() {
                        Some(page) => {
                            if !page["cursor"].is_null() {
                                *last_cursor.write() = page["cursor"].clone();
                            }
                            page
                        }
                        None => json!({
                            "events": [], "cursor": *last_cursor.read(),
                            "hasMore": false, "nextPollMs": 60_000
                        }),
                    },
                    "events/subscribe" => mock.subscription.read().clone(),
                    "events/unsubscribe" => json!({}),
                    _ => {
                        return Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32601, "message": "Method not found" }
                        }))
                        .into_response();
                    }
                };
                Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
            }
        }),
    );
    format!("{}/mcp", crate::test_support::spawn_http_mock(app).await)
}
