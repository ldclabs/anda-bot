//! mcp.json reads and edits.
//!
//! A change edits the file's own JSON instead of writing parsed settings back,
//! so what Anda does not model survives it: unknown fields, raw `${VAR}`
//! references, the root the file uses (`mcpServers`, or the older `servers`
//! as an object or a list) and entries that were skipped as invalid.

use anda_core::BoxError;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tokio::sync::Mutex;

use super::McpError;
use crate::{
    config::{
        McpApproval, McpOAuthSettings, McpServerOptions, McpServerSettings, McpSettings,
        McpTimeoutSettings, McpTransportSettings,
    },
    engine::{backup_daemon_config, daemon_config_revision, write_daemon_config_atomically},
    util::text::read_text_file,
};

/// The roots a server list can live under, in the order they are read.
const ROOTS: [&str; 2] = ["mcpServers", "servers"];

/// Fields of an `mcpServers` entry that Anda reads; anything else is kept.
const ENTRY_FIELDS: &[&str] = &[
    "type",
    "command",
    "args",
    "env",
    "environment",
    "cwd",
    "url",
    "bearer_token",
    "headers",
    "oauth",
    "enabled",
    "disabled",
    "include",
    "exclude",
    "lifecycle",
    "startup",
    "tasks",
    "approval",
    "allow_external_users",
    "inherit_env",
    "timeouts",
    "concurrency",
    "limits",
];

/// Fields of an entry in the older list form.
const LIST_ENTRY_FIELDS: &[&str] = &[
    "id",
    "disabled",
    "transport",
    "include",
    "exclude",
    "lifecycle",
    "startup",
    "tasks",
    "approval",
    "allow_external_users",
    "timeouts",
    "concurrency",
    "limits",
];

/// mcp.json as it was read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct McpConfigFile {
    /// `None` when the file does not exist.
    pub content: Option<String>,
    /// Digest of the content, empty when the file does not exist. A change
    /// can carry the revision it was based on, and is refused when the file
    /// changed since.
    pub revision: String,
}

impl McpConfigFile {
    pub fn new(content: Option<String>) -> Self {
        let revision = content
            .as_deref()
            .map(daemon_config_revision)
            .unwrap_or_default();
        Self { content, revision }
    }

    pub async fn read(path: &Path) -> Result<Self, BoxError> {
        match read_text_file(path).await {
            Ok(content) => Ok(Self::new(Some(content))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn text(&self) -> &str {
        self.content.as_deref().unwrap_or_default()
    }

    /// The parsed root, or an empty object when the file is missing or is
    /// not a JSON object.
    pub fn root(&self) -> Value {
        serde_json::from_str::<Value>(self.text())
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}))
    }
}

