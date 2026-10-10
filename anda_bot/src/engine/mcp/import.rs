//! Importing MCP servers from other clients: Claude Desktop, Claude Code,
//! Cursor, VS Code (with GitHub Copilot's CLI), Windsurf and Codex.
//!
//! A scan only reads their files and shows each server redacted, with what
//! importing it would do. An import scans again and writes what the files say
//! then, so a client names servers by their key and never hands in an entry.
//! On the way in, an entry is translated into mcp.json's form: the other
//! client's variables become Anda's, a variable the daemon's environment
//! lacks becomes a secret to set, plaintext tokens move to the secret store
//! (unless the owner keeps them), and a local server runs without the
//! daemon's whole environment.

use anda_core::BoxError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use super::{McpError, redact::redact_entry, secrets::McpSecretStore, view::describe};
use crate::config::{
    McpSecretValues, McpServerSettings, McpSettings, McpTransportSettings, is_secret_name,
    secret_references,
};

/// `~/.claude.json` keeps Claude Code's history too, so it can be large.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Claude Code lists every directory it was opened in; their `.mcp.json`
/// files are read up to this many.
const MAX_PROJECTS: usize = 200;

/// A client whose MCP configuration can be imported.
#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpImportSource {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    /// VS Code, and GitHub Copilot's CLI, which shares its servers.
    Vscode,
    Windsurf,
    Codex,
}

impl McpImportSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude_desktop",
            Self::ClaudeCode => "claude_code",
            Self::Cursor => "cursor",
            Self::Vscode => "vscode",
            Self::Windsurf => "windsurf",
            Self::Codex => "codex",
        }
    }

    /// The client's name, as people know it.
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Vscode => "VS Code",
            Self::Windsurf => "Windsurf",
            Self::Codex => "Codex",
        }
    }

    /// A short suffix for an id that is taken.
    fn suffix(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude",
            Self::ClaudeCode => "claude-code",
            Self::Cursor => "cursor",
            Self::Vscode => "vscode",
            Self::Windsurf => "windsurf",
            Self::Codex => "codex",
        }
    }
}

/// Where a scan looks.
#[derive(Clone, Debug)]
pub(crate) struct McpImportContext {
    pub user_home: PathBuf,
    /// Where desktop apps keep their settings: `~/Library/Application
    /// Support`, `%APPDATA%`, or `$XDG_CONFIG_HOME` (`~/.config`).
    pub app_data: PathBuf,
    /// Project directories whose own configuration is read too.
    pub workspaces: Vec<PathBuf>,
}

impl McpImportContext {
    /// The current user's directories.
    pub fn detect(workspaces: Vec<PathBuf>) -> Result<Self, BoxError> {
        let user_home = std::env::home_dir().ok_or("could not find the user's home directory")?;
        let app_data = if cfg!(target_os = "macos") {
            user_home.join("Library").join("Application Support")
        } else if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| user_home.join("AppData").join("Roaming"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| user_home.join(".config"))
        };
        Ok(Self {
            user_home,
            app_data,
            workspaces,
        })
    }
}

/// A server Anda already has, to compare candidates with.
#[derive(Clone, Debug)]
pub(crate) struct McpKnownServer {
    pub id: String,
    /// `None` for an entry that does not parse.
    pub endpoint: Option<String>,
}

impl McpKnownServer {
    pub fn of(server: &McpServerSettings) -> Self {
        Self {
            id: server.id.trim().to_string(),
            endpoint: Some(endpoint(server)),
        }
    }
}

/// What importing a candidate would do.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpImportStatus {
    /// It is imported as `id`.
    New,
    /// Its name is taken by another server, so it is imported as `id`.
    Renamed,
    /// Anda has it already, as `existing_id`.
    Exists,
    /// Anda runs the same server as `existing_id`, or the same server is
    /// listed earlier (`duplicate_of`).
    Duplicate,
    /// It cannot be imported; `error` says why.
    Invalid,
}

/// A secret an imported server needs a value for.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct McpImportSecret {
    pub name: String,
    /// What it stood for in the other client.
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpImportCandidate {
    /// Names it in an import.
    pub key: String,
    pub source: McpImportSource,
    /// The file it is in.
    pub path: String,
    /// The project it belongs to, for a project's configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Its name in that file.
    pub name: String,
    /// The id it is imported as.
    pub id: String,
    pub status: McpImportStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    /// `stdio`, `http`, or `unknown` when it does not translate.
    pub transport: &'static str,
    /// The command line or URL, redacted.
    pub summary: String,
    pub enabled: bool,
    /// The entry as it would be written, redacted.
    pub settings: Value,
    /// Fields holding plaintext values that would move to the secret store.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plaintext: Vec<String>,
    /// Secrets it references that are not set yet.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needs_secrets: Vec<McpImportSecret>,
    /// What does not carry over.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The translated entry, with its values: never shown.
    #[serde(skip)]
    entry: Option<Map<String, Value>>,
}

/// A configuration file a scan read.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpImportFile {
    pub source: McpImportSource,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Servers found in it.
    pub servers: usize,
    /// Why it could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct McpImportScan {
    pub files: Vec<McpImportFile>,
    pub candidates: Vec<McpImportCandidate>,
}

/// One candidate to import, under its own id unless `id` gives another.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpImportPick {
    pub key: String,
    #[serde(default)]
    pub id: Option<String>,
}

/// An import, as the API and the CLI ask for one.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpImportRequest {
    pub items: Vec<McpImportPick>,
    /// Values for the secrets the servers need that are not set yet.
    #[serde(default)]
    pub secrets: McpSecretValues,
    /// Move plaintext tokens into the secret store (the default).
    #[serde(default = "default_true")]
    pub store_secrets: bool,
    /// Project directories the scan read beyond the daemon's own workspace.
    #[serde(default)]
    pub workspaces: Vec<PathBuf>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

fn default_true() -> bool {
    true
}

/// What an import writes.
#[derive(Debug, Default)]
pub(crate) struct McpImportPlan {
    /// Each server with the file it came from.
    pub servers: Vec<(McpServerSettings, String)>,
    /// Secrets to store: moved plaintext and the values given for missing
    /// ones.
    pub secrets: BTreeMap<String, String>,
}

/// What a scan knows about Anda: its servers, and the secrets that are set.
pub(crate) struct McpImportTarget<'a> {
    pub known: &'a [McpKnownServer],
    pub secrets: &'a BTreeSet<String>,
    /// Whether the daemon's environment has a variable.
    pub has_env: &'a (dyn Fn(&str) -> bool + Sync),
}

#[derive(Clone, Copy)]
enum Format {
    /// `{"mcpServers": {...}}`, as most clients write it.
    McpServers,
    /// `~/.claude.json`: user servers, and servers by project.
    ClaudeUser,
    /// VS Code's `{"servers": {...}, "inputs": [...]}`.
    Vscode,
    /// Codex's config.toml: `[mcp_servers.<name>]`.
    Codex,
}

struct ConfigFile {
    source: McpImportSource,
    path: PathBuf,
    format: Format,
}

/// A server as another client wrote it.
struct RawServer {
    name: String,
    value: Value,
    project: Option<PathBuf>,
}

