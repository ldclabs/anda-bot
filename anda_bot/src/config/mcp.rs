use anda_core::BoxError;
use anda_engine::extension::mcp::{
    McpServerConfig, McpStdioTransport, McpStreamableHttpTransport, McpTransportConfig,
};
use http::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::util::text::read_text_file;

use super::normalize_string;

pub const MCP_CONFIG_FILE_NAME: &str = "mcp.json";

/// MCP host/client configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpSettings {
    /// MCP servers exposed as dynamic Anda tools.
    #[serde(default)]
    pub servers: Vec<McpServerSettings>,
}

impl McpSettings {
    pub fn file_path(home_dir: &Path) -> PathBuf {
        home_dir.join(MCP_CONFIG_FILE_NAME)
    }

    pub async fn from_file(home_dir: &Path) -> Result<Self, BoxError> {
        let path = Self::file_path(home_dir);
        let content = match read_text_file(&path).await {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err.into()),
        };
        let settings = Self::from_json_contents(&content)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let issues = settings.setup_issues();
        if !issues.is_empty() {
            return Err(format!(
                "invalid MCP config {}: {}",
                path.display(),
                issues.join(", ")
            )
            .into());
        }
        Ok(settings)
    }

    pub fn from_json_contents(content: &str) -> Result<Self, BoxError> {
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        let file: McpJsonRoot = serde_json::from_str(content)?;
        file.into_settings()
    }

    pub fn setup_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let mut seen_ids = BTreeSet::new();
        let vars = McpExpansionVars::validation();

        for (index, server) in self.servers.iter().enumerate() {
            if server.disabled {
                continue;
            }

            let base = format!("mcp.json.servers[{index}]");
            let server_id = server.id.trim();
            if server_id.is_empty() || !seen_ids.insert(server_id.to_string()) {
                issues.push(format!("{base}.id"));
            }

            server.transport.setup_issues(&base, &vars, &mut issues);
        }

        issues
    }

    pub fn server_configs(
        &self,
        home_dir: &Path,
        default_cwd: Option<&Path>,
    ) -> Result<Vec<McpServerConfig>, BoxError> {
        let vars = McpExpansionVars::new(home_dir, default_cwd);
        self.servers
            .iter()
            .enumerate()
            .filter(|(_, server)| !server.disabled)
            .map(|(index, server)| server.to_server_config(index, &vars, default_cwd))
            .collect()
    }
}

#[derive(Debug, Default, Deserialize)]
struct McpJsonRoot {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: McpJsonServers,
    #[serde(default)]
    servers: McpJsonServers,
}

impl McpJsonRoot {
    fn into_settings(self) -> Result<McpSettings, BoxError> {
        let mut servers = self.mcp_servers.into_servers("mcpServers")?;
        servers.extend(self.servers.into_servers("servers")?);
        Ok(McpSettings { servers })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum McpJsonServers {
    Map(BTreeMap<String, McpJsonServer>),
    LegacyList(Vec<McpServerSettings>),
}

impl Default for McpJsonServers {
    fn default() -> Self {
        Self::Map(BTreeMap::new())
    }
}

impl McpJsonServers {
    fn into_servers(self, root: &str) -> Result<Vec<McpServerSettings>, BoxError> {
        match self {
            Self::Map(servers) => servers
                .into_iter()
                .map(|(id, server)| server.into_settings(root, id))
                .collect(),
            Self::LegacyList(servers) => Ok(servers),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct McpJsonServer {
    #[serde(default, rename = "type")]
    transport_type: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default, alias = "environment")]
    env: BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    include: BTreeSet<String>,
    #[serde(default)]
    exclude: BTreeSet<String>,
}

impl McpJsonServer {
    fn into_settings(self, root: &str, id: String) -> Result<McpServerSettings, BoxError> {
        let Self {
            transport_type,
            command,
            args,
            env,
            cwd,
            url,
            bearer_token,
            headers,
            enabled,
            disabled,
            include,
            exclude,
        } = self;

        let disabled = disabled || enabled == Some(false);
        let transport_type = transport_type
            .as_deref()
            .and_then(normalize_string)
            .map(|value| value.to_ascii_lowercase());
        let transport = match transport_type.as_deref() {
            Some("stdio") => McpTransportSettings::Stdio(McpStdioSettings {
                command: command.unwrap_or_default(),
                args,
                env,
                cwd,
            }),
            Some("http") | Some("streamable_http") => {
                McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                    url: url.unwrap_or_default(),
                    bearer_token,
                    headers,
                })
            }
            Some(other) => {
                return Err(format!(
                    "mcp.json.{root}.{id}.type has unsupported transport {other:?}"
                )
                .into());
            }
            None => {
                if command.as_deref().and_then(normalize_string).is_some() {
                    McpTransportSettings::Stdio(McpStdioSettings {
                        command: command.unwrap_or_default(),
                        args,
                        env,
                        cwd,
                    })
                } else if url.as_deref().and_then(normalize_string).is_some() {
                    McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                        url: url.unwrap_or_default(),
                        bearer_token,
                        headers,
                    })
                } else {
                    return Err(format!(
                        "mcp.json.{root}.{id}.type is missing and transport cannot be inferred"
                    )
                    .into());
                }
            }
        };