/// One change to mcp.json.
pub(crate) enum McpFileEdit<'a> {
    /// Adds a server; refused when the id is already declared.
    Add(&'a McpServerSettings),
    /// Adds servers together; refused when any of their ids is declared, or
    /// used twice among them.
    AddAll(&'a [McpServerSettings]),
    /// Replaces the fields Anda reads of an existing entry.
    Replace(&'a McpServerSettings),
    /// Removes every entry declaring the id.
    Remove(&'a str),
    SetEnabled(&'a str, bool),
    SetToolVisible {
        id: &'a str,
        tool: &'a str,
        visible: bool,
    },
    /// Records the OAuth marker of an authorized server.
    SetOAuth(&'a str, &'a McpOAuthSettings),
    /// Sets the approval policy of one tool, or with no tool the server's
    /// default. `None` removes it, so the tool follows the server's default
    /// and the server falls back to `auto`.
    SetApproval {
        id: &'a str,
        tool: Option<&'a str>,
        approval: Option<McpApproval>,
    },
    SetExternalUsers(&'a str, bool),
    /// Replaces the advanced settings, keeping what Anda does not read
    /// inside `timeouts` and `limits`.
    SetOptions(&'a str, &'a McpServerOptions),
}

/// Reads mcp.json, applies `edit`, and writes the result atomically, backing
/// up the previous file, all under `lock`. With `expected_revision`, a file
/// that changed since the caller read it is left alone.
pub(crate) async fn edit(
    path: &Path,
    lock: &Mutex<()>,
    expected_revision: Option<&str>,
    edit: McpFileEdit<'_>,
) -> Result<McpConfigFile, BoxError> {
    let _guard = lock.lock().await;
    let current = McpConfigFile::read(path).await?;
    if let Some(expected) = expected_revision
        && expected != current.revision
    {
        return Err(McpError::conflict(
            "mcp.json changed since it was read; reload it and try again",
        ));
    }
    let next = apply_edit(current.text(), edit)?;
    if current.content.as_deref() != Some(next.as_str()) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if current.content.is_some() {
            backup_daemon_config(path).await?;
        }
        write_daemon_config_atomically(path, next.as_bytes()).await?;
    }
    Ok(McpConfigFile::new(Some(next)))
}

/// Applies `edit` to mcp.json `content` and returns the new contents.
pub(crate) fn apply_edit(content: &str, edit: McpFileEdit<'_>) -> Result<String, BoxError> {
    let mut root = if content.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(content)?
    };
    if !root.is_object() {
        return Err("mcp.json root must be an object".into());
    }

    match edit {
        McpFileEdit::Add(server) => add_entries(&mut root, content, std::slice::from_ref(server))?,
        McpFileEdit::AddAll(servers) => add_entries(&mut root, content, servers)?,
        McpFileEdit::Replace(server) => {
            let (entry, form) = entry_mut(&mut root, &server.id)?;
            let mut next = match form {
                Form::Map => entry_json(server),
                Form::List => list_entry_json(server)?,
            };
            let known = match form {
                Form::Map => ENTRY_FIELDS,
                Form::List => LIST_ENTRY_FIELDS,
            };
            for (key, value) in entry.iter() {
                if !known.contains(&key.as_str()) && !next.contains_key(key) {
                    next.insert(key.clone(), value.clone());
                }
            }
            *entry = next;
        }
        McpFileEdit::Remove(id) => {
            let mut removed = false;
            for key in ROOTS {
                match root.get_mut(key) {
                    Some(Value::Object(servers)) => {
                        let before = servers.len();
                        servers.retain(|name, _| name.trim() != id);
                        removed |= servers.len() != before;
                    }
                    Some(Value::Array(servers)) => {
                        let before = servers.len();
                        servers.retain(|entry| entry_id(entry) != Some(id));
                        removed |= servers.len() != before;
                    }
                    _ => {}
                }
            }
            if !removed {
                return Err(McpError::not_found(id));
            }
        }
        McpFileEdit::SetEnabled(id, enabled) => {
            let (entry, form) = entry_mut(&mut root, id)?;
            match (form, enabled) {
                (Form::Map, true) => {
                    entry.remove("enabled");
                    entry.remove("disabled");
                }
                (Form::Map, false) => {
                    entry.remove("disabled");
                    entry.insert("enabled".into(), json!(false));
                }
                (Form::List, true) => {
                    entry.remove("disabled");
                }
                (Form::List, false) => {
                    entry.insert("disabled".into(), json!(true));
                }
            }
        }
        McpFileEdit::SetToolVisible { id, tool, visible } => {
            let (entry, _) = entry_mut(&mut root, id)?;
            let mut include = string_set(entry.get("include"));
            let mut exclude = string_set(entry.get("exclude"));
            set_tool_visible(&mut include, &mut exclude, tool, visible);
            for (key, values) in [("include", include), ("exclude", exclude)] {
                if values.is_empty() {
                    entry.remove(key);
                } else {
                    entry.insert(key.into(), json!(values));
                }
            }
        }
        McpFileEdit::SetOAuth(id, oauth) => {
            let (entry, _) = entry_mut(&mut root, id)?;
            // The older list form nests the transport fields.
            let transport = match entry.get_mut("transport") {
                Some(Value::Object(transport)) => transport,
                _ => entry,
            };
            transport.insert("oauth".into(), serde_json::to_value(oauth)?);
            transport.remove("bearer_token");
        }
        McpFileEdit::SetApproval { id, tool, approval } => {
            let (entry, _) = entry_mut(&mut root, id)?;
            let mut policy = match entry.remove("approval") {
                Some(Value::Object(policy)) => policy,
                _ => Map::new(),
            };
            match tool {
                None => set_or_remove(&mut policy, "default", approval),
                Some(tool) => {
                    let mut tools = match policy.remove("tools") {
                        Some(Value::Object(tools)) => tools,
                        _ => Map::new(),
                    };
                    set_or_remove(&mut tools, tool, approval);
                    if !tools.is_empty() {
                        policy.insert("tools".into(), Value::Object(tools));
                    }
                }
            }
            if !policy.is_empty() {
                entry.insert("approval".into(), Value::Object(policy));
            }
        }
        McpFileEdit::SetExternalUsers(id, allowed) => {
            let (entry, _) = entry_mut(&mut root, id)?;
            if allowed {
                entry.insert("allow_external_users".into(), json!(true));
            } else {
                entry.remove("allow_external_users");
            }
        }
        McpFileEdit::SetOptions(id, options) => {
            let (entry, form) = entry_mut(&mut root, id)?;
            for (key, value) in [
                ("startup", options.startup.map(|v| json!(v))),
                ("lifecycle", options.lifecycle.map(|v| json!(v))),
                ("concurrency", options.concurrency.map(|v| json!(v))),
                ("tasks", options.tasks.as_ref().map(|v| json!(v))),
            ] {
                set_or_remove_value(entry, key, value);
            }
            let timeouts = &options.timeouts;
            merge_fields(
                entry,
                "timeouts",
                McpTimeoutSettings::FIELDS.iter().zip([
                    timeouts.setup_secs,
                    timeouts.list_secs,
                    timeouts.request_secs,
                    timeouts.call_secs,
                    timeouts.elicitation_secs,
                ]),
            );
            merge_fields(
                entry,
                "limits",
                [(&"output_text_bytes", options.limits.output_text_bytes)].into_iter(),
            );
            // The older list form nests the transport fields.
            let transport = match (form, entry.get_mut("transport")) {
                (Form::List, Some(Value::Object(transport))) => transport,
                _ => entry,
            };
            set_or_remove_value(
                transport,
                "inherit_env",
                options.inherit_env.map(|v| json!(v)),
            );
        }
    }

    let mut content = serde_json::to_string_pretty(&root)?;
    content.push('\n');
    Ok(content)
}

fn set_or_remove_value(object: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            object.insert(key.to_string(), value);
        }
        None => {
            object.remove(key);
        }
    }
}

/// Sets or removes the named fields of the object at `key`, keeping its
/// other fields, and drops the object once it is empty.
fn merge_fields<'k, T: serde::Serialize>(
    entry: &mut Map<String, Value>,
    key: &str,
    fields: impl Iterator<Item = (&'k &'k str, Option<T>)>,
) {
    let mut object = match entry.remove(key) {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    };
    for (field, value) in fields {
        set_or_remove_value(&mut object, field, value.map(|v| json!(v)));
    }
    if !object.is_empty() {
        entry.insert(key.to_string(), Value::Object(object));
    }
}