/// Reads the other clients' configuration and translates every server in it.
pub(crate) async fn scan(
    ctx: &McpImportContext,
    sources: &[McpImportSource],
    target: &McpImportTarget<'_>,
) -> McpImportScan {
    let wanted = |source| sources.is_empty() || sources.contains(&source);
    let home = &ctx.user_home;
    let mut files = Vec::new();
    let mut add = |source, path: PathBuf, format| {
        if wanted(source) {
            files.push(ConfigFile {
                source,
                path,
                format,
            });
        }
    };
    add(
        McpImportSource::ClaudeDesktop,
        ctx.app_data
            .join("Claude")
            .join("claude_desktop_config.json"),
        Format::McpServers,
    );
    add(
        McpImportSource::ClaudeCode,
        home.join(".claude.json"),
        Format::ClaudeUser,
    );
    add(
        McpImportSource::Cursor,
        home.join(".cursor").join("mcp.json"),
        Format::McpServers,
    );
    for app in ["Code", "Code - Insiders"] {
        add(
            McpImportSource::Vscode,
            ctx.app_data.join(app).join("User").join("mcp.json"),
            Format::Vscode,
        );
    }
    add(
        McpImportSource::Vscode,
        home.join(".copilot").join("mcp-config.json"),
        Format::McpServers,
    );
    add(
        McpImportSource::Windsurf,
        home.join(".codeium")
            .join("windsurf")
            .join("mcp_config.json"),
        Format::McpServers,
    );
    add(
        McpImportSource::Codex,
        home.join(".codex").join("config.toml"),
        Format::Codex,
    );

    let mut scan = McpImportScan::default();
    let mut raw_servers = Vec::new();
    let mut projects: Vec<PathBuf> = ctx.workspaces.clone();
    for file in files {
        let (servers, file_projects, error) = match read_config(&file.path, file.format).await {
            Ok(None) => continue,
            Ok(Some(root)) => {
                let (servers, file_projects) = servers_in(&root, file.format);
                (servers, file_projects, None)
            }
            Err(err) => (Vec::new(), Vec::new(), Some(err)),
        };
        projects.extend(file_projects);
        scan.files.push(McpImportFile {
            source: file.source,
            path: file.path.display().to_string(),
            project: None,
            servers: servers.len(),
            error,
        });
        raw_servers.push((file.source, file.path, servers));
    }

    // Projects: the workspaces asked for, and the ones Claude Code knows.
    let mut seen = BTreeSet::new();
    projects.retain(|project| project.is_absolute() && seen.insert(project.clone()));
    projects.truncate(MAX_PROJECTS);
    for project in projects {
        for (source, path, format) in [
            (
                McpImportSource::ClaudeCode,
                project.join(".mcp.json"),
                Format::McpServers,
            ),
            (
                McpImportSource::Cursor,
                project.join(".cursor").join("mcp.json"),
                Format::McpServers,
            ),
            (
                McpImportSource::Vscode,
                project.join(".vscode").join("mcp.json"),
                Format::Vscode,
            ),
        ] {
            if !wanted(source) {
                continue;
            }
            let (servers, error) = match read_config(&path, format).await {
                Ok(None) => continue,
                Ok(Some(root)) => (servers_in(&root, format).0, None),
                Err(err) => (Vec::new(), Some(err)),
            };
            let servers: Vec<RawServer> = servers
                .into_iter()
                .map(|server| RawServer {
                    project: Some(project.clone()),
                    ..server
                })
                .collect();
            scan.files.push(McpImportFile {
                source,
                path: path.display().to_string(),
                project: Some(project.display().to_string()),
                servers: servers.len(),
                error,
            });
            raw_servers.push((source, path, servers));
        }
    }

    let mut ids = Ids::new(target.known);
    for (source, path, servers) in raw_servers {
        for server in servers {
            let candidate = candidate(source, &path, server, ctx, target, &mut ids);
            scan.candidates.push(candidate);
        }
    }
    scan
}

/// The ids and endpoints taken so far, by Anda's servers and by earlier
/// candidates.
struct Ids {
    taken: BTreeSet<String>,
    known: BTreeMap<String, String>,
    listed: BTreeMap<String, String>,
}

impl Ids {
    fn new(known: &[McpKnownServer]) -> Self {
        Self {
            taken: known.iter().map(|server| server.id.clone()).collect(),
            known: known
                .iter()
                .filter_map(|server| Some((server.endpoint.clone()?, server.id.clone())))
                .collect(),
            listed: BTreeMap::new(),
        }
    }

    fn free(&self, name: &str, source: McpImportSource) -> String {
        let base = format!("{name}-{}", source.suffix());
        let mut id = base.clone();
        for n in 2.. {
            if !self.taken.contains(&id) {
                break;
            }
            id = format!("{base}-{n}");
        }
        id
    }
}

fn candidate(
    source: McpImportSource,
    path: &Path,
    server: RawServer,
    ctx: &McpImportContext,
    target: &McpImportTarget<'_>,
    ids: &mut Ids,
) -> McpImportCandidate {
    let RawServer {
        name,
        value,
        project,
    } = server;
    let key = match &project {
        Some(project) => format!(
            "{}:{}@{}#{name}",
            source.as_str(),
            path.display(),
            project.display()
        ),
        None => format!("{}:{}#{name}", source.as_str(), path.display()),
    };
    let mut candidate = McpImportCandidate {
        key,
        source,
        path: path.display().to_string(),
        project: project.as_ref().map(|path| path.display().to_string()),
        name: name.clone(),
        id: import_id(&name),
        status: McpImportStatus::Invalid,
        existing_id: None,
        duplicate_of: None,
        transport: "unknown",
        summary: String::new(),
        enabled: true,
        settings: redact_entry(&value),
        plaintext: Vec::new(),
        needs_secrets: Vec::new(),
        warnings: Vec::new(),
        error: None,
        entry: None,
    };
    // VS Code keeps its inputs beside the servers; `servers_in` hands them
    // over inside the entry.
    let inputs = value.get(INPUTS_KEY).map(vscode_inputs).unwrap_or_default();
    let convert = Convert {
        user_home: &ctx.user_home,
        project: project.as_deref(),
        inputs: &inputs,
        has_env: target.has_env,
    };
    let translated = match convert.entry(source, &value) {
        Ok(translated) => translated,
        Err(err) => {
            candidate.error = Some(err);
            return candidate;
        }
    };
    candidate.warnings = translated.warnings;
    let entry = translated.entry;
    let settings = match McpSettings::parse_entry(&candidate.id, &Value::Object(entry.clone())) {
        Ok(settings) => settings,
        Err(err) => {
            candidate.error = Some(err.to_string());
            return candidate;
        }
    };
    let issues = settings.setup_issues();
    if !issues.is_empty() {
        candidate.error = Some(issues.join("; "));
        return candidate;
    }
    let (transport, summary, _) = describe(&settings.transport);
    candidate.transport = transport;
    candidate.summary = summary;
    candidate.enabled = !settings.disabled;
    candidate.settings = redact_entry(&Value::Object(entry.clone()));
    candidate.plaintext = plaintext_fields(&entry);
    let mut referenced = BTreeSet::new();
    for value in string_values(&entry) {
        secret_references(value, &mut referenced);
    }
    candidate.needs_secrets = referenced
        .into_iter()
        .filter(|name| !target.secrets.contains(name))
        .map(|name| McpImportSecret {
            description: translated
                .secrets
                .get(&name)
                .cloned()
                .unwrap_or_else(|| format!("secret {name}")),
            name,
        })
        .collect();
    candidate.entry = Some(entry);

    let endpoint = endpoint(&settings);
    if let Some(existing) = ids.known.get(&endpoint) {
        candidate.status = if *existing == candidate.id {
            McpImportStatus::Exists
        } else {
            McpImportStatus::Duplicate
        };
        candidate.existing_id = Some(existing.clone());
    } else if let Some(earlier) = ids.listed.get(&endpoint) {
        candidate.status = McpImportStatus::Duplicate;
        candidate.duplicate_of = Some(earlier.clone());
    } else {
        ids.listed.insert(endpoint, candidate.key.clone());
        if ids.taken.contains(&candidate.id) {
            candidate.id = ids.free(&candidate.id, source);
            candidate.status = McpImportStatus::Renamed;
        } else {
            candidate.status = McpImportStatus::New;
        }
        ids.taken.insert(candidate.id.clone());
    }
    candidate
}