        Ok(McpServerSettings {
            id,
            disabled,
            transport,
            include,
            exclude,
        })
    }
}

/// One MCP server entry.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpServerSettings {
    /// Stable server id used in local tool names and audit output.
    #[serde(default)]
    pub id: String,
    /// Temporarily skip this server without deleting the entry.
    #[serde(default)]
    pub disabled: bool,
    /// Server transport.
    #[serde(default)]
    pub transport: McpTransportSettings,
    /// Optional remote tool allowlist. Empty means all tools except excluded.
    #[serde(default)]
    pub include: BTreeSet<String>,
    /// Optional remote tool denylist.
    #[serde(default)]
    pub exclude: BTreeSet<String>,
}

impl McpServerSettings {
    fn to_server_config(
        &self,
        index: usize,
        vars: &McpExpansionVars,
        default_cwd: Option<&Path>,
    ) -> Result<McpServerConfig, BoxError> {
        let base = format!("mcp.json.servers[{index}]");
        Ok(McpServerConfig {
            id: self.id.trim().to_string(),
            transport: self
                .transport
                .to_transport_config(&base, vars, default_cwd)?,
            include: self.include.clone(),
            exclude: self.exclude.clone(),
        })
    }
}

/// MCP transport configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum McpTransportSettings {
    /// stdio child process transport.
    #[serde(rename = "stdio")]
    Stdio(McpStdioSettings),
    /// Streamable HTTP transport.
    #[serde(rename = "http", alias = "streamable_http")]
    StreamableHttp(McpStreamableHttpSettings),
}

impl McpTransportSettings {
    fn setup_issues(&self, base: &str, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        match self {
            Self::Stdio(stdio) => stdio.setup_issues(base, vars, issues),
            Self::StreamableHttp(http) => http.setup_issues(base, vars, issues),
        }
    }

    fn to_transport_config(
        &self,
        base: &str,
        vars: &McpExpansionVars,
        default_cwd: Option<&Path>,
    ) -> Result<McpTransportConfig, BoxError> {
        match self {
            Self::Stdio(stdio) => stdio
                .to_transport(base, vars, default_cwd)
                .map(McpTransportConfig::Stdio),
            Self::StreamableHttp(http) => http
                .to_transport(base, vars)
                .map(McpTransportConfig::StreamableHttp),
        }
    }
}

impl Default for McpTransportSettings {
    fn default() -> Self {
        Self::Stdio(McpStdioSettings::default())
    }
}

/// stdio MCP server transport.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpStdioSettings {
    /// Executable to spawn. It is passed directly, not through a shell.
    #[serde(default)]
    pub command: String,
    /// Command arguments. These are passed without shell interpolation.
    #[serde(default)]
    pub args: Vec<String>,
    /// Additional environment variables for the child process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Optional working directory. Relative paths are rooted under ANDA_HOME.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl McpStdioSettings {
    fn setup_issues(&self, base: &str, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        if self.command.trim().is_empty() {
            issues.push(format!("{base}.transport.command"));
        }
        push_expansion_issues(
            &self.command,
            &format!("{base}.transport.command"),
            vars,
            issues,
        );
        for (arg_index, arg) in self.args.iter().enumerate() {
            push_expansion_issues(
                arg,
                &format!("{base}.transport.args[{arg_index}]"),
                vars,
                issues,
            );
        }
        for (name, value) in &self.env {
            if name.trim().is_empty() {
                issues.push(format!("{base}.transport.env"));
            }
            push_expansion_issues(value, &format!("{base}.transport.env.{name}"), vars, issues);
        }
        if let Some(cwd) = &self.cwd {
            push_expansion_issues(cwd, &format!("{base}.transport.cwd"), vars, issues);
        }
    }

    fn to_transport(
        &self,
        base: &str,
        vars: &McpExpansionVars,
        default_cwd: Option<&Path>,
    ) -> Result<McpStdioTransport, BoxError> {
        let cwd = match self.cwd.as_deref().and_then(normalize_string) {
            Some(cwd) => Some(resolve_config_path(
                &expand_config_string(&cwd, vars, &format!("{base}.transport.cwd"))?,
                vars.home_dir,
            )),
            None => default_cwd.map(Path::to_path_buf),
        };

        Ok(McpStdioTransport {
            command: expand_config_string(
                self.command.trim(),
                vars,
                &format!("{base}.transport.command"),
            )?,
            args: self
                .args
                .iter()
                .enumerate()
                .map(|(arg_index, arg)| {
                    expand_config_string(arg, vars, &format!("{base}.transport.args[{arg_index}]"))
                })
                .collect::<Result<_, _>>()?,
            env: self
                .env
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.trim().to_string(),
                        expand_config_string(value, vars, &format!("{base}.transport.env.{key}"))?,
                    ))
                })
                .collect::<Result<_, BoxError>>()?,
            cwd,
        })
    }
}