fn set_or_remove(object: &mut Map<String, Value>, key: &str, approval: Option<McpApproval>) {
    match approval {
        Some(approval) => {
            object.insert(key.to_string(), json!(approval));
        }
        None => {
            object.remove(key);
        }
    }
}

/// Shows or hides one remote tool through a server's `include`/`exclude`
/// filters. A hidden tool is never offered to the model.
pub(crate) fn set_tool_visible(
    include: &mut BTreeSet<String>,
    exclude: &mut BTreeSet<String>,
    tool: &str,
    visible: bool,
) {
    if visible {
        exclude.remove(tool);
        // An allowlist has to name it too.
        if !include.is_empty() {
            include.insert(tool.to_string());
        }
    } else {
        exclude.insert(tool.to_string());
    }
}

/// The entries of mcp.json in file order, by id, as raw JSON. Entries without
/// a usable id are left out.
pub(crate) fn raw_entries(root: &Value) -> Vec<(String, &Value)> {
    let mut entries = Vec::new();
    for key in ROOTS {
        match root.get(key) {
            Some(Value::Object(servers)) => {
                entries.extend(
                    servers
                        .iter()
                        .map(|(id, entry)| (id.trim().to_string(), entry)),
                );
            }
            Some(Value::Array(servers)) => {
                entries.extend(
                    servers
                        .iter()
                        .filter_map(|entry| Some((entry_id(entry)?.to_string(), entry))),
                );
            }
            _ => {}
        }
    }
    entries.retain(|(id, _)| !id.is_empty());
    entries
}