/// Picks the candidates to import from a fresh scan and works out what to
/// write: their entries, under the ids chosen, and the secrets to store.
/// `values` are values for secrets the candidates need; each must be one of
/// those. With `move_plaintext`, plaintext header and env values and bearer
/// tokens become secrets named `<ID>_<KEY>`.
pub(crate) fn plan(
    scan: &McpImportScan,
    picks: &[McpImportPick],
    values: &McpSecretValues,
    move_plaintext: bool,
    is_taken: impl Fn(&str) -> bool,
    secrets_set: &BTreeSet<String>,
) -> Result<McpImportPlan, BoxError> {
    if picks.is_empty() {
        return Err(McpError::invalid("pick at least one server to import"));
    }
    let mut plan = McpImportPlan::default();
    let mut ids = BTreeSet::new();
    let mut names: BTreeSet<String> = secrets_set.clone();
    let mut needed = BTreeSet::new();
    for pick in picks {
        let candidate = scan
            .candidates
            .iter()
            .find(|candidate| candidate.key == pick.key)
            .ok_or_else(|| {
                McpError::invalid(format!("{} is no longer in its file; scan again", pick.key))
            })?;
        let (Some(entry), status) = (&candidate.entry, candidate.status) else {
            return Err(McpError::invalid(format!(
                "{} cannot be imported: {}",
                candidate.name,
                candidate
                    .error
                    .as_deref()
                    .unwrap_or("it does not translate")
            )));
        };
        if status == McpImportStatus::Exists {
            return Err(McpError::invalid(format!(
                "{} is already configured as {}",
                candidate.name,
                candidate.existing_id.as_deref().unwrap_or_default()
            )));
        }
        let id = pick
            .id
            .as_deref()
            .map(str::trim)
            .unwrap_or(&candidate.id)
            .to_string();
        if id.is_empty() {
            return Err(McpError::invalid("an imported server needs an id"));
        }
        if is_taken(&id) || !ids.insert(id.clone()) {
            return Err(McpError::already_exists(format!(
                "MCP server {id} already exists; import it under another id"
            )));
        }
        let mut entry = entry.clone();
        // A local server gets the platform's essentials and its own `env`,
        // not every secret in the daemon's environment.
        if entry.get("type") == Some(&json!("stdio")) && !entry.contains_key("inherit_env") {
            entry.insert("inherit_env".into(), json!(false));
        }
        if move_plaintext {
            for (name, value) in move_to_secrets(&mut entry, &id, &mut names) {
                plan.secrets.insert(name, value);
            }
        }
        needed.extend(
            candidate
                .needs_secrets
                .iter()
                .map(|secret| secret.name.clone()),
        );
        let settings = McpSettings::parse_entry(&id, &Value::Object(entry))
            .map_err(|err| McpError::invalid(format!("MCP server {id}: {err}")))?;
        plan.servers.push((settings, candidate.path.clone()));
    }
    for (name, value) in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if !needed.contains(name) {
            return Err(McpError::invalid(format!(
                "the servers being imported do not need the secret {name}"
            )));
        }
        plan.secrets.insert(name.clone(), value.to_string());
    }
    Ok(plan)
}

/// Stores the plan's secrets, then runs `write`, which adds its servers.
/// Secrets go first so the servers start with them; when the write fails,
/// the ones that were new are deleted again.
pub(crate) async fn store_and_write<T>(
    plan: &McpImportPlan,
    store: &McpSecretStore,
    write: impl Future<Output = Result<T, BoxError>>,
) -> Result<T, BoxError> {
    let before = store.names();
    let forget = async || {
        for name in plan
            .secrets
            .keys()
            .filter(|name| !before.contains_key(*name))
        {
            if let Err(err) = store.set(name, None).await {
                log::warn!("the imported secret {name} could not be deleted again: {err}");
            }
        }
    };
    for (name, value) in &plan.secrets {
        if let Err(err) = store.set(name, Some(value)).await {
            forget().await;
            return Err(err);
        }
    }
    let written = write.await;
    if written.is_err() {
        forget().await;
    }
    written
}

/// What decides that two entries run the same server: the URL, or the
/// command line, as written.
pub(crate) fn endpoint(server: &McpServerSettings) -> String {
    match &server.transport {
        McpTransportSettings::StreamableHttp(http) => {
            format!(
                "url {}",
                http.url.trim().trim_end_matches('/').to_lowercase()
            )
        }
        McpTransportSettings::Stdio(stdio) => {
            let mut line = format!("cmd {}", stdio.command.trim());
            for arg in &stdio.args {
                line.push('\u{1f}');
                line.push_str(arg);
            }
            line
        }
    }
}

/// A secret name from free text: upper case letters, digits and `_`.
pub(crate) fn secret_name(text: &str) -> String {
    let mut name = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name.push(ch.to_ascii_uppercase());
        } else if !name.ends_with('_') {
            name.push('_');
        }
    }
    let name = name.trim_matches('_');
    if name.starts_with(|ch: char| ch.is_ascii_uppercase()) {
        name.to_string()
    } else {
        format!("_{name}")
    }
}

/// Moves the plaintext credentials of `entry` (see [`credential_fields`])
/// into secrets named after `id`, avoiding `names`, and returns them.
fn move_to_secrets(
    entry: &mut Map<String, Value>,
    id: &str,
    names: &mut BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut moved = BTreeMap::new();
    for (field, key) in credential_fields(entry) {
        let value = match field {
            "bearer_token" => entry.get_mut(field),
            _ => entry
                .get_mut(field)
                .and_then(Value::as_object_mut)
                .and_then(|values| values.get_mut(&key)),
        };
        let Some(value) = value else {
            continue;
        };
        let base = secret_name(&format!(
            "{id}_{}",
            if key.is_empty() { "TOKEN" } else { &key }
        ));
        let mut name = base.clone();
        for n in 2.. {
            if !names.contains(&name) {
                break;
            }
            name = format!("{base}_{n}");
        }
        names.insert(name.clone());
        moved.insert(name.clone(), value.as_str().unwrap_or_default().to_string());
        *value = json!(format!("${{secret:{name}}}"));
    }
    moved
}