/// Streamable HTTP MCP server transport.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpStreamableHttpSettings {
    /// MCP endpoint URL.
    #[serde(default)]
    pub url: String,
    /// Bearer token value, without the `Bearer ` prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    /// Custom HTTP headers sent with every request.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl McpStreamableHttpSettings {
    fn setup_issues(&self, base: &str, vars: &McpExpansionVars, issues: &mut Vec<String>) {
        if self.url.trim().is_empty() {
            issues.push(format!("{base}.transport.url"));
        }
        push_expansion_issues(&self.url, &format!("{base}.transport.url"), vars, issues);
        if let Some(token) = &self.bearer_token {
            push_expansion_issues(
                token,
                &format!("{base}.transport.bearer_token"),
                vars,
                issues,
            );
        }
        for (name, value) in &self.headers {
            let field = format!("{base}.transport.headers.{name}");
            if HeaderName::from_bytes(name.as_bytes()).is_err() {
                issues.push(field.clone());
            }
            match expand_config_string(value, vars, &field) {
                Ok(expanded) => {
                    if HeaderValue::from_str(&expanded).is_err() {
                        issues.push(field);
                    }
                }
                Err(_) => issues.push(field),
            }
        }
    }

    fn to_transport(
        &self,
        base: &str,
        vars: &McpExpansionVars,
    ) -> Result<McpStreamableHttpTransport, BoxError> {
        Ok(McpStreamableHttpTransport {
            url: expand_config_string(self.url.trim(), vars, &format!("{base}.transport.url"))?,
            bearer_token: self
                .bearer_token
                .as_deref()
                .and_then(normalize_string)
                .map(|token| {
                    expand_config_string(&token, vars, &format!("{base}.transport.bearer_token"))
                })
                .transpose()?,
            headers: self
                .headers
                .iter()
                .map(|(key, value)| {
                    HeaderName::from_bytes(key.as_bytes())?;
                    let value = expand_config_string(
                        value,
                        vars,
                        &format!("{base}.transport.headers.{key}"),
                    )?;
                    HeaderValue::from_str(&value)?;
                    Ok((key.trim().to_string(), value))
                })
                .collect::<Result<_, BoxError>>()?,
        })
    }
}

struct McpExpansionVars<'a> {
    home_dir: &'a Path,
    default_cwd: Option<&'a Path>,
    validate_only: bool,
}

impl<'a> McpExpansionVars<'a> {
    fn new(home_dir: &'a Path, default_cwd: Option<&'a Path>) -> Self {
        Self {
            home_dir,
            default_cwd,
            validate_only: false,
        }
    }

    fn validation() -> Self {
        Self {
            home_dir: Path::new(""),
            default_cwd: None,
            validate_only: true,
        }
    }

    fn get(&self, name: &str) -> Option<String> {
        match name {
            "ANDA_HOME" => {
                (!self.validate_only).then(|| self.home_dir.to_string_lossy().to_string())
            }
            "ANDA_WORKSPACE" => self
                .default_cwd
                .filter(|_| !self.validate_only)
                .map(|path| path.to_string_lossy().to_string()),
            _ => std::env::var(name).ok(),
        }
    }

    fn is_known_builtin(&self, name: &str) -> bool {
        self.validate_only && matches!(name, "ANDA_HOME" | "ANDA_WORKSPACE")
    }
}