/// The mcp.json entry for `server`, written the way other MCP clients write
/// one under `mcpServers`.
pub(crate) fn entry_json(server: &McpServerSettings) -> Map<String, Value> {
    let mut object = Map::new();
    match &server.transport {
        McpTransportSettings::Stdio(stdio) => {
            object.insert("type".into(), json!("stdio"));
            object.insert("command".into(), json!(stdio.command));
            if !stdio.args.is_empty() {
                object.insert("args".into(), json!(stdio.args));
            }
            if !stdio.env.is_empty() {
                object.insert("env".into(), string_map_json(&stdio.env));
            }
            if let Some(cwd) = &stdio.cwd {
                object.insert("cwd".into(), json!(cwd));
            }
            if let Some(inherit_env) = stdio.inherit_env {
                object.insert("inherit_env".into(), json!(inherit_env));
            }
        }
        McpTransportSettings::StreamableHttp(http) => {
            object.insert("type".into(), json!("http"));
            object.insert("url".into(), json!(http.url));
            let mut headers = http.headers.clone();
            if let Some(token) = &http.bearer_token {
                let has_authorization = headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization"));
                if !has_authorization {
                    headers.insert("Authorization".into(), format!("Bearer {token}"));
                }
            }
            if !headers.is_empty() {
                object.insert("headers".into(), string_map_json(&headers));
            }
            if let Some(oauth) = &http.oauth {
                object.insert("oauth".into(), json!(oauth));
            }
        }
    }

    if server.disabled {
        object.insert("enabled".into(), json!(false));
    }
    if !server.include.is_empty() {
        object.insert("include".into(), json!(server.include));
    }
    if !server.exclude.is_empty() {
        object.insert("exclude".into(), json!(server.exclude));
    }
    if let Some(lifecycle) = server.lifecycle {
        object.insert("lifecycle".into(), json!(lifecycle));
    }
    if let Some(startup) = server.startup {
        object.insert("startup".into(), json!(startup));
    }
    if let Some(tasks) = &server.tasks {
        object.insert("tasks".into(), json!(tasks));
    }
    if !server.approval.is_empty() {
        object.insert("approval".into(), json!(server.approval));
    }
    if server.allow_external_users {
        object.insert("allow_external_users".into(), json!(true));
    }
    if !server.timeouts.is_empty() {
        object.insert("timeouts".into(), json!(server.timeouts));
    }
    if let Some(concurrency) = server.concurrency {
        object.insert("concurrency".into(), json!(concurrency));
    }
    if !server.limits.is_empty() {
        object.insert("limits".into(), json!(server.limits));
    }
    object
}

#[derive(Clone, Copy)]
enum Form {
    /// `{"mcpServers": {"id": {...}}}`
    Map,
    /// `{"servers": [{"id": "...", "transport": {...}}]}`
    List,
}

fn list_entry_json(server: &McpServerSettings) -> Result<Map<String, Value>, BoxError> {
    match serde_json::to_value(server)? {
        Value::Object(object) => Ok(object),
        _ => Err("MCP server settings must serialize to an object".into()),
    }
}