/// The plaintext credentials of `entry`, as (field, key): every header
/// value, the env values whose names or contents say they are credentials,
/// and the bearer token (key empty). Other env values, such as paths and
/// modes, stay readable in mcp.json. A value that refers to a variable or
/// secret is not one: its secret would hold the reference, which is never
/// expanded.
fn credential_fields(entry: &Map<String, Value>) -> Vec<(&'static str, String)> {
    let mut fields = Vec::new();
    for field in ["headers", "env"] {
        if let Some(Value::Object(values)) = entry.get(field) {
            for (key, value) in values {
                let Some(text) = value.as_str().filter(|text| is_plaintext(text)) else {
                    continue;
                };
                if field == "headers" || is_credential(key, text) {
                    fields.push((field, key.clone()));
                }
            }
        }
    }
    if entry
        .get("bearer_token")
        .and_then(Value::as_str)
        .is_some_and(is_plaintext)
    {
        fields.push(("bearer_token", String::new()));
    }
    fields
}

/// Whether an environment variable holds a credential: its name has a word
/// such as TOKEN or KEY, or its value is a URL with a password in it.
fn is_credential(name: &str, value: &str) -> bool {
    const WORDS: &[&str] = &[
        "token",
        "tokens",
        "secret",
        "secrets",
        "password",
        "passwd",
        "pwd",
        "pass",
        "credential",
        "credentials",
        "key",
        "keys",
        "apikey",
        "auth",
        "authorization",
        "bearer",
        "cookie",
        "pat",
        "jwt",
    ];
    name.to_ascii_lowercase()
        .split(['_', '-', '.'])
        .any(|word| WORDS.contains(&word))
        || reqwest::Url::parse(value).is_ok_and(|url| url.password().is_some())
}

/// A non-empty value that refers to no variable or secret.
fn is_plaintext(value: &str) -> bool {
    !value.trim().is_empty() && !has_reference(value)
}

fn has_reference(value: &str) -> bool {
    value.match_indices('$').any(|(at, _)| {
        value[at + 1..]
            .chars()
            .next()
            .is_some_and(|ch| ch == '{' || ch == '_' || ch.is_ascii_alphabetic())
    })
}

/// The fields of `entry` holding plaintext credentials, as the scan shows
/// them.
fn plaintext_fields(entry: &Map<String, Value>) -> Vec<String> {
    credential_fields(entry)
        .into_iter()
        .map(|(field, key)| match key.as_str() {
            "" => field.to_string(),
            _ => format!("{field}.{key}"),
        })
        .collect()
}

/// The id a server named `name` elsewhere gets: its name, or for a name
/// such as `microsoft/markitdown` the last part, with the characters an id
/// should not hold replaced.
fn import_id(name: &str) -> String {
    let name = name.trim();
    let base = name
        .rsplit('/')
        .find(|part| !part.trim().is_empty())
        .unwrap_or(name);
    let id: String = base
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    match id.trim_matches('-') {
        "" => "mcp".to_string(),
        id => id.to_string(),
    }
}

fn string_values(entry: &Map<String, Value>) -> Vec<&str> {
    let mut values = Vec::new();
    for value in entry.values() {
        match value {
            Value::String(text) => values.push(text.as_str()),
            Value::Array(items) => values.extend(items.iter().filter_map(Value::as_str)),
            Value::Object(object) => values.extend(object.values().filter_map(Value::as_str)),
            _ => {}
        }
    }
    values
}

/// Reads a configuration file: `None` when there is none.
async fn read_config(path: &Path, format: Format) -> Result<Option<Value>, String> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("the file is too large to read".to_string());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| err.to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let root = match format {
        Format::Codex => toml::from_str::<toml::Value>(text)
            .map_err(|err| format!("not valid TOML: {}", err.message()))
            .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string()))?,
        _ => serde_json::from_str(&strip_jsonc(text))
            .map_err(|err| format!("not valid JSON: {err}"))?,
    };
    Ok(Some(root))
}

/// The key `servers_in` hands VS Code's inputs over under.
const INPUTS_KEY: &str = "\u{0}inputs";

/// The servers in a configuration file, and the projects it names.
fn servers_in(root: &Value, format: Format) -> (Vec<RawServer>, Vec<PathBuf>) {
    let entries = |value: Option<&Value>, project: Option<PathBuf>| -> Vec<RawServer> {
        value
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, value)| RawServer {
                name: name.trim().to_string(),
                value: value.clone(),
                project: project.clone(),
            })
            .collect()
    };
    match format {
        Format::McpServers => (entries(root.get("mcpServers"), None), Vec::new()),
        Format::Codex => (entries(root.get("mcp_servers"), None), Vec::new()),
        Format::Vscode => {
            let mut servers = entries(root.get("servers"), None);
            if let Some(inputs) = root.get("inputs") {
                for server in &mut servers {
                    if let Value::Object(object) = &mut server.value {
                        object.insert(INPUTS_KEY.into(), inputs.clone());
                    }
                }
            }
            (servers, Vec::new())
        }
        Format::ClaudeUser => {
            let mut servers = entries(root.get("mcpServers"), None);
            let mut projects = Vec::new();
            for (path, project) in root
                .get("projects")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let path = PathBuf::from(path);
                servers.extend(entries(project.get("mcpServers"), Some(path.clone())));
                projects.push(path);
            }
            (servers, projects)
        }
    }
}

/// VS Code's inputs by id, with what each asks for.
fn vscode_inputs(inputs: &Value) -> BTreeMap<String, String> {
    inputs
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|input| {
            let id = input.get("id")?.as_str()?.trim().to_string();
            let description = input
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("VS Code input {id}"));
            Some((id, description))
        })
        .collect()
}

/// Drops `//` and `/* */` comments and trailing commas, which VS Code allows
/// in its JSON files.
fn strip_jsonc(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            stripped.push(ch);
            match ch {
                '\\' => stripped.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (ch, chars.peek()) {
            ('"', _) => {
                in_string = true;
                stripped.push(ch);
            }
            ('/', Some('/')) => {
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                stripped.push(' ');
            }
            _ => stripped.push(ch),
        }
    }

    // Trailing commas, outside strings.
    let mut out = String::with_capacity(stripped.len());
    let mut in_string = false;
    let mut escaped = false;
    for (at, ch) in stripped.char_indices() {
        if in_string {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ',' && stripped[at + 1..].trim_start().starts_with(['}', ']']) {
            continue;
        }
        out.push(ch);
    }
    out
}

/// Translates one client's entry into mcp.json's form.
struct Convert<'a> {
    user_home: &'a Path,
    /// The project of a project's configuration.
    project: Option<&'a Path>,
    /// VS Code inputs by id.
    inputs: &'a BTreeMap<String, String>,
    has_env: &'a (dyn Fn(&str) -> bool + Sync),
}

struct Translated {
    entry: Map<String, Value>,
    warnings: Vec<String>,
    /// The secrets the entry now refers to, with what each stood for.
    secrets: BTreeMap<String, String>,
}

/// Fields that say nothing about running a server.
const IGNORED_FIELDS: &[&str] = &[
    "$schema",
    "description",
    "title",
    "name",
    "id",
    "icon",
    "icons",
    "gallery",
    "version",
    "dev",
    INPUTS_KEY,
];

/// Fields of Anda's own mcp.json form, taken as they are.
const ANDA_FIELDS: &[&str] = &[
    "bearer_token",
    "include",
    "exclude",
    "startup",
    "lifecycle",
    "tasks",
    "timeouts",
    "concurrency",
    "limits",
    "inherit_env",
];

