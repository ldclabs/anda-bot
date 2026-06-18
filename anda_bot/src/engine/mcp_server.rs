use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::{context::BaseCtx, extension::mcp::McpToolProvider};
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::{
    config::{
        McpServerSettings, McpSettings, McpStdioSettings, McpStreamableHttpSettings,
        McpTransportSettings, normalize_string,
    },
    util::text::read_text_file,
};

use super::{backup_daemon_config, daemon_config_needs_backup, write_daemon_config_atomically};

#[derive(Clone)]
pub struct McpServerTool {
    provider: Arc<McpToolProvider>,
    home_dir: PathBuf,
    default_cwd: Option<PathBuf>,
    config_path: PathBuf,
    config_write_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AddMcpServerArgs {
    pub id: String,
    #[serde(default, rename = "type")]
    pub r#type: Option<McpServerTransportType>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub persist: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum McpServerTransportType {
    #[serde(rename = "stdio")]
    Stdio,
    #[serde(rename = "http", alias = "streamable_http")]
    Http,
}

impl McpServerTool {
    pub const NAME: &'static str = "add_mcp_server";

    pub fn new(
        provider: Arc<McpToolProvider>,
        home_dir: PathBuf,
        default_cwd: Option<PathBuf>,
        config_path: PathBuf,
        config_write_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            provider,
            home_dir,
            default_cwd,
            config_path,
            config_write_lock,
        }
    }

    fn server_settings(&self, args: AddMcpServerArgs) -> Result<McpServerSettings, BoxError> {
        let AddMcpServerArgs {
            id,
            r#type,
            command,
            args,
            env,
            cwd,
            url,
            bearer_token,
            headers,
            enabled,
            include,
            exclude,
            persist: _,
        } = args;
        let id = normalize_string(&id).ok_or("MCP server id cannot be empty")?;
        let command_present = command.as_deref().and_then(normalize_string).is_some();
        let url_present = url.as_deref().and_then(normalize_string).is_some();
        let transport = match r#type {
            Some(McpServerTransportType::Stdio) => stdio_transport(command, args, env, cwd)?,
            Some(McpServerTransportType::Http) => http_transport(url, bearer_token, headers)?,
            None if command_present => stdio_transport(command, args, env, cwd)?,
            None if url_present => http_transport(url, bearer_token, headers)?,
            None => {
                return Err(
                    "MCP server type is missing and transport cannot be inferred from command or url"
                        .into(),
                );
            }
        };

        let server = McpServerSettings {
            id,
            disabled: enabled == Some(false),
            transport,
            include: normalize_string_set(include),
            exclude: normalize_string_set(exclude),
        };
        let issues = McpSettings {
            servers: vec![server.clone()],
        }
        .setup_issues();
        if !issues.is_empty() {
            return Err(format!("invalid MCP server configuration: {}", issues.join(", ")).into());
        }
        Ok(server)
    }

    async fn persist_server(&self, server: McpServerSettings) -> Result<(), BoxError> {
        let _guard = self.config_write_lock.lock().await;
        persist_mcp_server_config(&self.config_path, server).await
    }
}

impl Tool<BaseCtx> for McpServerTool {
    type Args = AddMcpServerArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Connects a new MCP server to the current Anda daemon and exposes its tools dynamically. ",
            "Use stdio for local child-process MCP servers and http for remote MCP endpoints. ",
            "Set persist=true only when the server should be written to mcp.json and survive daemon restart. ",
            "Stdio commands are spawned directly without a shell."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: add_mcp_server_parameters(),
            strict: Some(false),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let persist = args.persist;
        let server = self.server_settings(args)?;
        let enabled = !server.disabled;
        if !enabled && !persist {
            return Err("MCP server enabled=false is only useful with persist=true".into());
        }
        if enabled && self.provider.contains_server(&server.id) {
            return Err(format!("MCP server {} already exists", server.id).into());
        }

        let server_id = server.id.clone();
        if enabled {
            let server_configs = McpSettings {
                servers: vec![server.clone()],
            }
            .server_configs(&self.home_dir, self.default_cwd.as_deref())?;
            let server_config = server_configs
                .into_iter()
                .next()
                .ok_or("MCP server configuration was unexpectedly empty")?;
            self.provider.add_server(server_config).await?;
        }

        let mut persisted = false;
        if persist {
            if let Err(err) = self.persist_server(server.clone()).await {
                return Err(format!(
                    "MCP server {server_id} was added for the current daemon, but failed to persist to {}: {err}",
                    self.config_path.display()
                )
                .into());
            }
            persisted = true;
        }