fn push_expansion_issues(
    value: &str,
    field: &str,
    vars: &McpExpansionVars<'_>,
    issues: &mut Vec<String>,
) {
    if expand_config_string(value, vars, field).is_err() {
        issues.push(field.to_string());
    }
}

fn expand_config_string(
    value: &str,
    vars: &McpExpansionVars<'_>,
    field: &str,
) -> Result<String, BoxError> {
    let mut out = String::with_capacity(value.len());
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '$' {
            out.push(chars[index]);
            index += 1;
            continue;
        }

        let Some(next) = chars.get(index + 1).copied() else {
            out.push('$');
            index += 1;
            continue;
        };

        if next == '{' {
            let mut end = index + 2;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end >= chars.len() {
                return Err(
                    format!("{field} contains an unterminated environment reference").into(),
                );
            }
            let name = chars[index + 2..end].iter().collect::<String>();
            out.push_str(&expand_env_reference(&name, vars, field)?);
            index = end + 1;
            continue;
        }

        if !is_env_name_start(next) {
            out.push('$');
            index += 1;
            continue;
        }

        let mut end = index + 2;
        while end < chars.len() && is_env_name_char(chars[end]) {
            end += 1;
        }
        let name = chars[index + 1..end].iter().collect::<String>();
        out.push_str(&expand_env_reference(&name, vars, field)?);
        index = end;
    }

    Ok(out)
}

fn expand_env_reference(
    name: &str,
    vars: &McpExpansionVars<'_>,
    field: &str,
) -> Result<String, BoxError> {
    if name.is_empty()
        || !name.chars().next().is_some_and(is_env_name_start)
        || !name.chars().all(is_env_name_char)
    {
        return Err(format!("{field} contains an invalid environment reference").into());
    }
    if let Some(value) = vars.get(name) {
        return Ok(value);
    }
    if vars.is_known_builtin(name) {
        return Ok(String::new());
    }
    Err(format!("{field} references missing environment variable {name}").into())
}