impl Convert<'_> {
    fn entry(&self, source: McpImportSource, raw: &Value) -> Result<Translated, String> {
        let Value::Object(raw) = raw else {
            return Err("the entry is not an object".to_string());
        };
        let mut raw = raw.clone();
        let mut warnings = Vec::new();
        if source == McpImportSource::Codex {
            raw = codex_entry(raw, &mut warnings)?;
        }
        let mut entry = Map::new();
        let mut secrets = BTreeMap::new();

        let kind = ["type", "transport", "transportType"]
            .iter()
            .find_map(|key| raw.remove(*key))
            .and_then(|value| value.as_str().map(|kind| kind.trim().to_ascii_lowercase()));
        let url = ["url", "serverUrl", "httpUrl"]
            .iter()
            .find_map(|key| raw.remove(*key))
            .and_then(|value| value.as_str().map(str::to_string));
        let command = raw
            .remove("command")
            .and_then(|value| value.as_str().map(str::to_string));
        let stdio = match kind.as_deref() {
            Some("stdio" | "local") => true,
            Some("http" | "streamable-http" | "streamable_http" | "streamablehttp") => false,
            Some("sse") => {
                return Err(
                    "it uses the SSE transport, which Anda does not support; add the server's Streamable HTTP endpoint instead if it has one"
                        .to_string(),
                );
            }
            Some(other) => return Err(format!("it uses the {other} transport")),
            None if command.is_some() => true,
            None if url.is_some() => false,
            None => return Err("it has neither a command nor a url".to_string()),
        };

        if stdio {
            let command = command.ok_or("it has no command")?;
            entry.insert("type".into(), json!("stdio"));
            entry.insert(
                "command".into(),
                json!(self.text(&command, "command", &mut secrets)?),
            );
            if let Some(args) = raw.remove("args") {
                let args = args
                    .as_array()
                    .ok_or("args is not a list")?
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        self.text(&scalar(arg)?, &format!("args[{index}]"), &mut secrets)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if !args.is_empty() {
                    entry.insert("args".into(), json!(args));
                }
            }
            for key in ["env", "environment"] {
                if let Some(env) = raw.remove(key) {
                    entry.insert("env".into(), self.map(&env, "env", &mut secrets)?);
                }
            }
            if let Some(cwd) = raw
                .remove("cwd")
                .and_then(|cwd| cwd.as_str().map(str::to_string))
            {
                entry.insert("cwd".into(), json!(self.text(&cwd, "cwd", &mut secrets)?));
            }
            if raw.remove("envFile").is_some() {
                warnings.push(
                    "envFile was not imported; add its variables to env or as secrets".to_string(),
                );
            }
        } else {
            let url = url.ok_or("it has no url")?;
            entry.insert("type".into(), json!("http"));
            entry.insert("url".into(), json!(self.text(&url, "url", &mut secrets)?));
            if let Some(headers) = raw.remove("headers") {
                entry.insert(
                    "headers".into(),
                    self.map(&headers, "headers", &mut secrets)?,
                );
            }
            if raw.remove("headersHelper").is_some() {
                warnings.push(
                    "headersHelper was not imported; set the headers it makes as secrets"
                        .to_string(),
                );
            }
            if let Some(oauth) = self.oauth(&mut raw, &mut warnings) {
                entry.insert("oauth".into(), oauth);
            }
        }
        if let Some(token) = raw
            .remove("bearer_token")
            .and_then(|token| token.as_str().map(str::to_string))
        {
            entry.insert(
                "bearer_token".into(),
                json!(self.text(&token, "bearer_token", &mut secrets)?),
            );
        }

        let disabled = raw.remove("disabled") == Some(json!(true));
        let enabled = raw.remove("enabled");
        if disabled || enabled == Some(json!(false)) {
            entry.insert("enabled".into(), json!(false));
        }
        // Windsurf's tools turned off, and Copilot's allowlist.
        if let Some(tools) = raw.remove("disabledTools").and_then(string_list) {
            entry.insert("exclude".into(), json!(tools));
        }
        if let Some(tools) = raw.remove("tools").and_then(string_list)
            && !tools.iter().any(|tool| tool == "*")
        {
            entry.insert("include".into(), json!(tools));
        }
        for key in ANDA_FIELDS {
            if let Some(value) = raw.remove(*key) {
                entry.entry(key.to_string()).or_insert(value);
            }
        }
        if ["alwaysAllow", "autoApprove"]
            .iter()
            .any(|key| raw.remove(*key).is_some())
        {
            warnings.push(
                "its auto-approved tools were not imported; set approvals in Anda".to_string(),
            );
        }
        let mut left: Vec<&str> = raw
            .keys()
            .map(String::as_str)
            .filter(|key| !IGNORED_FIELDS.contains(key))
            .collect();
        left.sort_unstable();
        if !left.is_empty() {
            warnings.push(format!("not imported: {}", left.join(", ")));
        }
        Ok(Translated {
            entry,
            warnings,
            secrets,
        })
    }

    /// An OAuth marker from Claude Code's `oauth` or Cursor's `auth`.
    fn oauth(&self, raw: &mut Map<String, Value>, warnings: &mut Vec<String>) -> Option<Value> {
        let auth = raw.remove("oauth").or_else(|| raw.remove("auth"))?;
        let auth = auth.as_object()?;
        let field = |names: &[&str]| names.iter().find_map(|name| auth.get(*name));
        let mut oauth = Map::new();
        if let Some(client_id) =
            field(&["clientId", "client_id", "CLIENT_ID"]).and_then(Value::as_str)
        {
            oauth.insert("client_id".into(), json!(client_id));
        }
        let scopes = match field(&["scopes", "scope"]) {
            Some(Value::String(scopes)) => scopes
                .split([' ', ','])
                .filter(|scope| !scope.is_empty())
                .map(str::to_string)
                .collect(),
            Some(scopes) => string_list(scopes.clone()).unwrap_or_default(),
            None => Vec::new(),
        };
        if !scopes.is_empty() {
            oauth.insert("scopes".into(), json!(scopes));
        }
        if field(&["clientSecret", "client_secret", "CLIENT_SECRET"]).is_some() {
            warnings.push(
                "its OAuth client secret was not imported; Anda signs in as a public client"
                    .to_string(),
            );
        }
        Some(Value::Object(oauth))
    }

    /// An object of string values, translated.
    fn map(
        &self,
        value: &Value,
        field: &str,
        secrets: &mut BTreeMap<String, String>,
    ) -> Result<Value, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{field} is not an object"))?;
        let mut out = Map::new();
        for (key, value) in object {
            if value.is_null() {
                continue;
            }
            let text = scalar(value)?;
            out.insert(
                key.clone(),
                json!(self.text(&text, &format!("{field}.{key}"), secrets)?),
            );
        }
        Ok(Value::Object(out))
    }

    /// Translates the variables in one value into Anda's: `${env:X}` and
    /// `${X}` stay variables when the daemon has them and become secrets
    /// when it does not, `${input:x}` becomes a secret, and VS Code's
    /// `${userHome}` and `${workspaceFolder}` are filled in.
    fn text(
        &self,
        value: &str,
        field: &str,
        secrets: &mut BTreeMap<String, String>,
    ) -> Result<String, String> {
        let mut out = String::with_capacity(value.len());
        let mut rest = value;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            if let Some(braced) = after.strip_prefix('{') {
                let end = braced
                    .find('}')
                    .ok_or_else(|| format!("{field} has an unterminated ${{"))?;
                out.push_str(&self.reference(&braced[..end], field, secrets)?);
                rest = &braced[end + 1..];
            } else if after.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic()) {
                let end = after
                    .find(|ch: char| ch != '_' && !ch.is_ascii_alphanumeric())
                    .unwrap_or(after.len());
                out.push_str(&self.reference(&after[..end], field, secrets)?);
                rest = &after[end..];
            } else {
                out.push('$');
                rest = after;
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn reference(
        &self,
        reference: &str,
        field: &str,
        secrets: &mut BTreeMap<String, String>,
    ) -> Result<String, String> {
        let (name, default) = match reference.split_once(":-") {
            Some((name, default)) => (name.trim(), Some(default)),
            None => (reference.trim(), None),
        };
        if let Some(input) = name.strip_prefix("input:") {
            let secret = secret_name(input);
            let description = self
                .inputs
                .get(input.trim())
                .cloned()
                .unwrap_or_else(|| format!("VS Code input {input}"));
            secrets.insert(secret.clone(), description);
            return Ok(format!("${{secret:{secret}}}"));
        }
        if let Some(secret) = name.strip_prefix("secret:")
            && is_secret_name(secret)
        {
            return Ok(format!("${{{reference}}}"));
        }
        match name {
            "userHome" => return Ok(self.user_home.display().to_string()),
            "workspaceFolder" => {
                return Ok(match self.project {
                    Some(project) => project.display().to_string(),
                    None => "${ANDA_WORKSPACE}".to_string(),
                });
            }
            "workspaceFolderBasename" => {
                if let Some(base) = self.project.and_then(Path::file_name) {
                    return Ok(base.to_string_lossy().into_owned());
                }
            }
            "pathSeparator" | "/" => return Ok(std::path::MAIN_SEPARATOR.to_string()),
            _ => {}
        }
        let variable = name.strip_prefix("env:").unwrap_or(name);
        if !is_secret_name(variable) {
            return Err(format!(
                "{field} uses ${{{reference}}}, which Anda cannot fill in"
            ));
        }
        let keep = default.is_some()
            || matches!(variable, "ANDA_HOME" | "ANDA_WORKSPACE")
            || (self.has_env)(variable);
        let name = if keep {
            variable.to_string()
        } else {
            // The daemon runs without the shell's environment: what the
            // other client read from it is a secret here.
            secrets
                .entry(variable.to_string())
                .or_insert_with(|| format!("environment variable {variable}"));
            format!("secret:{variable}")
        };
        Ok(match default {
            Some(default) => format!("${{{name}:-{default}}}"),
            None => format!("${{{name}}}"),
        })
    }
}