        let tools = self
            .provider
            .routes()
            .into_iter()
            .filter(|route| route.server_id == server_id)
            .map(|route| {
                json!({
                    "name": route.name,
                    "remote_name": route.remote_name,
                    "server_id": route.server_id,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput::new(Response::Ok {
            result: json!({
                "status": if enabled { "added" } else { "saved_disabled" },
                "server_id": server_id,
                "persisted": persisted,
                "enabled": enabled,
                "tools": tools,
            }),
            next_cursor: None,
        }))
    }
}

fn add_mcp_server_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "Stable server id used in local tool names. Example: filesystem, github, browser."
            },
            "type": {
                "type": "string",
                "enum": ["stdio", "http", "streamable_http"],
                "description": "Matches mcp.json server type. Use stdio for a local child process, or http for an HTTP MCP endpoint. Omit to infer from command or url; streamable_http is accepted for compatibility."
            },
            "command": {
                "type": "string",
                "description": "Executable for stdio transport. Required when type is stdio."
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Arguments for stdio transport."
            },
            "env": {
                "type": "object",
                "additionalProperties": { "type": "string" },
                "description": "Additional environment variables for stdio transport, matching mcp.json env object."
            },
            "cwd": {
                "type": "string",
                "description": "Optional working directory for stdio. Relative paths are rooted under ANDA_HOME. Omit to use the first Anda workspace."
            },
            "url": {
                "type": "string",
                "description": "MCP HTTP endpoint URL. Required when type is http."
            },
            "bearer_token": {
                "type": "string",
                "description": "Optional Streamable HTTP bearer token without the Bearer prefix. Prefer headers.Authorization for portable mcp.json-compatible config."
            },
            "headers": {
                "type": "object",
                "additionalProperties": { "type": "string" },
                "description": "Custom HTTP headers for HTTP transport, matching mcp.json headers object."
            },
            "enabled": {
                "type": "boolean",
                "description": "Matches mcp.json enabled. Omit or set true to connect now; set false only with persist=true to save a disabled entry."
            },
            "include": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional remote MCP tool allowlist. Omit to include all tools except excluded ones."
            },
            "exclude": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional remote MCP tool denylist."
            },
            "persist": {
                "type": "boolean",
                "description": "Tool-only option. Set true to also write this server to mcp.json so it survives daemon restart. Defaults to false."
            }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

fn normalize_string_set(values: Vec<String>) -> BTreeSet<String> {
    values
        .into_iter()
        .filter_map(|value| normalize_string(&value))
        .collect()
}

fn stdio_transport(
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<String>,
) -> Result<McpTransportSettings, BoxError> {
    let command = normalize_string(command.as_deref().unwrap_or_default())
        .ok_or("MCP stdio command cannot be empty")?;
    Ok(McpTransportSettings::Stdio(McpStdioSettings {
        command,
        args,
        env: normalize_string_map("env", env)?,
        cwd: cwd.and_then(|cwd| normalize_string(&cwd)),
    }))
}

fn http_transport(
    url: Option<String>,
    bearer_token: Option<String>,
    headers: BTreeMap<String, String>,
) -> Result<McpTransportSettings, BoxError> {
    let url = normalize_string(url.as_deref().unwrap_or_default())
        .ok_or("MCP HTTP URL cannot be empty")?;
    Ok(McpTransportSettings::StreamableHttp(
        McpStreamableHttpSettings {
            url,
            bearer_token: bearer_token.and_then(|token| normalize_string(&token)),
            headers: normalize_string_map("headers", headers)?,
        },
    ))
}

fn normalize_string_map(
    field: &str,
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, BoxError> {
    let mut map = BTreeMap::new();
    for (raw_key, value) in values {
        let key = normalize_string(&raw_key)
            .ok_or_else(|| format!("MCP server {field} entries cannot have an empty key"))?;
        if map.insert(key.clone(), value).is_some() {
            return Err(format!("MCP server {field} contains duplicate key {key}").into());
        }
    }
    Ok(map)
}

async fn persist_mcp_server_config(
    config_path: &Path,
    server: McpServerSettings,
) -> Result<(), BoxError> {
    let content = match read_text_file(config_path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(err) => return Err(err.into()),
    };

    let settings = McpSettings::from_json_contents(&content)?;
    let issues = settings.setup_issues();
    if !issues.is_empty() {
        return Err(format!("invalid mcp.json: {}", issues.join(", ")).into());
    }
    if settings
        .servers
        .iter()
        .any(|existing| existing.id.trim() == server.id)
    {
        return Err(format!("MCP server {} already exists in mcp.json", server.id).into());
    }

    let content = append_mcp_server_to_config_content(&content, &server)?;

    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if daemon_config_needs_backup(config_path, content.as_bytes()).await? {
        backup_daemon_config(config_path).await?;
    }
    write_daemon_config_atomically(config_path, content.as_bytes()).await
}

fn append_mcp_server_to_config_content(
    content: &str,
    server: &McpServerSettings,
) -> Result<String, BoxError> {
    let mut root = if content.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(content)?
    };
    let object = root
        .as_object_mut()
        .ok_or("mcp.json root must be an object")?;

    let root_key = if object.contains_key("mcpServers") {
        "mcpServers"
    } else if object.contains_key("servers") {
        "servers"
    } else {
        object.insert("mcpServers".to_string(), json!({}));
        "mcpServers"
    };

    let servers = object
        .get_mut(root_key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("mcp.json {root_key} must be an object to persist a server"))?;
    if servers.contains_key(&server.id) {
        return Err(format!("MCP server {} already exists in mcp.json", server.id).into());
    }
    servers.insert(server.id.clone(), mcp_server_json(server));

    let mut content = serde_json::to_string_pretty(&root)?;
    content.push('\n');
    Ok(content)
}

