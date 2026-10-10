//! Searching the official MCP Registry. The daemon makes the request, so it
//! goes through the daemon's proxy settings like every other outbound call.
//!
//! The Registry says that a server was published, not that it is safe:
//! installing one goes through the same test, review pinning and approval
//! policy as any other server. The Registry is still in preview, so entries
//! are passed on as the Registry writes them (`server.json`) and the app
//! reads only the fields it knows.

use anda_core::BoxError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use super::McpError;
use crate::util::http_client::check_http_response;

pub(crate) const MCP_REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io";
const PAGE_SIZE: usize = 30;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_QUERY_CHARS: usize = 200;
const STATUS_META: &str = "io.modelcontextprotocol.registry/official";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpRegistryQuery {
    /// Words to look for in server names; empty lists them all.
    #[serde(default)]
    pub query: String,
    /// Where the previous page ended.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpRegistryPage {
    /// The latest version of each active server, as `server.json`.
    pub servers: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// One page of the Registry's servers matching `query`.
pub(crate) async fn search(
    http: &reqwest::Client,
    base_url: &str,
    query: &McpRegistryQuery,
) -> Result<McpRegistryPage, BoxError> {
    let text = query.query.trim();
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(McpError::invalid("the search is too long"));
    }
    let mut url = reqwest::Url::parse(base_url)?.join("v0.1/servers")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("version", "latest");
        pairs.append_pair("limit", &PAGE_SIZE.to_string());
        if !text.is_empty() {
            pairs.append_pair("search", text);
        }
        if let Some(cursor) = query.cursor.as_deref().filter(|cursor| !cursor.is_empty()) {
            pairs.append_pair("cursor", cursor);
        }
    }
    let response = http.get(url).timeout(REQUEST_TIMEOUT).send().await?;
    let body: Value = check_http_response(response, "MCP Registry")
        .await?
        .json()
        .await?;
    let servers = body["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            // Deprecated and deleted servers are left out; an entry without
            // a status is shown, since the Registry may stop sending it.
            item["_meta"][STATUS_META]["status"]
                .as_str()
                .is_none_or(|status| status == "active")
        })
        .filter_map(|item| {
            let mut server = item.get("server")?.clone();
            // What a publisher attaches can be large and is not shown.
            server.as_object_mut()?.remove("_meta");
            Some(server)
        })
        .collect();
    Ok(McpRegistryPage {
        servers,
        next_cursor: body["metadata"]["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Query, routing::get};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn search_keeps_active_servers_and_passes_the_query_on() {
        let app = Router::new().route(
            "/v0.1/servers",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                Json(json!({
                    "servers": [
                        {
                            "server": {
                                "name": "io.github.example/docs",
                                "remotes": [{ "type": "streamable-http", "url": "https://docs.example/mcp" }],
                                "params": params,
                                "_meta": { "publisher": "large" }
                            },
                            "_meta": { STATUS_META: { "status": "active" } }
                        },
                        {
                            "server": { "name": "io.github.example/old" },
                            "_meta": { STATUS_META: { "status": "deprecated" } }
                        },
                        { "server": { "name": "io.github.example/new" } }
                    ],
                    "metadata": { "nextCursor": "io.github.example/new:0.2.0", "count": 3 }
                }))
            }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let http = reqwest::Client::builder().no_proxy().build().unwrap();

        let query = McpRegistryQuery {
            query: " docs ".into(),
            cursor: Some("io.github.example/a:1.0.0".into()),
        };
        let page = search(&http, &base_url, &query).await.unwrap();
        let names: Vec<&str> = page
            .servers
            .iter()
            .filter_map(|server| server["name"].as_str())
            .collect();
        assert_eq!(names, ["io.github.example/docs", "io.github.example/new"]);
        assert!(page.servers[0].get("_meta").is_none());
        assert_eq!(
            page.servers[0]["params"],
            json!({
                "search": "docs",
                "version": "latest",
                "limit": "30",
                "cursor": "io.github.example/a:1.0.0"
            })
        );
        assert_eq!(
            page.next_cursor.as_deref(),
            Some("io.github.example/new:0.2.0")
        );

        let long = McpRegistryQuery {
            query: "x".repeat(MAX_QUERY_CHARS + 1),
            cursor: None,
        };
        assert!(search(&http, &base_url, &long).await.is_err());
    }
}