/// A Codex `[mcp_servers.<name>]` table in the common form.
fn codex_entry(
    mut raw: Map<String, Value>,
    warnings: &mut Vec<String>,
) -> Result<Map<String, Value>, String> {
    let mut entry = Map::new();
    for key in ["command", "args", "env", "cwd", "url", "bearer_token"] {
        if let Some(value) = raw.remove(key) {
            entry.insert(key.into(), value);
        }
    }
    if let Some(names) = raw.remove("env_vars").and_then(string_list) {
        let env = entry
            .entry("env")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("env is not a table")?;
        for name in names {
            env.entry(name.clone())
                .or_insert_with(|| json!(format!("${{{name}}}")));
        }
    }
    if let Some(variable) = raw.remove("bearer_token_env_var")
        && let Some(variable) = variable.as_str()
    {
        entry.insert("bearer_token".into(), json!(format!("${{{variable}}}")));
    }
    let mut headers = match raw.remove("http_headers") {
        Some(Value::Object(headers)) => headers,
        _ => Map::new(),
    };
    if let Some(Value::Object(from_env)) = raw.remove("env_http_headers") {
        for (name, variable) in from_env {
            if let Some(variable) = variable.as_str() {
                headers.insert(name, json!(format!("${{{variable}}}")));
            }
        }
    }
    if !headers.is_empty() {
        entry.insert("headers".into(), Value::Object(headers));
    }
    if raw.remove("enabled") == Some(json!(false)) {
        entry.insert("enabled".into(), json!(false));
    }
    if let Some(tools) = raw.remove("enabled_tools").and_then(string_list) {
        entry.insert("include".into(), json!(tools));
    }
    if let Some(tools) = raw.remove("disabled_tools").and_then(string_list) {
        entry.insert("exclude".into(), json!(tools));
    }
    let mut timeouts = Map::new();
    let seconds = |value: Option<Value>, scale: f64| {
        value
            .and_then(|value| value.as_f64())
            .filter(|secs| *secs > 0.0)
            .map(|secs| (secs / scale).ceil() as u64)
    };
    if let Some(secs) = seconds(raw.remove("startup_timeout_sec"), 1.0)
        .or_else(|| seconds(raw.remove("startup_timeout_ms"), 1000.0))
    {
        timeouts.insert("setup_secs".into(), json!(secs));
    }
    if let Some(secs) = seconds(raw.remove("tool_timeout_sec"), 1.0) {
        timeouts.insert("call_secs".into(), json!(secs));
    }
    if !timeouts.is_empty() {
        entry.insert("timeouts".into(), Value::Object(timeouts));
    }
    // Anda never lets one server hold up the daemon.
    raw.remove("required");
    let mut left: Vec<&str> = raw.keys().map(String::as_str).collect();
    left.sort_unstable();
    if !left.is_empty() {
        warnings.push(format!("not imported: {}", left.join(", ")));
    }
    Ok(entry)
}

/// A list of strings, or `None` when the value is not one.
fn string_list(value: Value) -> Option<Vec<String>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| item.as_str().map(|text| text.trim().to_string()))
            .collect::<Option<Vec<_>>>()
            .map(|items| items.into_iter().filter(|item| !item.is_empty()).collect()),
        _ => None,
    }
}