fn mcp_server_json(server: &McpServerSettings) -> Value {
    let mut object = Map::new();
    match &server.transport {
        McpTransportSettings::Stdio(stdio) => {
            object.insert("type".to_string(), json!("stdio"));
            object.insert("command".to_string(), json!(stdio.command));
            if !stdio.args.is_empty() {
                object.insert("args".to_string(), json!(stdio.args));
            }
            if !stdio.env.is_empty() {
                object.insert("env".to_string(), string_map_json(&stdio.env));
            }
            if let Some(cwd) = &stdio.cwd {
                object.insert("cwd".to_string(), json!(cwd));
            }
        }
        McpTransportSettings::StreamableHttp(http) => {
            object.insert("type".to_string(), json!("http"));
            object.insert("url".to_string(), json!(http.url));
            let mut headers = http.headers.clone();
            if let Some(token) = &http.bearer_token {
                let has_authorization = headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization"));
                if !has_authorization {
                    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
                }
            }
            if !headers.is_empty() {
                object.insert("headers".to_string(), string_map_json(&headers));
            }
        }
    }

    if server.disabled {
        object.insert("enabled".to_string(), json!(false));
    }
    if !server.include.is_empty() {
        object.insert("include".to_string(), json!(server.include));
    }
    if !server.exclude.is_empty() {
        object.insert("exclude".to_string(), json!(server.exclude));
    }

    Value::Object(object)
}