/// Adds `servers` after checking that none of their ids is taken.
fn add_entries(
    root: &mut Value,
    content: &str,
    servers: &[McpServerSettings],
) -> Result<(), BoxError> {
    let declared = McpSettings::from_json_contents(content)?;
    let mut ids = BTreeSet::new();
    for server in servers {
        if declared.declares(&server.id) || !ids.insert(server.id.as_str()) {
            return Err(McpError::already_exists(format!(
                "MCP server {} already exists in mcp.json",
                server.id
            )));
        }
    }
    for server in servers {
        add_entry(root, server)?;
    }
    Ok(())
}

fn add_entry(root: &mut Value, server: &McpServerSettings) -> Result<(), BoxError> {
    let object = root
        .as_object_mut()
        .ok_or("mcp.json root must be an object")?;
    let root_key = ROOTS
        .into_iter()
        .find(|key| object.contains_key(*key))
        .unwrap_or("mcpServers");
    match object.entry(root_key).or_insert_with(|| json!({})) {
        Value::Object(servers) => {
            servers.insert(server.id.clone(), Value::Object(entry_json(server)));
        }
        Value::Array(servers) => servers.push(Value::Object(list_entry_json(server)?)),
        _ => {
            return Err(
                format!("mcp.json {root_key} must be an object to persist a server").into(),
            );
        }
    }
    Ok(())
}

/// The first entry declaring `id`, and the form it is written in.
fn entry_mut<'a>(
    root: &'a mut Value,
    id: &str,
) -> Result<(&'a mut Map<String, Value>, Form), BoxError> {
    // Locate first and borrow mutably after, so the search can look at both
    // roots.
    let location = ROOTS.into_iter().find_map(|key| match root.get(key) {
        Some(Value::Object(servers)) => servers
            .keys()
            .find(|name| name.trim() == id)
            .map(|name| (key, Some(name.clone()), 0)),
        Some(Value::Array(servers)) => servers
            .iter()
            .position(|entry| entry_id(entry) == Some(id))
            .map(|index| (key, None, index)),
        _ => None,
    });
    let (key, name, index) = location.ok_or_else(|| McpError::not_found(id))?;
    let servers = &mut root[key];
    let (entry, form) = match name {
        Some(name) => (&mut servers[name.as_str()], Form::Map),
        None => (&mut servers[index], Form::List),
    };
    match entry {
        Value::Object(entry) => Ok((entry, form)),
        _ => Err(McpError::invalid(format!(
            "MCP server {id} in mcp.json is not an object; fix it by hand"
        ))),
    }
}