/// A string, number or boolean as text.
fn scalar(value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        _ => Err("a value is not text".to_string()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A user's home with every client's configuration, and a project.
    pub(crate) struct Fixture {
        pub _dir: tempfile::TempDir,
        pub ctx: McpImportContext,
    }

    pub(crate) async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app");
        let project = dir.path().join("project");
        let write = |path: PathBuf, content: String| async move {
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(path, content).await.unwrap();
        };
        write(
            app_data.join("Claude/claude_desktop_config.json"),
            json!({ "mcpServers": {
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"],
                    "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_plaintext" }
                },
                "events": { "type": "sse", "url": "https://events.example/sse" }
            }})
            .to_string(),
        )
        .await;
        write(
            home.join(".claude.json"),
            json!({
                "numStartups": 42,
                "mcpServers": {
                    "linear": {
                        "type": "http",
                        "url": "https://mcp.linear.app/mcp",
                        "oauth": { "clientId": "public-client" }
                    }
                },
                "projects": { project.display().to_string(): { "mcpServers": {
                    "db": { "command": "pg-mcp", "args": ["${DATABASE_URL}"] }
                }}}
            })
            .to_string(),
        )
        .await;
        write(
            project.join(".mcp.json"),
            json!({ "mcpServers": { "docs": {
                "type": "http",
                "url": "https://docs.example/mcp",
                "headers": { "Authorization": "Bearer ${DOCS_TOKEN:-anonymous}" }
            }}})
            .to_string(),
        )
        .await;
        write(
            home.join(".cursor/mcp.json"),
            json!({ "mcpServers": {
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"],
                    "env": { "TOOL_PATH": "${env:PATH}" }
                },
                "notes": {
                    "url": "https://notes.example/mcp",
                    "auth": { "CLIENT_ID": "notes-client", "CLIENT_SECRET": "shh", "scopes": ["read"] }
                }
            }})
            .to_string(),
        )
        .await;
        write(
            app_data.join("Code/User/mcp.json"),
            r#"{
                // Asked for when the server starts.
                "inputs": [
                    { "id": "search-key", "type": "promptString", "description": "Search API key", "password": true },
                ],
                "servers": {
                    /* the local search index */
                    "search": {
                        "type": "stdio",
                        "command": "search-mcp",
                        "args": ["--root", "${userHome}/notes", "--url", "http://x.test/a//b"],
                        "env": { "API_KEY": "${input:search-key}", "MODE": "fast", "SEARCH_TOKEN": "tok-plain" },
                        "envFile": "${workspaceFolder}/.env",
                    },
                },
            }"#
            .to_string(),
        )
        .await;
        write(
            home.join(".codeium/windsurf/mcp_config.json"),
            json!({ "mcpServers": { "wiki": {
                "serverUrl": "https://wiki.example/mcp",
                "disabledTools": ["delete_page"],
                "disabled": true
            }}})
            .to_string(),
        )
        .await;
        write(
            home.join(".codex/config.toml"),
            r#"
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
env_vars = ["CONTEXT7_API_KEY"]
startup_timeout_sec = 20
tool_timeout_sec = 120.5
enabled_tools = ["resolve-library-id"]

[mcp_servers.remote]
url = "https://remote.example/mcp"
bearer_token_env_var = "REMOTE_TOKEN"
"#
            .to_string(),
        )
        .await;
        Fixture {
            ctx: McpImportContext {
                user_home: home,
                app_data,
                workspaces: Vec::new(),
            },
            _dir: dir,
        }
    }

    fn known() -> Vec<McpKnownServer> {
        let linear = McpSettings::parse_entry(
            "linear",
            &json!({ "type": "http", "url": "https://mcp.linear.app/mcp/" }),
        )
        .unwrap();
        let context7 =
            McpSettings::parse_entry("context7", &json!({ "command": "context7-mcp" })).unwrap();
        vec![
            McpKnownServer::of(&linear),
            McpKnownServer::of(&context7),
            McpKnownServer {
                id: "broken".into(),
                endpoint: None,
            },
        ]
    }

    pub(crate) async fn scan_fixture(fixture: &Fixture) -> McpImportScan {
        let known = known();
        let secrets = BTreeSet::new();
        // Only what every environment has: a variable kept is checked
        // against the real one.
        let has_env = |name: &str| name == "PATH";
        let target = McpImportTarget {
            known: &known,
            secrets: &secrets,
            has_env: &has_env,
        };
        scan(&fixture.ctx, &[], &target).await
    }

    fn find<'a>(
        scan: &'a McpImportScan,
        source: McpImportSource,
        name: &str,
    ) -> &'a McpImportCandidate {
        scan.candidates
            .iter()
            .find(|candidate| candidate.source == source && candidate.name == name)
            .unwrap_or_else(|| panic!("{name} from {source:?} was not found"))
    }

    fn entry(candidate: &McpImportCandidate) -> Value {
        Value::Object(candidate.entry.clone().expect("a translated entry"))
    }

    #[tokio::test]
    async fn a_scan_reads_each_client_and_translates_its_servers() {
        let fixture = fixture().await;
        let scan = scan_fixture(&fixture).await;
        let counts: Vec<(McpImportSource, usize)> = scan
            .files
            .iter()
            .map(|file| (file.source, file.servers))
            .collect();
        assert_eq!(
            counts,
            [
                (McpImportSource::ClaudeDesktop, 2),
                (McpImportSource::ClaudeCode, 2),
                (McpImportSource::Cursor, 2),
                (McpImportSource::Vscode, 1),
                (McpImportSource::Windsurf, 1),
                (McpImportSource::Codex, 2),
                // The project Claude Code knows, read for its `.mcp.json`.
                (McpImportSource::ClaudeCode, 1),
            ]
        );

        let github = find(&scan, McpImportSource::ClaudeDesktop, "github");
        assert_eq!(github.status, McpImportStatus::New);
        assert_eq!(github.summary, "npx -y @modelcontextprotocol/server-github");
        assert_eq!(github.plaintext, ["env.GITHUB_PERSONAL_ACCESS_TOKEN"]);
        let events = find(&scan, McpImportSource::ClaudeDesktop, "events");
        assert_eq!(events.status, McpImportStatus::Invalid);
        assert!(events.error.as_deref().unwrap().contains("SSE"));

        let linear = find(&scan, McpImportSource::ClaudeCode, "linear");
        assert_eq!(
            (linear.status, linear.existing_id.as_deref()),
            (McpImportStatus::Exists, Some("linear"))
        );
        assert_eq!(
            entry(linear)["oauth"],
            json!({ "client_id": "public-client" })
        );
        // A variable the daemon lacks becomes a secret to set.
        let db = find(&scan, McpImportSource::ClaudeCode, "db");
        assert!(db.project.is_some());
        assert_eq!(entry(db)["args"], json!(["${secret:DATABASE_URL}"]));
        assert_eq!(
            db.needs_secrets,
            [McpImportSecret {
                name: "DATABASE_URL".into(),
                description: "environment variable DATABASE_URL".into(),
            }]
        );
        let docs = find(&scan, McpImportSource::ClaudeCode, "docs");
        assert_eq!(docs.status, McpImportStatus::New);
        assert_eq!(
            entry(docs)["headers"]["Authorization"],
            "Bearer ${DOCS_TOKEN:-anonymous}"
        );
        assert!(docs.needs_secrets.is_empty() && docs.plaintext.is_empty());

        // The same command line as Claude Desktop's github, listed earlier.
        let cursor_github = find(&scan, McpImportSource::Cursor, "github");
        assert_eq!(cursor_github.status, McpImportStatus::Duplicate);
        assert_eq!(
            cursor_github.duplicate_of.as_deref(),
            Some(github.key.as_str())
        );
        assert_eq!(entry(cursor_github)["env"]["TOOL_PATH"], "${PATH}");
        let notes = find(&scan, McpImportSource::Cursor, "notes");
        assert_eq!(
            entry(notes)["oauth"],
            json!({ "client_id": "notes-client", "scopes": ["read"] })
        );
        assert!(notes.warnings[0].contains("client secret"));

        let search = find(&scan, McpImportSource::Vscode, "search");
        assert_eq!(search.status, McpImportStatus::New);
        let home = fixture.ctx.user_home.display().to_string();
        assert_eq!(entry(search)["args"][1], format!("{home}/notes"));
        assert_eq!(entry(search)["args"][3], "http://x.test/a//b");
        assert_eq!(entry(search)["env"]["API_KEY"], "${secret:SEARCH_KEY}");
        assert_eq!(search.plaintext, ["env.SEARCH_TOKEN"]);
        assert_eq!(search.needs_secrets[0].description, "Search API key");
        assert!(search.warnings[0].starts_with("envFile"));

        let wiki = find(&scan, McpImportSource::Windsurf, "wiki");
        assert!(!wiki.enabled);
        assert_eq!(entry(wiki)["exclude"], json!(["delete_page"]));
        assert_eq!(entry(wiki)["url"], "https://wiki.example/mcp");

        // Anda's context7 runs something else, so this one is renamed.
        let context7 = find(&scan, McpImportSource::Codex, "context7");
        assert_eq!(
            (context7.status, context7.id.as_str()),
            (McpImportStatus::Renamed, "context7-codex")
        );
        let context7 = entry(context7);
        assert_eq!(context7["include"], json!(["resolve-library-id"]));
        assert_eq!(
            context7["timeouts"],
            json!({ "setup_secs": 20, "call_secs": 121 })
        );
        assert_eq!(
            context7["env"]["CONTEXT7_API_KEY"],
            "${secret:CONTEXT7_API_KEY}"
        );
        let remote = find(&scan, McpImportSource::Codex, "remote");
        assert_eq!(entry(remote)["bearer_token"], "${secret:REMOTE_TOKEN}");

        // What a scan shows holds no value.
        let shown = serde_json::to_string(&scan).unwrap();
        assert!(
            !shown.contains("ghp_plaintext") && !shown.contains("shh"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn a_plan_moves_plaintext_isolates_local_servers_and_checks_ids() {
        let fixture = fixture().await;
        let scan = scan_fixture(&fixture).await;
        let key = |source, name| find(&scan, source, name).key.clone();
        let picks = vec![
            McpImportPick {
                key: key(McpImportSource::ClaudeDesktop, "github"),
                id: None,
            },
            McpImportPick {
                key: key(McpImportSource::Codex, "context7"),
                id: Some(" c7 ".into()),
            },
            McpImportPick {
                key: key(McpImportSource::Vscode, "search"),
                id: None,
            },
        ];
        let taken = BTreeSet::from(["GITHUB_GITHUB_PERSONAL_ACCESS_TOKEN".to_string()]);
        let values = McpSecretValues::from([("SEARCH_KEY".to_string(), " key-1 ".to_string())]);
        let planned = plan(&scan, &picks, &values, true, |id| id == "taken", &taken).unwrap();
        let ids: Vec<&str> = planned
            .servers
            .iter()
            .map(|(server, _)| server.id.as_str())
            .collect();
        assert_eq!(ids, ["github", "c7", "search"]);
        let (github, path) = &planned.servers[0];
        assert!(path.ends_with("claude_desktop_config.json"));
        let McpTransportSettings::Stdio(stdio) = &github.transport else {
            panic!("github runs a command");
        };
        assert_eq!(stdio.inherit_env, Some(false));
        assert_eq!(
            stdio.env["GITHUB_PERSONAL_ACCESS_TOKEN"],
            "${secret:GITHUB_GITHUB_PERSONAL_ACCESS_TOKEN_2}"
        );
        assert_eq!(
            planned.secrets,
            BTreeMap::from([
                (
                    "GITHUB_GITHUB_PERSONAL_ACCESS_TOKEN_2".to_string(),
                    "ghp_plaintext".to_string()
                ),
                ("SEARCH_KEY".to_string(), "key-1".to_string()),
                ("SEARCH_SEARCH_TOKEN".to_string(), "tok-plain".to_string()),
            ])
        );

        // Kept as it is when asked.
        let kept = plan(
            &scan,
            &picks[..1],
            &BTreeMap::new(),
            false,
            |_| false,
            &taken,
        )
        .unwrap();
        let McpTransportSettings::Stdio(stdio) = &kept.servers[0].0.transport else {
            panic!("github runs a command");
        };
        assert_eq!(stdio.env["GITHUB_PERSONAL_ACCESS_TOKEN"], "ghp_plaintext");
        assert!(kept.secrets.is_empty());

        let pick = |source, name, id: Option<&str>| McpImportPick {
            key: key(source, name),
            id: id.map(str::to_string),
        };
        for (picks, values, message) in [
            (
                vec![pick(McpImportSource::ClaudeCode, "linear", None)],
                BTreeMap::new(),
                "already configured as linear",
            ),
            (
                vec![pick(McpImportSource::ClaudeDesktop, "events", None)],
                BTreeMap::new(),
                "SSE",
            ),
            (
                vec![pick(McpImportSource::Windsurf, "wiki", Some("taken"))],
                BTreeMap::new(),
                "already exists",
            ),
            (
                vec![
                    pick(McpImportSource::Windsurf, "wiki", Some("same")),
                    pick(McpImportSource::Codex, "remote", Some("same")),
                ],
                BTreeMap::new(),
                "already exists",
            ),
            (
                vec![pick(McpImportSource::Windsurf, "wiki", None)],
                BTreeMap::from([("SEARCH_KEY".to_string(), "x".to_string())]),
                "do not need the secret SEARCH_KEY",
            ),
            (
                vec![McpImportPick {
                    key: "cursor:/nowhere#x".into(),
                    id: None,
                }],
                BTreeMap::new(),
                "scan again",
            ),
            (Vec::new(), BTreeMap::new(), "at least one"),
        ] {
            let err = plan(&scan, &picks, &values, true, |id| id == "taken", &taken).unwrap_err();
            assert!(err.to_string().contains(message), "{err}");
        }
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_are_dropped_outside_strings() {
        let text = r#"{
            "a": "// stays", /* gone */ "b": [1, 2,],
            "c": "a /* stays */ b", // gone
            "d": "\"quoted\", ]",
        }"#;
        let value: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(
            value,
            json!({ "a": "// stays", "b": [1, 2], "c": "a /* stays */ b", "d": "\"quoted\", ]" })
        );
    }

    #[test]
    fn secret_names_are_upper_case_words() {
        assert_eq!(secret_name("github-token"), "GITHUB_TOKEN");
        assert_eq!(secret_name("docs_Authorization"), "DOCS_AUTHORIZATION");
        assert_eq!(secret_name("1password key"), "_1PASSWORD_KEY");
        assert_eq!(import_id("microsoft/markitdown"), "markitdown");
        assert_eq!(import_id(" my server "), "my-server");
        assert_eq!(import_id("/"), "mcp");
        for (name, value, credential) in [
            ("GITHUB_PERSONAL_ACCESS_TOKEN", "ghp_1", true),
            ("OPENAI_API_KEY", "sk-1", true),
            ("DB_PASSWORD", "p", true),
            ("DATABASE_URL", "postgres://app:pw@db/app", true),
            ("DATABASE_URL", "postgres://db/app", false),
            ("GIT_AUTHOR_NAME", "Ada", false),
            ("MEMORY_FILE_PATH", "/tmp/memory.json", false),
            ("NODE_ENV", "production", false),
        ] {
            assert_eq!(is_credential(name, value), credential, "{name}");
        }
        assert!(is_plaintext("Bearer abc$"));
        assert!(!is_plaintext("Bearer ${TOKEN}"));
        assert!(!is_plaintext("$TOKEN"));
        assert!(!is_plaintext("  "));
    }
}