fn string_map_json(values: &BTreeMap<String, String>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(key, value)| (key.clone(), json!(value)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::extension::mcp::McpServerConfig;

    fn test_tool() -> McpServerTool {
        let provider = Arc::new(McpToolProvider::new(Vec::new()).unwrap());
        let home = PathBuf::from("/tmp/anda-home");
        McpServerTool::new(
            provider,
            home.clone(),
            Some(home.join("workspace")),
            McpSettings::file_path(&home),
            Arc::new(Mutex::new(())),
        )
    }

    #[test]
    fn add_mcp_server_schema_matches_mcp_json_server_shape() {
        let tool = test_tool();
        let definition = tool.definition();
        assert_eq!(definition.strict, Some(false));

        let properties = definition
            .parameters
            .get("properties")
            .and_then(Value::as_object)
            .unwrap();
        assert!(properties.get("transport_type").is_none());
        assert!(properties.get("type").is_some());
        assert_eq!(properties["env"]["type"], "object");
        assert_eq!(properties["env"]["additionalProperties"]["type"], "string");
        assert_eq!(properties["headers"]["type"], "object");
        assert_eq!(
            properties["headers"]["additionalProperties"]["type"],
            "string"
        );
        assert_eq!(properties["enabled"]["type"], "boolean");
    }

    #[test]
    fn add_mcp_server_args_convert_to_stdio_settings() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: " filesystem ".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some(" npx ".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                ],
                env: BTreeMap::from([(" TOKEN ".to_string(), "secret".to_string())]),
                cwd: Some(" workspace ".to_string()),
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: None,
                include: vec![" read_file ".to_string(), " ".to_string()],
                exclude: vec!["write_file".to_string()],
                persist: false,
            })
            .unwrap();

        assert_eq!(server.id, "filesystem");
        assert_eq!(server.include, BTreeSet::from(["read_file".to_string()]));
        assert_eq!(server.exclude, BTreeSet::from(["write_file".to_string()]));
        match server.transport {
            McpTransportSettings::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(stdio.cwd.as_deref(), Some("workspace"));
                assert_eq!(stdio.env.get("TOKEN").map(String::as_str), Some("secret"));
            }
            _ => panic!("expected stdio"),
        }
    }

    #[test]
    fn add_mcp_server_args_convert_to_http_settings() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: "remote".to_string(),
                r#type: Some(McpServerTransportType::Http),
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: Some(" https://mcp.example.test/mcp ".to_string()),
                bearer_token: Some(" token ".to_string()),
                headers: BTreeMap::from([("x-client".to_string(), "anda".to_string())]),
                enabled: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            })
            .unwrap();

        match server.transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
                assert_eq!(http.bearer_token.as_deref(), Some("token"));
                assert_eq!(
                    http.headers.get("x-client").map(String::as_str),
                    Some("anda")
                );
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn add_mcp_server_args_infer_transport_and_enabled_false() {
        let tool = test_tool();
        let server = tool
            .server_settings(AddMcpServerArgs {
                id: "remote".to_string(),
                r#type: None,
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: Some("https://mcp.example.test/mcp".to_string()),
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: Some(false),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            })
            .unwrap();

        assert!(server.disabled);
        match server.transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[tokio::test]
    async fn persist_mcp_server_config_appends_server() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(&config_path, "{\n  \"note\": true\n}\n")
            .await
            .unwrap();

        persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let settings = McpSettings::from_file(dir.path()).await.unwrap();
        assert_eq!(settings.servers.len(), 1);
        assert_eq!(settings.servers[0].id, "remote");

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["note"], true);
        assert_eq!(json["mcpServers"]["remote"]["type"], "http");
        assert_eq!(
            json["mcpServers"]["remote"]["url"],
            "https://mcp.example.test/mcp"
        );
    }

    #[tokio::test]
    async fn persist_mcp_server_config_preserves_existing_servers_root() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(
            &config_path,
            r#"{
  "servers": {
    "existing": {
      "type": "stdio",
      "command": "existing-mcp"
    }
  },
  "other": 1
}
"#,
        )
        .await
        .unwrap();

        persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["other"], 1);
        assert!(json.get("mcpServers").is_none());
        assert_eq!(json["servers"]["existing"]["command"], "existing-mcp");
        assert_eq!(json["servers"]["remote"]["type"], "http");

        let settings = McpSettings::from_file(dir.path()).await.unwrap();
        assert_eq!(settings.servers.len(), 2);
    }

    #[tokio::test]
    async fn persist_mcp_server_config_rejects_non_object_root() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = McpSettings::file_path(dir.path());
        tokio::fs::write(&config_path, "[]\n").await.unwrap();

        let err = persist_mcp_server_config(
            &config_path,
            McpServerSettings {
                id: "remote".to_string(),
                transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: "https://mcp.example.test/mcp".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("root must be an object"));
    }

    #[tokio::test]
    async fn call_persists_disabled_server_without_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let tool = McpServerTool::new(
            Arc::new(McpToolProvider::new(Vec::new()).unwrap()),
            dir.path().to_path_buf(),
            None,
            McpSettings::file_path(dir.path()),
            Arc::new(Mutex::new(())),
        );

        let output = Tool::call(
            &tool,
            anda_engine::engine::EngineBuilder::new().mock_ctx().base,
            AddMcpServerArgs {
                id: "disabled".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some("missing-command".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: Some(false),
                include: Vec::new(),
                exclude: Vec::new(),
                persist: true,
            },
            Vec::new(),
        )
        .await
        .unwrap();

        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["status"], "saved_disabled");
                assert_eq!(result["enabled"], false);
            }
            _ => panic!("expected ok response"),
        }
        assert!(tool.provider.routes().is_empty());

        let content = tokio::fs::read_to_string(McpSettings::file_path(dir.path()))
            .await
            .unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["mcpServers"]["disabled"]["enabled"], false);
        assert_eq!(json["mcpServers"]["disabled"]["command"], "missing-command");
    }

    #[tokio::test]
    async fn call_rejects_duplicate_runtime_server_before_connecting() {
        let provider = Arc::new(
            McpToolProvider::new(vec![McpServerConfig::stdio("dupe", "missing-command")]).unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let tool = McpServerTool::new(
            provider,
            dir.path().to_path_buf(),
            None,
            McpSettings::file_path(dir.path()),
            Arc::new(Mutex::new(())),
        );

        let err = Tool::call(
            &tool,
            anda_engine::engine::EngineBuilder::new().mock_ctx().base,
            AddMcpServerArgs {
                id: "dupe".to_string(),
                r#type: Some(McpServerTransportType::Stdio),
                command: Some("missing-command".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                bearer_token: None,
                headers: BTreeMap::new(),
                enabled: None,
                include: Vec::new(),
                exclude: Vec::new(),
                persist: false,
            },
            Vec::new(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("already exists"));
    }
}