fn entry_id(entry: &Value) -> Option<&str> {
    entry.get("id").and_then(Value::as_str).map(str::trim)
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
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
    use crate::config::McpStreamableHttpSettings;

    fn http(id: &str, url: &str) -> McpServerSettings {
        McpServerSettings {
            id: id.to_string(),
            transport: McpTransportSettings::StreamableHttp(McpStreamableHttpSettings {
                url: url.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn edited(content: &str, edit: McpFileEdit<'_>) -> Value {
        serde_json::from_str(&apply_edit(content, edit).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn add_appends_and_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = McpSettings::file_path(dir.path());
        tokio::fs::write(&path, "{\n  \"note\": true\n}\n")
            .await
            .unwrap();

        let server = http("remote", "https://mcp.example.test/mcp");
        let file = edit(&path, &Mutex::new(()), None, McpFileEdit::Add(&server))
            .await
            .unwrap();
        assert_eq!(file, McpConfigFile::read(&path).await.unwrap());

        let json: Value = serde_json::from_str(file.text()).unwrap();
        assert_eq!(json["note"], true);
        assert_eq!(json["mcpServers"]["remote"]["type"], "http");
        assert_eq!(
            json["mcpServers"]["remote"]["url"],
            "https://mcp.example.test/mcp"
        );
    }

    #[test]
    fn add_writes_into_the_root_the_file_uses() {
        let json = edited(
            r#"{"servers":{"existing":{"type":"stdio","command":"existing-mcp"}},"other":1}"#,
            McpFileEdit::Add(&http("remote", "https://mcp.example.test/mcp")),
        );
        assert_eq!(json["other"], 1);
        assert!(json.get("mcpServers").is_none());
        assert_eq!(json["servers"]["existing"]["command"], "existing-mcp");
        assert_eq!(json["servers"]["remote"]["type"], "http");

        let json = edited(
            r#"{"servers":[{"id":"existing","transport":{"type":"stdio","command":"x"}}]}"#,
            McpFileEdit::Add(&http("remote", "https://mcp.example.test/mcp")),
        );
        assert_eq!(json["servers"][1]["id"], "remote");
        assert_eq!(json["servers"][1]["transport"]["type"], "http");
        let settings = McpSettings::from_json_contents(&json.to_string()).unwrap();
        assert_eq!(settings.servers.len(), 2);
    }

    #[test]
    fn options_edit_their_fields_and_keep_the_rest() {
        let content = r#"{"mcpServers":{"docs":{"url":"https://x.test/mcp","startup":"eager","limits":{"schema_bytes":1}}},
            "servers":[{"id":"tool","transport":{"type":"stdio","command":"t"}}]}"#;
        let options: McpServerOptions = serde_json::from_value(json!({
            "lifecycle": "initialize",
            "concurrency": "parallel",
            "timeouts": { "setup_secs": 10, "request_secs": 20 },
            "limits": { "output_text_bytes": 4096 }
        }))
        .unwrap();
        let json = edited(content, McpFileEdit::SetOptions("docs", &options));
        assert_eq!(
            json["mcpServers"]["docs"],
            json!({
                "url": "https://x.test/mcp",
                "lifecycle": "initialize",
                "concurrency": "parallel",
                "timeouts": { "setup_secs": 10, "request_secs": 20 },
                "limits": { "schema_bytes": 1, "output_text_bytes": 4096 }
            })
        );
        // The older list form keeps the environment with its transport.
        let options: McpServerOptions =
            serde_json::from_value(json!({ "inherit_env": false })).unwrap();
        let json = edited(content, McpFileEdit::SetOptions("tool", &options));
        assert_eq!(json["servers"][0]["transport"]["inherit_env"], false);
        let settings = McpSettings::from_json_contents(&json.to_string()).unwrap();
        assert_eq!(settings.servers[1].options().inherit_env, Some(false));
    }

    #[test]
    fn adding_several_refuses_any_taken_or_repeated_id() {
        let content = r#"{"mcpServers":{"docs":{"url":"https://x.test/mcp"}}}"#;
        let added = edited(
            content,
            McpFileEdit::AddAll(&[
                http("a", "https://a.test/mcp"),
                http("b", "https://b.test/mcp"),
            ]),
        );
        assert_eq!(
            added["mcpServers"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["docs", "a", "b"]
        );
        for servers in [
            vec![
                http("a", "https://a.test/mcp"),
                http("docs", "https://d.test/mcp"),
            ],
            vec![
                http("a", "https://a.test/mcp"),
                http("a", "https://b.test/mcp"),
            ],
        ] {
            let err = apply_edit(content, McpFileEdit::AddAll(&servers)).unwrap_err();
            assert!(err.to_string().contains("already exists"), "{err}");
        }
    }

    #[test]
    fn add_refuses_an_id_a_skipped_entry_owns_and_a_non_object_root() {
        let broken = r#"{"mcpServers":{"github":{"command":"gh","lifecycle":"handshake"}}}"#;
        let err = apply_edit(
            broken,
            McpFileEdit::Add(&http("github", "https://x.test/mcp")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");

        let err = apply_edit(
            "[]",
            McpFileEdit::Add(&http("remote", "https://x.test/mcp")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("root must be an object"), "{err}");
    }

    #[test]
    fn replace_keeps_unknown_fields_and_the_list_form() {
        let content = r#"{"mcpServers":{"docs":{"url":"https://old.test/mcp","timeouts":{"call_secs":600},"approval":{"default":"ask"},"description":"Docs"}}}"#;
        let mut server = http("docs", "https://new.test/mcp");
        server
            .approval
            .tools
            .insert("search".into(), McpApproval::Allow);
        server.timeouts.setup_secs = Some(30);
        let json = edited(content, McpFileEdit::Replace(&server));
        let entry = &json["mcpServers"]["docs"];
        assert_eq!(entry["url"], "https://new.test/mcp");
        assert_eq!(entry["timeouts"], json!({ "setup_secs": 30 }));
        assert_eq!(entry["description"], "Docs");
        // The policy is a field Anda reads, so it is the one replaced with.
        assert_eq!(entry["approval"], json!({ "tools": { "search": "allow" } }));

        let content = r#"{"servers":[{"id":"docs","transport":{"type":"http","url":"https://old.test/mcp"},"note":"kept"}]}"#;
        let json = edited(
            content,
            McpFileEdit::Replace(&http("docs", "https://new.test/mcp")),
        );
        assert_eq!(
            json["servers"][0]["transport"]["url"],
            "https://new.test/mcp"
        );
        assert_eq!(json["servers"][0]["note"], "kept");

        let err = apply_edit(
            content,
            McpFileEdit::Replace(&http("missing", "https://x.test")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not configured"), "{err}");
    }

    #[test]
    fn approval_edits_keep_the_rest_of_the_policy() {
        let content = r#"{"mcpServers":{"docs":{"url":"https://x.test/mcp","approval":{"default":"ask","note":"kept"}}},
"servers":[{"id":"legacy","transport":{"type":"http","url":"https://y.test/mcp"}}]}"#;
        let set = |content: &str, id, tool, approval| {
            apply_edit(content, McpFileEdit::SetApproval { id, tool, approval }).unwrap()
        };
        let content = set(content, "docs", Some("delete"), Some(McpApproval::Ask));
        let content = set(&content, "docs", None, Some(McpApproval::Allow));
        let content = set(&content, "legacy", Some("search"), Some(McpApproval::Allow));
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            json["mcpServers"]["docs"]["approval"],
            json!({ "default": "allow", "note": "kept", "tools": { "delete": "ask" } })
        );
        assert_eq!(
            json["servers"][0]["approval"],
            json!({ "tools": { "search": "allow" } })
        );
        let settings = McpSettings::from_json_contents(&content).unwrap();
        let docs = &settings.servers[0];
        assert_eq!(docs.approval.for_tool("delete"), McpApproval::Ask);
        assert_eq!(docs.approval.for_tool("search"), McpApproval::Allow);
        assert_eq!(
            settings.servers[1].approval.for_tool("search"),
            McpApproval::Allow
        );

        // Clearing the last policy removes the section.
        let content = set(&content, "legacy", Some("search"), None);
        let content = apply_edit(&content, McpFileEdit::SetExternalUsers("legacy", true)).unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert!(json["servers"][0].get("approval").is_none(), "{json}");
        assert_eq!(json["servers"][0]["allow_external_users"], true);
        let content = apply_edit(&content, McpFileEdit::SetExternalUsers("legacy", false)).unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert!(json["servers"][0].get("allow_external_users").is_none());
    }

    #[test]
    fn enable_disable_and_remove_touch_only_their_entry() {
        let content = r#"{
  "mcpServers": {
    "github": {
      "command": "gh-mcp",
      "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" },
      "lifecycle": "handshake"
    },
    "docs": { "url": "https://docs.test/mcp", "disabled": true }
  },
  "servers": [{ "id": "legacy", "transport": { "type": "stdio", "command": "x" } }]
}"#;
        // The skipped github entry can still be disabled, and its raw
        // reference survives.
        let json = edited(content, McpFileEdit::SetEnabled("github", false));
        assert_eq!(json["mcpServers"]["github"]["enabled"], false);
        assert_eq!(
            json["mcpServers"]["github"]["env"]["GITHUB_TOKEN"],
            "${GITHUB_TOKEN}"
        );

        let json = edited(content, McpFileEdit::SetEnabled("docs", true));
        assert!(json["mcpServers"]["docs"].get("disabled").is_none());
        assert!(json["mcpServers"]["docs"].get("enabled").is_none());

        let json = edited(content, McpFileEdit::SetEnabled("legacy", false));
        assert_eq!(json["servers"][0]["disabled"], true);

        let json = edited(content, McpFileEdit::Remove("legacy"));
        assert_eq!(json["servers"].as_array().unwrap().len(), 0);
        let json = edited(content, McpFileEdit::Remove("github"));
        assert!(json["mcpServers"].get("github").is_none());
        assert_eq!(json["mcpServers"]["docs"]["url"], "https://docs.test/mcp");

        let err = apply_edit(content, McpFileEdit::Remove("missing")).unwrap_err();
        assert!(err.to_string().contains("not configured"), "{err}");
    }

    #[test]
    fn tool_visibility_edits_the_filters() {
        let content = r#"{"mcpServers":{"github":{"url":"https://gh.test/mcp","exclude":["a"]}}}"#;
        let json = edited(
            content,
            McpFileEdit::SetToolVisible {
                id: "github",
                tool: "delete_repository",
                visible: false,
            },
        );
        assert_eq!(
            json["mcpServers"]["github"]["exclude"],
            json!(["a", "delete_repository"])
        );

        let json = edited(
            content,
            McpFileEdit::SetToolVisible {
                id: "github",
                tool: "a",
                visible: true,
            },
        );
        assert!(json["mcpServers"]["github"].get("exclude").is_none());

        // An allowlist has to name a tool that is shown again.
        let content = r#"{"mcpServers":{"github":{"url":"https://gh.test/mcp","include":["read"],"exclude":["write"]}}}"#;
        let json = edited(
            content,
            McpFileEdit::SetToolVisible {
                id: "github",
                tool: "write",
                visible: true,
            },
        );
        assert_eq!(
            json["mcpServers"]["github"]["include"],
            json!(["read", "write"])
        );
    }

    #[test]
    fn oauth_markers_keep_raw_environment_references_and_legacy_lists() {
        for content in [
            r#"{"mcpServers":{"srv":{"type":"http","url":"$ENDPOINT","headers":{"X-Tenant":"$TENANT"},"include":["read"],"bearer_token":"old"}}}"#,
            r#"{"servers":[{"id":"srv","transport":{"type":"http","url":"$ENDPOINT","headers":{"X-Tenant":"$TENANT"}},"include":["read"]}]}"#,
        ] {
            let updated = apply_edit(
                content,
                McpFileEdit::SetOAuth(
                    "srv",
                    &McpOAuthSettings {
                        client_id: Some("public-client".into()),
                        scopes: vec!["read".into()],
                    },
                ),
            )
            .unwrap();
            assert!(
                updated.contains("$ENDPOINT")
                    && updated.contains("$TENANT")
                    && updated.contains("public-client")
                    && !updated.contains("bearer_token")
            );
            McpSettings::from_json_contents(&updated).unwrap();
        }
    }

    #[tokio::test]
    async fn an_edit_based_on_a_stale_revision_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = McpSettings::file_path(dir.path());
        let lock = Mutex::new(());
        let first = edit(
            &path,
            &lock,
            Some(""),
            McpFileEdit::Add(&http("a", "https://a.test/mcp")),
        )
        .await
        .unwrap();

        // Based on the missing file: stale now.
        let err = edit(
            &path,
            &lock,
            Some(""),
            McpFileEdit::Add(&http("b", "https://b.test/mcp")),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.downcast_ref::<McpError>().unwrap().code,
            "revision_conflict"
        );

        edit(
            &path,
            &lock,
            Some(&first.revision),
            McpFileEdit::Add(&http("b", "https://b.test/mcp")),
        )
        .await
        .unwrap();
        let file = McpConfigFile::read(&path).await.unwrap();
        let settings = McpSettings::from_json_contents(file.text()).unwrap();
        assert_eq!(settings.servers.len(), 2);
    }

    #[test]
    fn raw_entries_follow_the_file_order_across_roots() {
        let root: Value = serde_json::from_str(
            r#"{"mcpServers":{"b":{},"a":{}},"servers":[{"id":"c"},{"no_id":true}]}"#,
        )
        .unwrap();
        let ids: Vec<String> = raw_entries(&root).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["b", "a", "c"]);
    }
}
