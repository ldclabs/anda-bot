//! A Streamable HTTP MCP server for tests, answering JSON. It offers the
//! tools in its catalog, which a test can change to play a server that
//! updates its tools, and answers every call with what it was called with.

use axum::{Json, http::StatusCode, response::IntoResponse, routing};
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) type Catalog = Arc<RwLock<Vec<Value>>>;

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
    let app = axum::Router::new().route(
        "/mcp",
        routing::post(move |Json(request): Json<Value>| {
            let catalog = catalog.clone();
            async move {
                let Some(id) = request.get("id").cloned() else {
                    return StatusCode::ACCEPTED.into_response();
                };
                let reply = match request["method"].as_str() {
                    Some("initialize") => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "protocolVersion": request["params"]["protocolVersion"],
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "mock", "title": "Mock Server", "version": "1.0.0" },
                            "instructions": "Use the mock."
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