fn is_env_name_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_env_name_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn resolve_config_path(path: &str, home_dir: &Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        home_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MCP_TEST_ENV_VARS: &[&str] = &["ANDA_MCP_TEST_TOKEN", "ANDA_MCP_TEST_PATH"];

    struct EnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new() -> Self {
            static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
            let lock = LOCK
                .get_or_init(|| std::sync::Mutex::new(()))
                .lock()
                .unwrap();
            let saved = MCP_TEST_ENV_VARS
                .iter()
                .map(|&name| (name, std::env::var_os(name)))
                .collect();
            for &name in MCP_TEST_ENV_VARS {
                unsafe { std::env::remove_var(name) };
            }
            Self { saved, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for &name in MCP_TEST_ENV_VARS {
                unsafe { std::env::remove_var(name) };
            }
            for (name, value) in &self.saved {
                if let Some(value) = value {
                    unsafe { std::env::set_var(name, value) };
                }
            }
        }
    }

    #[test]
    fn mcp_json_accepts_mcp_servers_root_and_standard_http() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "github": {
                  "type": "stdio",
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-github"],
                  "environment": {
                    "GITHUB_TOKEN": "ghp_xxx"
                  }
                },
                "remote-server": {
                  "type": "http",
                  "url": "https://mcp.example.com/api",
                  "headers": {
                    "Authorization": "Bearer token"
                  }
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.servers.len(), 2);
        assert_eq!(settings.servers[0].id, "github");
        match &settings.servers[0].transport {
            McpTransportSettings::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(
                    stdio.env.get("GITHUB_TOKEN").map(String::as_str),
                    Some("ghp_xxx")
                );
            }
            _ => panic!("expected stdio"),
        }

        assert_eq!(settings.servers[1].id, "remote-server");
        match &settings.servers[1].transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.com/api");
                assert_eq!(
                    http.headers.get("Authorization").map(String::as_str),
                    Some("Bearer token")
                );
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn mcp_json_accepts_servers_root_and_infers_transport() {
        let settings = McpSettings::from_json_contents(
            r#"{
              "servers": {
                "filesystem": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-filesystem", "$ANDA_WORKSPACE"]
                },
                "remote": {
                  "url": "https://mcp.example.test/mcp",
                  "enabled": false
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.servers.len(), 2);
        assert_eq!(settings.servers[0].id, "filesystem");
        assert!(!settings.servers[0].disabled);
        assert_eq!(settings.servers[1].id, "remote");
        assert!(settings.servers[1].disabled);
        match &settings.servers[1].transport {
            McpTransportSettings::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
            }
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn mcp_json_reports_unknown_transport() {
        let err = McpSettings::from_json_contents(
            r#"{
              "mcpServers": {
                "legacy": {
                  "type": "sse",
                  "url": "https://mcp.example.test/sse"
                }
              }
            }"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unsupported transport"));
    }

    #[test]
    fn mcp_server_configs_expand_env_and_default_cwd() {
        let _env = EnvGuard::new();
        unsafe { std::env::set_var("ANDA_MCP_TEST_TOKEN", "token-1") };
        unsafe { std::env::set_var("ANDA_MCP_TEST_PATH", "project-a") };

        let settings = McpSettings {
            servers: vec![
                McpServerSettings {
                    id: "fs".to_string(),
                    transport: McpTransportSettings::Stdio(McpStdioSettings {
                        command: "npx".to_string(),
                        args: vec![
                            "-y".to_string(),
                            "@modelcontextprotocol/server-filesystem".to_string(),
                            "$ANDA_HOME/$ANDA_MCP_TEST_PATH".to_string(),
                        ],
                        env: BTreeMap::from([(
                            "TOKEN".to_string(),
                            "${ANDA_MCP_TEST_TOKEN}".to_string(),
                        )]),
                        cwd: None,
                    }),
                    ..Default::default()
                },
                McpServerSettings {
                    id: "remote".to_string(),
                    transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                        url: "https://mcp.example.test/mcp".to_string(),
                        bearer_token: Some("$ANDA_MCP_TEST_TOKEN".to_string()),
                        headers: BTreeMap::from([(
                            "x-anda-home".to_string(),
                            "$ANDA_HOME".to_string(),
                        )]),
                    }),
                    include: BTreeSet::from(["search".to_string()]),
                    ..Default::default()
                },
            ],
        };

        assert!(settings.setup_issues().is_empty());

        let home = Path::new("/tmp/anda-home");
        let workspace = home.join("workspace");
        let servers = settings.server_configs(home, Some(&workspace)).unwrap();
        assert_eq!(servers.len(), 2);

        match &servers[0].transport {
            McpTransportConfig::Stdio(stdio) => {
                assert_eq!(stdio.command, "npx");
                assert_eq!(stdio.args[2], "/tmp/anda-home/project-a");
                assert_eq!(stdio.env.get("TOKEN").map(String::as_str), Some("token-1"));
                assert_eq!(stdio.cwd.as_deref(), Some(workspace.as_path()));
            }
            _ => panic!("expected stdio transport"),
        }

        match &servers[1].transport {
            McpTransportConfig::StreamableHttp(http) => {
                assert_eq!(http.url, "https://mcp.example.test/mcp");
                assert_eq!(http.bearer_token.as_deref(), Some("token-1"));
                assert_eq!(
                    http.headers.get("x-anda-home").map(String::as_str),
                    Some("/tmp/anda-home")
                );
            }
            _ => panic!("expected streamable HTTP transport"),
        }
        assert_eq!(servers[1].include, BTreeSet::from(["search".to_string()]));
    }

    #[test]
    fn mcp_setup_issues_report_missing_fields_and_env_refs() {
        let _env = EnvGuard::new();
        let settings = McpSettings {
            servers: vec![
                McpServerSettings {
                    transport: McpTransportSettings::Stdio(McpStdioSettings {
                        command: "${ANDA_MCP_TEST_TOKEN}".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                McpServerSettings {
                    id: "remote".to_string(),
                    transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                        url: String::new(),
                        headers: BTreeMap::from([("bad header".to_string(), "ok".to_string())]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
        };

        let issues = settings.setup_issues();
        for expected in [
            "mcp.json.servers[0].id",
            "mcp.json.servers[0].transport.command",
            "mcp.json.servers[1].transport.url",
            "mcp.json.servers[1].transport.headers.bad header",
        ] {
            assert!(
                issues.iter().any(|issue| issue == expected),
                "missing issue {expected:?} in {issues:?}"
            );
        }
    }

    #[test]
    fn mcp_disabled_servers_are_ignored() {
        let settings = McpSettings {
            servers: vec![McpServerSettings {
                disabled: true,
                ..Default::default()
            }],
        };

        assert!(settings.setup_issues().is_empty());
        assert!(
            settings
                .server_configs(Path::new("/tmp/anda-home"), None)
                .unwrap()
                .is_empty()
        );
    }
}
