use crate::util::tool_response::ToolResponse as Response;
use anda_core::{
    BoxError, FunctionDefinition, Resource, Tool, ToolOutput, Usage, validate_function_name,
};
use anda_engine::{
    context::BaseCtx,
    extension::skill::{
        Skill, SkillArgs, SkillContentOutput, SkillExecution, SkillManager, SkillsReadArgs,
        SkillsReadOutput, format_skill_md, normalise_skill_agent_name, parse_skill_md,
        validate_skill_name,
    },
    hook::ToolHook,
    subagent::{SubAgent, SubAgentSet},
    unix_ms,
};
use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::sync::Mutex;

const MAX_SKILL_FILE_BYTES: u64 = 512 * 1024;
const MAX_SKILL_VIEW_FILE_BYTES: u64 = 1024 * 1024;
const MANIFEST_FILE_NAME: &str = "skills-manifest.json";
const BACKUPS_DIR_NAME: &str = "skill-backups";
const TRASH_DIR_NAME: &str = "skill-trash";
/// Before v0.10.5 a Personal skill that shadowed a bundled one was recorded
/// in the manifest as `legacy:<name>`.
const LEGACY_ID_PREFIX: &str = "legacy:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    Personal,
    Bundled,
    Shared,
}

impl SkillSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            SkillSourceKind::Personal => "personal",
            SkillSourceKind::Bundled => "bundled",
            SkillSourceKind::Shared => "shared",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SkillSourceKind::Personal => "Personal",
            SkillSourceKind::Bundled => "Bundled",
            SkillSourceKind::Shared => "Shared",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSourceInfo {
    pub source: SkillSourceKind,
    pub source_label: String,
    pub priority: u32,
    pub path: String,
    pub editable: bool,
    pub exists: bool,
    /// What the last scan could not read in this directory, such as an
    /// unreadable folder or a traversal limit. The skills it did reach are
    /// still listed.
    #[serde(default)]
    pub diagnostics: Vec<SkillDiagnostic>,
}

#[derive(Debug, Clone)]
struct SkillSource {
    kind: SkillSourceKind,
    /// Position in the registry's directory list, which is also its rank when
    /// two directories hold a skill of the same name.
    priority: u32,
    path: PathBuf,
}

impl SkillSource {
    fn editable(&self) -> bool {
        self.kind == SkillSourceKind::Personal
    }

    fn info(&self, diagnostics: Vec<SkillDiagnostic>) -> SkillSourceInfo {
        SkillSourceInfo {
            source: self.kind,
            source_label: self.kind.label().to_string(),
            priority: self.priority,
            path: self.path.display().to_string(),
            editable: self.editable(),
            exists: self.path.is_dir(),
            diagnostics,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDiagnostic {
    pub severity: SkillDiagnosticSeverity,
    pub code: String,
    pub message: String,
}

impl SkillDiagnostic {
    fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: SkillDiagnosticSeverity::Error,
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: SkillDiagnosticSeverity::Warning,
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisabledSkill {
    pub disabled_at: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub version: u32,
    #[serde(default)]
    pub disabled: BTreeMap<String, DisabledSkill>,
}

impl Default for SkillManifest {
    fn default() -> Self {
        Self {
            version: 1,
            disabled: BTreeMap::new(),
        }
    }
}

impl SkillManifest {
    /// Renames `legacy:<name>` entries to the Personal id they now belong to.
    fn migrate_legacy_ids(&mut self) {
        let legacy: Vec<String> = self
            .disabled
            .keys()
            .filter(|id| id.starts_with(LEGACY_ID_PREFIX))
            .cloned()
            .collect();
        for id in legacy {
            if let Some(entry) = self.disabled.remove(&id) {
                self.disabled
                    .entry(personal_skill_id(&id[LEGACY_ID_PREFIX.len()..]))
                    .or_insert(entry);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedSkill {
    pub id: String,
    pub source: SkillSourceKind,
    pub source_label: String,
    pub priority: u32,
    pub name: String,
    pub agent_name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    /// How the skill runs. `inline` skills are read into the calling agent's
    /// own context through `skills_manager`; only `subagent` ones become
    /// `SA_<agent_name>` callables.
    #[serde(default)]
    pub execution: SkillExecution,
    pub allowed_tools: Vec<String>,
    pub metadata: Value,
    pub path: String,
    pub directory: String,
    pub editable: bool,
    pub active: bool,
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadowed_by: Option<String>,
    pub diagnostics: Vec<SkillDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<SkillUsageSummary>,
    pub version: String,
}

impl ManagedSkill {
    fn has_error(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == SkillDiagnosticSeverity::Error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedSkillDetail {
    #[serde(flatten)]
    pub skill: ManagedSkill,
    pub content: String,
    pub files: Vec<SkillFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillFileEntry {
    pub path: String,
    pub name: String,
    pub kind: SkillFileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillFileContent {
    pub id: String,
    pub path: String,
    pub content: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillUsageSummary {
    pub callable: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationResult {
    pub valid: bool,
    pub diagnostics: Vec<SkillDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptSkill {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum SkillsApiArgs {
    ListSkillSources {},
    ListSkills {
        /// Strict tool calls send `null` for an unused field, which a plain
        /// `bool` would reject.
        #[serde(default)]
        include_inactive: Option<bool>,
    },
    GetSkill {
        id: String,
    },
    GetSkillFile {
        id: String,
        path: String,
    },
    CreateSkill {
        name: String,
        description: String,
        content: String,
    },
    UpdateSkill {
        id: String,
        content: String,
        #[serde(default)]
        expected_version: Option<String>,
    },
    CloneSkill {
        id: String,
        #[serde(default)]
        new_name: Option<String>,
    },
    SetSkillEnabled {
        id: String,
        enabled: bool,
    },
    DeletePersonalSkill {
        id: String,
    },
    ValidateSkill {
        content: String,
    },
    ReloadSkills {},
}

#[derive(Clone)]
struct SkillRecord {
    managed: ManagedSkill,
    content: String,
    base_dir: PathBuf,
}

#[derive(Default)]
struct SkillLibraryState {
    records: Vec<SkillRecord>,
    /// Scan problems per source, indexed like `SkillLibrary::sources`.
    source_diagnostics: Vec<Vec<SkillDiagnostic>>,
}

/// One `SKILL.md` the registry found, with the reason it refused to load it.
struct DiscoveredSkill {
    source: usize,
    path: PathBuf,
    rejection: Option<String>,
}

#[derive(Clone)]
pub struct SkillLibrary {
    personal_dir: PathBuf,
    /// Personal, Bundled, then each Shared directory: the registry's order.
    sources: Vec<SkillSource>,
    manifest_path: PathBuf,
    backups_dir: PathBuf,
    trash_dir: PathBuf,
    skill_manager: Arc<SkillManager>,
    known_tools: Arc<BTreeSet<String>>,
    tools_usage_reader: Arc<dyn Fn() -> HashMap<String, Usage> + Send + Sync>,
    operation_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<SkillLibraryState>>,
}

impl SkillLibrary {
    pub const NAME: &'static str = "skills_api";

    pub fn new(
        home_dir: PathBuf,
        personal_dir: PathBuf,
        bundled_dir: PathBuf,
        shared_dirs: Vec<PathBuf>,
        skill_manager: Arc<SkillManager>,
        known_tools: BTreeSet<String>,
    ) -> Self {
        let sources = [
            (SkillSourceKind::Personal, personal_dir.clone()),
            (SkillSourceKind::Bundled, bundled_dir),
        ]
        .into_iter()
        .chain(
            shared_dirs
                .into_iter()
                .map(|dir| (SkillSourceKind::Shared, dir)),
        )
        .enumerate()
        .map(|(priority, (kind, path))| SkillSource {
            kind,
            priority: priority as u32,
            path,
        })
        .collect();
        Self {
            manifest_path: home_dir.join(MANIFEST_FILE_NAME),
            backups_dir: home_dir.join(BACKUPS_DIR_NAME),
            trash_dir: home_dir.join(TRASH_DIR_NAME),
            personal_dir,
            sources,
            skill_manager,
            known_tools: Arc::new(known_tools),
            tools_usage_reader: Arc::new(HashMap::new),
            operation_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(SkillLibraryState::default())),
        }
    }

    pub fn with_tools_usage_reader<F>(mut self, reader: F) -> Self
    where
        F: Fn() -> HashMap<String, Usage> + Send + Sync + 'static,
    {
        self.tools_usage_reader = Arc::new(reader);
        self
    }

    #[cfg(test)]
    pub(crate) fn for_test(home_dir: PathBuf) -> Arc<Self> {
        let personal_dir = home_dir.join("skills");
        let bundled_dir = home_dir.join("bundled-skills");
        let shared_dir = home_dir.join("shared-skills");
        let default_skill_tools = vec![
            "shell".to_string(),
            "shell_session".to_string(),
            "read_file".to_string(),
            "search_file".to_string(),
            "tools_select".to_string(),
            "skills_read".to_string(),
        ];
        let raw = Arc::new(
            SkillManager::new_with_dirs(
                personal_dir.clone(),
                vec![bundled_dir.clone(), shared_dir.clone()],
            )
            .with_default_skill_tools(default_skill_tools.clone()),
        );
        Arc::new(Self::new(
            home_dir,
            personal_dir,
            bundled_dir,
            vec![shared_dir],
            raw,
            BTreeSet::from_iter(default_skill_tools),
        ))
    }

    pub fn skill_sources(&self) -> Vec<SkillSourceInfo> {
        let state = self.state.read();
        self.sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                source.info(
                    state
                        .source_diagnostics
                        .get(index)
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    #[cfg(test)]
    pub fn skill_manager(&self) -> Arc<SkillManager> {
        self.skill_manager.clone()
    }

    pub fn list_managed_skills(&self, include_inactive: bool) -> Vec<ManagedSkill> {
        let tools_usage = (self.tools_usage_reader)();
        self.state
            .read()
            .records
            .iter()
            .filter(|record| include_inactive || record.managed.active)
            .map(|record| attach_usage(record.managed.clone(), &tools_usage))
            .collect()
    }

    pub fn prompt_skills(&self) -> Vec<PromptSkill> {
        self.state
            .read()
            .records
            .iter()
            .filter(|record| record.managed.active)
            .map(|record| PromptSkill {
                name: record.managed.name.clone(),
                description: (!record.managed.description.is_empty())
                    .then(|| record.managed.description.clone()),
            })
            .collect()
    }

    pub async fn get_skill_detail(&self, id: &str) -> Result<ManagedSkillDetail, BoxError> {
        let tools_usage = (self.tools_usage_reader)();
        let record = self
            .record_by_id(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        let base_dir = record.base_dir.clone();
        let files = tokio::task::spawn_blocking(move || list_skill_files(&base_dir)).await?;
        Ok(ManagedSkillDetail {
            files,
            content: record.content,
            skill: attach_usage(record.managed, &tools_usage),
        })
    }

    pub async fn get_skill_file(&self, id: &str, path: &str) -> Result<SkillFileContent, BoxError> {
        let record = self
            .record_by_id(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        let id = id.to_string();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || read_skill_file(&id, &record.base_dir, &path)).await?
    }

    pub async fn reload(&self) -> Result<Vec<ManagedSkill>, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let manifest = self.load_manifest().await?;
        self.reload_locked(&manifest).await
    }

    pub async fn create_skill(
        &self,
        name: String,
        description: String,
        content: String,
    ) -> Result<ManagedSkillDetail, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let name = normalize_skill_name(name)?;
        let target_dir = self.personal_dir.join(&name);
        self.ensure_new_personal_skill_dir(&target_dir).await?;

        let content = normalize_new_skill_content(&name, &description, &content);
        let validation = validate_skill_content(Some(&name), &content);
        if !validation.valid {
            return Err(format!(
                "skill content is invalid: {}",
                diagnostic_summary(&validation.diagnostics)
            )
            .into());
        }

        tokio::fs::create_dir_all(&target_dir).await?;
        atomic_write_text(&target_dir.join("SKILL.md"), &content).await?;
        let manifest = self.load_manifest().await?;
        self.reload_locked(&manifest).await?;
        self.get_skill_detail(&personal_skill_id(&name)).await
    }

    pub async fn update_skill(
        &self,
        id: String,
        content: String,
        expected_version: Option<String>,
    ) -> Result<ManagedSkillDetail, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let record = self
            .record_by_id(&id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        if !record.managed.editable {
            return Err("only Personal skills can be updated from the Dashboard".into());
        }
        let skill_md = record.base_dir.join("SKILL.md");
        if let Some(expected_version) = expected_version
            && expected_version != content_version(&tokio::fs::read(&skill_md).await?)
        {
            return Err("skill changed on disk; reload before saving again".into());
        }

        let validation = validate_skill_content(Some(&record.managed.name), &content);
        if !validation.valid {
            return Err(format!(
                "skill content is invalid: {}",
                diagnostic_summary(&validation.diagnostics)
            )
            .into());
        }

        self.ensure_existing_personal_skill_dir(&record.base_dir)
            .await?;
        self.backup_skill_md(&record.managed.name, &skill_md)
            .await?;
        atomic_write_text(&skill_md, &content).await?;
        let manifest = self.load_manifest().await?;
        self.reload_locked(&manifest).await?;
        self.get_skill_detail(&id).await
    }

    pub async fn clone_skill(
        &self,
        id: String,
        new_name: Option<String>,
    ) -> Result<ManagedSkillDetail, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let record = self
            .record_by_id(&id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        let mut parsed = parse_skill_md(record.base_dir.clone(), &record.content)
            .map_err(|_| "only valid skills can be cloned")?;
        let new_name = match new_name {
            Some(name) => normalize_skill_name(name)?,
            None => self.available_clone_name(&record.managed.name)?,
        };
        let target_dir = self.personal_dir.join(&new_name);
        self.ensure_new_personal_skill_dir(&target_dir).await?;

        let source = record.base_dir.clone();
        let destination = target_dir.clone();
        tokio::task::spawn_blocking(move || copy_dir_regular_files(&source, &destination))
            .await??;
        parsed.frontmatter.name = new_name.clone();
        parsed.frontmatter.metadata.insert(
            "anda".to_string(),
            json!({
                "origin": id,
                "cloned_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
            }),
        );
        parsed.base_dir = target_dir.clone();
        let content = format_skill_md(&parsed)?;
        atomic_write_text(&target_dir.join("SKILL.md"), &content).await?;

        let manifest = self.load_manifest().await?;
        self.reload_locked(&manifest).await?;
        self.get_skill_detail(&personal_skill_id(&new_name)).await
    }

    pub async fn set_skill_enabled(
        &self,
        id: String,
        enabled: bool,
    ) -> Result<Vec<ManagedSkill>, BoxError> {
        let _guard = self.operation_lock.lock().await;
        if self.record_by_id(&id).is_none() {
            return Err(format!("skill not found: {id}").into());
        }
        let mut manifest = self.load_manifest().await?;
        if enabled {
            manifest.disabled.remove(&id);
        } else {
            manifest.disabled.insert(
                id,
                DisabledSkill {
                    disabled_at: unix_ms(),
                    reason: "User disabled from Dashboard".to_string(),
                },
            );
        }
        self.write_manifest(&manifest).await?;
        self.reload_locked(&manifest).await
    }

    pub async fn delete_personal_skill(&self, id: String) -> Result<Value, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let record = self
            .record_by_id(&id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        if !record.managed.editable {
            return Err("only Personal skills can be deleted from the Dashboard".into());
        }
        self.ensure_existing_personal_skill_dir(&record.base_dir)
            .await?;
        // Read the manifest before moving anything, so a broken one stops the
        // delete instead of leaving a stale disabled entry behind.
        let mut manifest = self.load_manifest().await?;

        let trash_dir = unique_path(
            self.trash_dir
                .join(&record.managed.name)
                .join(timestamp_for_path()),
        );
        if let Some(parent) = trash_dir.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&record.base_dir, &trash_dir)
            .await
            .map_err(|err| format!("failed to move skill to trash: {err}"))?;

        if manifest.disabled.remove(&id).is_some() {
            self.write_manifest(&manifest).await?;
        }
        self.reload_locked(&manifest).await?;
        Ok(json!({
            "deleted": true,
            "id": id,
            "trash_path": trash_dir.display().to_string()
        }))
    }

    pub fn validate_skill(&self, content: String) -> SkillValidationResult {
        validate_skill_content(None, &content)
    }

    async fn reload_locked(&self, manifest: &SkillManifest) -> Result<Vec<ManagedSkill>, BoxError> {
        let (mut records, source_diagnostics) = self.scan_records().await?;
        apply_effective_state(&mut records, manifest);

        // Hand the registry the one decision it cannot make for itself, then let it
        // reload. `SkillManager` already resolves duplicate names by directory
        // priority and drops what it cannot parse; host diagnostics and the manifest's disabled set
        // live up here. Rejecting a disabled copy also promotes the next directory's
        // copy of that name, which is what the Dashboard switch is expected to do.
        let rejected_dirs: BTreeSet<PathBuf> = records
            .iter()
            .filter(|record| record.managed.disabled || record.managed.has_error())
            .map(|record| record.base_dir.clone())
            .collect();
        self.skill_manager
            .set_skill_filter(Some(Arc::new(move |skill: &Skill| {
                !rejected_dirs.contains(&skill.base_dir)
            })));

        *self.state.write() = SkillLibraryState {
            records,
            source_diagnostics,
        };
        if let Err(err) = self.skill_manager.load().await {
            log::warn!("failed to reload raw skills_manager after library scan: {err}");
        }
        Ok(self.list_managed_skills(true))
    }

    /// Every `SKILL.md` the registry would consider, plus each source's scan
    /// problems.
    ///
    /// Discovery runs through a throwaway `SkillManager` over the same
    /// directories, so the Dashboard sees exactly the files the registry does,
    /// including a partial scan. `find_skill_files` instead fails a whole
    /// directory over one unreadable folder or a deep `node_modules`, which hid
    /// skills that were still live and left them impossible to disable.
    async fn discover(
        &self,
    ) -> Result<(Vec<DiscoveredSkill>, Vec<Vec<SkillDiagnostic>>), BoxError> {
        let scanner = SkillManager::new_with_dirs(
            self.personal_dir.clone(),
            self.sources[1..]
                .iter()
                .map(|source| source.path.clone())
                .collect(),
        );
        scanner.reload().await?;
        let catalog = scanner.catalog();
        let source_of = |path: &Path| {
            self.sources
                .iter()
                .position(|source| path.starts_with(&source.path))
        };

        let mut found: Vec<DiscoveredSkill> = catalog
            .skills
            .iter()
            .filter_map(|skill| {
                Some(DiscoveredSkill {
                    source: source_of(&skill.base_dir)?,
                    path: skill.base_dir.join("SKILL.md"),
                    rejection: None,
                })
            })
            .collect();
        let mut source_diagnostics = vec![Vec::new(); self.sources.len()];
        for diagnostic in &catalog.report.diagnostics {
            let Some(source) = source_of(&diagnostic.path) else {
                continue;
            };
            if diagnostic.kind == "invalid"
                && diagnostic.path.file_name() == Some(OsStr::new("SKILL.md"))
            {
                found.push(DiscoveredSkill {
                    source,
                    path: diagnostic.path.clone(),
                    rejection: Some(diagnostic.message.clone()),
                });
            } else if diagnostic.kind != "conflict" {
                // Same-name conflicts are reported on the skills themselves.
                source_diagnostics[source].push(SkillDiagnostic::warning(
                    &diagnostic.kind,
                    format!("{}: {}", diagnostic.path.display(), diagnostic.message),
                ));
            }
        }
        Ok((found, source_diagnostics))
    }

    async fn scan_records(
        &self,
    ) -> Result<(Vec<SkillRecord>, Vec<Vec<SkillDiagnostic>>), BoxError> {
        let (found, source_diagnostics) = self.discover().await?;
        let sources = self.sources.clone();
        let known_tools = self.known_tools.clone();
        let mut records = tokio::task::spawn_blocking(move || {
            found
                .into_iter()
                .map(|skill| {
                    let mut record =
                        scan_skill_file(&sources[skill.source], &skill.path, &known_tools);
                    // Our own reading of a file the registry refused can come out
                    // clean (a bad `agents/openai.yaml`, say); it is still not loaded.
                    if let Some(message) = skill.rejection
                        && !record.managed.has_error()
                    {
                        record
                            .managed
                            .diagnostics
                            .push(SkillDiagnostic::error("rejected", message));
                    }
                    record
                })
                .collect::<Vec<_>>()
        })
        .await?;

        // Same-name copies are ordered like the registry orders them, by
        // directory rank and then by path, so both pick the same winner.
        records.sort_by(|left, right| {
            left.managed
                .priority
                .cmp(&right.managed.priority)
                .then_with(|| left.managed.name.cmp(&right.managed.name))
                .then_with(|| left.base_dir.cmp(&right.base_dir))
        });
        // Discovery is recursive, so one source can hold two skills of one name.
        // The first keeps the plain id the manifest already knows; later copies
        // get a suffix that stays stable while they stay where they are.
        let mut ids = HashSet::new();
        for record in &mut records {
            if !ids.insert(record.managed.id.clone()) {
                record.managed.id =
                    format!("{}@{}", record.managed.id, short_hash(&record.base_dir));
                ids.insert(record.managed.id.clone());
            }
        }
        Ok((records, source_diagnostics))
    }

    async fn load_manifest(&self) -> Result<SkillManifest, BoxError> {
        let bytes = match tokio::fs::read(&self.manifest_path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SkillManifest::default());
            }
            Err(err) => {
                return Err(format!(
                    "failed to read skill manifest {}: {err}",
                    self.manifest_path.display()
                )
                .into());
            }
        };
        // A broken manifest is reported rather than treated as empty: falling
        // back would re-enable every disabled skill, and the next write would
        // erase the user's file.
        let mut manifest: SkillManifest = anda_core::text_from_bytes(&bytes)
            .ok_or_else(|| "not readable as text".to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
            .map_err(|err| {
                format!(
                    "skill manifest {} is invalid, fix or remove it: {err}",
                    self.manifest_path.display()
                )
            })?;
        manifest.migrate_legacy_ids();
        Ok(manifest)
    }

    async fn write_manifest(&self, manifest: &SkillManifest) -> Result<(), BoxError> {
        let content = serde_json::to_string_pretty(manifest)?;
        atomic_write_text(&self.manifest_path, &(content + "\n")).await
    }

    fn record_by_id(&self, id: &str) -> Option<SkillRecord> {
        self.state
            .read()
            .records
            .iter()
            .find(|record| record.managed.id == id)
            .cloned()
    }

    async fn ensure_new_personal_skill_dir(&self, dir: &Path) -> Result<(), BoxError> {
        ensure_path_is_direct_child(&self.personal_dir, dir).await?;
        if tokio::fs::symlink_metadata(dir).await.is_ok() {
            return Err(format!("personal skill already exists: {}", dir.display()).into());
        }
        Ok(())
    }

    async fn ensure_existing_personal_skill_dir(&self, dir: &Path) -> Result<(), BoxError> {
        ensure_path_is_direct_child(&self.personal_dir, dir).await?;
        let meta = tokio::fs::symlink_metadata(dir).await.map_err(|err| {
            format!(
                "failed to inspect personal skill directory {}: {err}",
                dir.display()
            )
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("personal skill path must be a regular directory".into());
        }
        Ok(())
    }

    /// Keeps the `SKILL.md` an update is about to replace. An update writes
    /// nothing else, so nothing else is copied.
    async fn backup_skill_md(&self, skill_name: &str, skill_md: &Path) -> Result<(), BoxError> {
        let backup_dir = unique_path(self.backups_dir.join(skill_name).join(timestamp_for_path()));
        tokio::fs::create_dir_all(&backup_dir).await?;
        tokio::fs::copy(skill_md, backup_dir.join("SKILL.md")).await?;
        Ok(())
    }

    fn available_clone_name(&self, base_name: &str) -> Result<String, BoxError> {
        let mut candidate = format!("{base_name}-copy");
        let mut suffix = 2usize;
        while self.personal_dir.join(&candidate).exists() {
            candidate = format!("{base_name}-copy-{suffix}");
            suffix += 1;
        }
        // A long name plus the suffix can pass the 64-character limit.
        normalize_skill_name(candidate)
    }

    /// The registry these skills are dispatched through.
    ///
    /// `SkillManager` owns loading and materialization, including which skills
    /// are callable at all — only those declaring `execution: subagent` are.
    /// This library layers the source, manifest, and diagnostic policy on top
    /// of it rather than keeping a second registry that could disagree.
    pub fn subagent_set(&self) -> &dyn SubAgentSet {
        self.skill_manager.as_ref()
    }

    /// The callable of the skill a `/skill` command names, by skill name or
    /// callable name; only skills declaring `execution: subagent` have one.
    pub fn skill_subagent(&self, name: &str) -> Option<SubAgent> {
        self.subagent_set().get_lowercase(&skill_agent_name(name))
    }

    /// Whether a `/skill` command naming `name` has an active skill to route to.
    pub fn has_active_skill(&self, name: &str) -> bool {
        let agent_name = skill_agent_name(name);
        self.state
            .read()
            .records
            .iter()
            .any(|record| record.managed.active && record.managed.agent_name == agent_name)
    }
}

fn skill_agent_name(name: &str) -> String {
    normalise_skill_agent_name(name.strip_prefix("skill_").unwrap_or(name))
}

impl Tool<BaseCtx> for SkillLibrary {
    type Args = SkillsApiArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Manage local Anda skills: inspect sources, browse complete skill directories, create Personal skills, clone read-only skills, enable or disable skills, validate SKILL.md content, and reload the runtime skill library."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: <Self as Tool<BaseCtx>>::name(self),
            description: <Self as Tool<BaseCtx>>::description(self),
            parameters: skills_api_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let result = match args {
            SkillsApiArgs::ListSkillSources {} => json!(self.skill_sources()),
            SkillsApiArgs::ListSkills { include_inactive } => {
                json!(self.list_managed_skills(include_inactive.unwrap_or(false)))
            }
            SkillsApiArgs::GetSkill { id } => json!(self.get_skill_detail(&id).await?),
            SkillsApiArgs::GetSkillFile { id, path } => {
                json!(self.get_skill_file(&id, &path).await?)
            }
            SkillsApiArgs::CreateSkill {
                name,
                description,
                content,
            } => json!(self.create_skill(name, description, content).await?),
            SkillsApiArgs::UpdateSkill {
                id,
                content,
                expected_version,
            } => json!(self.update_skill(id, content, expected_version).await?),
            SkillsApiArgs::CloneSkill { id, new_name } => {
                json!(self.clone_skill(id, new_name).await?)
            }
            SkillsApiArgs::SetSkillEnabled { id, enabled } => {
                json!(self.set_skill_enabled(id, enabled).await?)
            }
            SkillsApiArgs::DeletePersonalSkill { id } => self.delete_personal_skill(id).await?,
            SkillsApiArgs::ValidateSkill { content } => json!(self.validate_skill(content)),
            SkillsApiArgs::ReloadSkills {} => json!(self.reload().await?),
        };

        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

/// Books each read of an inline skill as one use of that skill.
///
/// A delegated skill is a callable, so the runner already books its calls under
/// `sa_skill_<name>`. An inline skill is only ever read — through
/// `skills_manager`, or page by page through `skills_read` when it is too large
/// for one response — and those reads land on the reader tools' own names, so
/// every inline skill used to look unused. The use is booked under
/// `skill_<name>`, the second key [`managed_skill_usage`] sums.
pub struct SkillUsageHook;

#[async_trait]
impl ToolHook<SkillArgs, SkillContentOutput> for SkillUsageHook {
    async fn after_tool_call(
        &self,
        _ctx: &BaseCtx,
        mut output: ToolOutput<SkillContentOutput>,
    ) -> Result<ToolOutput<SkillContentOutput>, BoxError> {
        if output.output.execution == SkillExecution::Inline {
            book_skill_use(&mut output.tools_usage, &output.output.name);
        }
        Ok(output)
    }
}

#[async_trait]
impl ToolHook<SkillsReadArgs, SkillsReadOutput> for SkillUsageHook {
    async fn after_tool_call(
        &self,
        _ctx: &BaseCtx,
        mut output: ToolOutput<SkillsReadOutput>,
    ) -> Result<ToolOutput<SkillsReadOutput>, BoxError> {
        // A paged SKILL.md counts once, on its last page.
        let read = &output.output;
        if read.execution == SkillExecution::Inline
            && read.resource == "SKILL.md"
            && read.next_cursor.is_none()
        {
            let name = read.name.clone();
            book_skill_use(&mut output.tools_usage, &name);
        }
        Ok(output)
    }
}

fn book_skill_use(tools_usage: &mut HashMap<String, Usage>, name: &str) {
    tools_usage
        .entry(normalise_skill_agent_name(name))
        .or_default()
        .requests += 1;
}

fn skills_api_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": [
                    "ListSkillSources",
                    "ListSkills",
                    "GetSkill",
                    "GetSkillFile",
                    "CreateSkill",
                    "UpdateSkill",
                    "CloneSkill",
                    "SetSkillEnabled",
                    "DeletePersonalSkill",
                    "ValidateSkill",
                    "ReloadSkills"
                ],
                "description": "Skill management operation to perform."
            },
            "include_inactive": {
                "type": ["boolean", "null"],
                "description": "For ListSkills, include disabled, shadowed, and invalid skills."
            },
            "id": {
                "type": ["string", "null"],
                "description": "Managed skill id such as personal:learn, bundled:pdf, or shared:docx."
            },
            "path": {
                "type": ["string", "null"],
                "description": "Skill-relative file path for GetSkillFile, such as SKILL.md or references/api.md."
            },
            "name": {
                "type": ["string", "null"],
                "description": "Kebab-case skill name for CreateSkill or CloneSkill."
            },
            "new_name": {
                "type": ["string", "null"],
                "description": "Optional new Personal skill name for CloneSkill."
            },
            "description": {
                "type": ["string", "null"],
                "description": "Short skill description for CreateSkill when content has no frontmatter."
            },
            "content": {
                "type": ["string", "null"],
                "description": "Full SKILL.md content, or body text for CreateSkill."
            },
            "expected_version": {
                "type": ["string", "null"],
                "description": "Current version hash from GetSkill, used to prevent overwriting a changed Personal skill."
            },
            "enabled": {
                "type": ["boolean", "null"],
                "description": "Whether the skill should be enabled."
            }
        },
        "required": [
            "type",
            "include_inactive",
            "id",
            "path",
            "name",
            "new_name",
            "description",
            "content",
            "expected_version",
            "enabled"
        ],
        "additionalProperties": false
    })
}

/// Reads one `SKILL.md` into a record. Never fails: whatever goes wrong becomes
/// a diagnostic, so a broken skill still shows up and can be fixed.
fn scan_skill_file(
    source: &SkillSource,
    path: &Path,
    known_tools: &BTreeSet<String>,
) -> SkillRecord {
    let base_dir = path.parent().unwrap_or(&source.path).to_path_buf();
    let mut diagnostics = Vec::new();

    let meta = std::fs::symlink_metadata(path);
    let (size, updated_at) = match &meta {
        Ok(meta) => (Some(meta.len()), modified_at_ms(meta)),
        Err(_) => (None, None),
    };
    let bytes = match &meta {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => Err(
            SkillDiagnostic::error("not_regular_file", "SKILL.md must be a regular file"),
        ),
        Ok(meta) if meta.len() > MAX_SKILL_FILE_BYTES => Err(SkillDiagnostic::error(
            "file_too_large",
            format!("SKILL.md must be at most {MAX_SKILL_FILE_BYTES} bytes"),
        )),
        Ok(_) => std::fs::read(path).map_err(|err| {
            SkillDiagnostic::error("read_failed", format!("failed to read SKILL.md: {err}"))
        }),
        Err(err) => Err(SkillDiagnostic::error(
            "metadata_failed",
            format!("failed to inspect SKILL.md: {err}"),
        )),
    };
    let version = content_version(bytes.as_deref().unwrap_or_default());
    let content = match bytes.as_deref().map(anda_core::text_from_bytes) {
        Ok(Some(text)) => text.into_owned(),
        Ok(None) => {
            diagnostics.push(SkillDiagnostic::error(
                "decode_failed",
                "SKILL.md must be readable as UTF-8 or the platform text encoding",
            ));
            String::new()
        }
        Err(diagnostic) => {
            diagnostics.push(diagnostic.clone());
            String::new()
        }
    };

    let parsed = if diagnostics.is_empty() {
        parse_skill_md(base_dir.clone(), &content)
            .inspect_err(|err| {
                diagnostics.push(SkillDiagnostic::error(
                    "parse_failed",
                    format!("invalid SKILL.md: {err}"),
                ))
            })
            .ok()
    } else {
        None
    };
    if let Some(skill) = &parsed {
        if source.kind == SkillSourceKind::Personal
            && base_dir.file_name() != Some(OsStr::new(&skill.frontmatter.name))
        {
            diagnostics.push(SkillDiagnostic::error(
                "name_directory_mismatch",
                "personal skill frontmatter name must match its directory",
            ));
        }
        diagnostics.extend(frontmatter_diagnostics(skill, known_tools));
    }

    let (name, agent_name, description, compatibility, execution, allowed_tools, metadata) =
        match parsed {
            Some(skill) => (
                skill.frontmatter.name,
                skill.agent_name,
                skill.frontmatter.description,
                skill.frontmatter.compatibility,
                skill.execution,
                skill.tools,
                json!(skill.frontmatter.metadata),
            ),
            None => {
                let name = fallback_skill_name(&base_dir);
                let agent_name = validate_skill_name(&name)
                    .map(|_| normalise_skill_agent_name(&name))
                    .unwrap_or_else(|_| format!("skill_invalid_{}", short_hash(path)));
                (
                    name,
                    agent_name,
                    String::new(),
                    None,
                    SkillExecution::default(),
                    Vec::new(),
                    json!({}),
                )
            }
        };
    SkillRecord {
        managed: ManagedSkill {
            id: skill_id(source.kind, &name),
            source: source.kind,
            source_label: source.kind.label().to_string(),
            priority: source.priority,
            name,
            agent_name,
            description,
            compatibility,
            execution,
            allowed_tools,
            metadata,
            path: path.display().to_string(),
            directory: base_dir.display().to_string(),
            editable: source.editable(),
            active: false,
            disabled: false,
            shadowed_by: None,
            diagnostics,
            updated_at,
            size,
            usage: None,
            version,
        },
        content,
        base_dir,
    }
}

fn attach_usage(mut managed: ManagedSkill, tools_usage: &HashMap<String, Usage>) -> ManagedSkill {
    managed.usage = managed_skill_usage(&managed, tools_usage);
    managed
}

fn managed_skill_usage(
    managed: &ManagedSkill,
    tools_usage: &HashMap<String, Usage>,
) -> Option<SkillUsageSummary> {
    let agent_name = managed.agent_name.to_ascii_lowercase();
    let callable = format!("sa_{agent_name}");
    let mut usage = Usage::default();
    let mut found = false;

    for key in [&callable, &agent_name] {
        if let Some(entry) = tools_usage.get(key) {
            usage.accumulate(entry);
            found = true;
        }
    }

    if !found
        || (usage.requests == 0
            && usage.input_tokens == 0
            && usage.output_tokens == 0
            && usage.cached_tokens == 0)
    {
        return None;
    }

    Some(SkillUsageSummary {
        callable,
        requests: usage.requests,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_tokens: usage.cached_tokens,
        total_tokens: usage.input_tokens.saturating_add(usage.output_tokens),
    })
}

/// Decides which copy of each name is active. `records` must be sorted by
/// priority and path, as `scan_records` leaves them.
fn apply_effective_state(records: &mut [SkillRecord], manifest: &SkillManifest) {
    let mut winners: BTreeMap<String, usize> = BTreeMap::new();
    for index in 0..records.len() {
        let managed = &mut records[index].managed;
        managed.disabled = manifest.disabled.contains_key(&managed.id);
        if managed.disabled || managed.has_error() {
            continue;
        }
        let Some(&winner) = winners.get(&managed.agent_name) else {
            managed.active = true;
            winners.insert(managed.agent_name.clone(), index);
            continue;
        };

        let winner_id = records[winner].managed.id.clone();
        if records[winner].managed.priority != records[index].managed.priority {
            let managed = &mut records[index].managed;
            managed.diagnostics.push(SkillDiagnostic::warning(
                "shadowed",
                format!("Shadowed by higher-priority skill {winner_id}."),
            ));
            managed.shadowed_by = Some(winner_id);
            continue;
        }

        // The registry will not choose between two copies in one source: the
        // name resolves to neither until one is disabled or removed.
        let index_id = records[index].managed.id.clone();
        records[index]
            .managed
            .diagnostics
            .push(conflict(&winner_id));
        let winner = &mut records[winner].managed;
        if winner.active {
            winner.active = false;
            winner.diagnostics.push(conflict(&index_id));
        }
    }
}

fn conflict(other_id: &str) -> SkillDiagnostic {
    SkillDiagnostic::warning(
        "conflict",
        format!(
            "{other_id} in the same source has the same name; neither is used until one is disabled or removed."
        ),
    )
}

fn skill_id(source: SkillSourceKind, name: &str) -> String {
    format!("{}:{name}", source.as_str())
}

fn personal_skill_id(name: &str) -> String {
    skill_id(SkillSourceKind::Personal, name)
}

fn normalize_skill_name(name: String) -> Result<String, BoxError> {
    let name = name.trim().to_ascii_lowercase();
    validate_skill_name(&name)?;
    Ok(name)
}

fn fallback_skill_name(base_dir: &Path) -> String {
    base_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown-skill")
        .to_ascii_lowercase()
}

fn normalize_new_skill_content(name: &str, description: &str, content: &str) -> String {
    let content = content.trim();
    if content.starts_with("---") {
        return format!("{content}\n");
    }
    format!(
        "---\nname: {name}\ndescription: {}\n---\n\n{}\n",
        yaml_double_quoted(description.trim()),
        content
    )
}

fn yaml_double_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn validate_skill_content(expected_name: Option<&str>, content: &str) -> SkillValidationResult {
    let mut diagnostics = Vec::new();
    let parsed = match parse_skill_md(PathBuf::from("preview"), content) {
        Ok(skill) => skill,
        Err(err) => {
            diagnostics.push(SkillDiagnostic::error(
                "parse_failed",
                format!("invalid SKILL.md: {err}"),
            ));
            return SkillValidationResult {
                valid: false,
                diagnostics,
                name: None,
                agent_name: None,
            };
        }
    };
    if let Some(expected_name) = expected_name
        && parsed.frontmatter.name != expected_name
    {
        diagnostics.push(SkillDiagnostic::error(
            "name_changed",
            format!("SKILL.md name must remain {expected_name}"),
        ));
    }
    SkillValidationResult {
        valid: diagnostics
            .iter()
            .all(|d| d.severity != SkillDiagnosticSeverity::Error),
        diagnostics,
        name: Some(parsed.frontmatter.name),
        agent_name: Some(parsed.agent_name),
    }
}

/// Flags frontmatter that does nothing for how the skill runs.
///
/// An inline skill is read into the calling agent's own context and keeps that
/// agent's tools: its `allowed-tools` grant nothing, so they are not checked
/// (they are common frontmatter in skills written for other agents), and its
/// `resource-tags` select nothing, which looks like a restriction that is
/// silently not applied. A delegated skill runs with exactly its declared
/// tools, so each must be one this host provides.
///
/// MCP tools come from servers that connect after skills load, so they
/// cannot be checked here. Both the Anda name (`mcp_<server>_<tool>`) and the
/// `mcp__<server>__<tool>` form other agents use reach a tool. A wildcard
/// does not: a delegated skill may call only the names it lists, so a
/// wildcard, or a name too long or odd to be a tool name, is flagged.
fn frontmatter_diagnostics(skill: &Skill, known_tools: &BTreeSet<String>) -> Vec<SkillDiagnostic> {
    if !skill.is_subagent() {
        if !skill.declares_resource_tags() {
            return Vec::new();
        }
        return vec![SkillDiagnostic::warning(
            "resource_tags_ignored",
            "resource-tags only applies to skills that declare execution: subagent.",
        )];
    }
    skill
        .tools
        .iter()
        .filter_map(|tool| {
            if tool.starts_with("mcp__") {
                if validate_function_name(&tool.to_ascii_lowercase()).is_ok() {
                    return None;
                }
                let hint = match anda_mcp_tool_name(tool) {
                    Some(name) => format!("use the Anda name, such as {name}"),
                    None => "list each tool, as mcp__<server>__<tool>".to_string(),
                };
                return Some(SkillDiagnostic::warning(
                    "unsupported_mcp_tool_name",
                    format!("allowed-tools names {tool}, which cannot be called; {hint}."),
                ));
            }
            if known_tools.contains(tool.as_str())
                || tool.starts_with("mcp_")
                || tool.starts_with("plugin__")
            {
                return None;
            }
            Some(SkillDiagnostic::warning(
                "unknown_tool",
                format!("allowed-tools includes unknown tool {tool}."),
            ))
        })
        .collect()
}

/// The Anda name of an `mcp__<server>__<tool>` entry, normalized the way the
/// engine names MCP tools (a name that collides or runs long gets a hash
/// suffix there, which cannot be predicted here). `None` for a wildcard.
fn anda_mcp_tool_name(name: &str) -> Option<String> {
    let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
    if tool.contains('*') {
        return None;
    }
    let part = |part: &str| {
        let normalized = part
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        if normalized.is_empty() {
            "x".to_string()
        } else {
            normalized
        }
    };
    Some(format!("mcp_{}_{}", part(server), part(tool)))
}

fn diagnostic_summary(diagnostics: &[SkillDiagnostic]) -> String {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == SkillDiagnosticSeverity::Error)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

async fn ensure_path_is_direct_child(root: &Path, dir: &Path) -> Result<(), BoxError> {
    tokio::fs::create_dir_all(root).await?;
    let root = tokio::fs::canonicalize(root).await?;
    let parent = dir
        .parent()
        .ok_or("personal skill path must have a parent directory")?;
    let parent = if parent.exists() {
        tokio::fs::canonicalize(parent).await?
    } else {
        root.clone()
    };
    if parent != root {
        return Err("personal skill path must be directly under the Personal skills root".into());
    }
    Ok(())
}

async fn atomic_write_text(path: &Path, content: &str) -> Result<(), BoxError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Ok(meta) = tokio::fs::symlink_metadata(path).await
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err(format!("refusing to overwrite non-regular file {}", path.display()).into());
    }
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), unix_ms()));
    tokio::fs::write(&tmp, content).await?;
    // `rename` replaces an existing file on Windows too, so the target is never
    // briefly missing.
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|err| format!("failed to replace {}: {err}", path.display()).into())
}

/// Hidden directories (`.git`, `.venv`) are skipped everywhere a skill
/// directory is walked, as skill discovery skips them.
fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}

fn copy_dir_regular_files(src: &Path, dst: &Path) -> Result<(), BoxError> {
    if dst.exists() {
        return Err(format!("destination already exists: {}", dst.display()).into());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() && !is_hidden(&entry) {
            copy_dir_regular_files(&from, &to)?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Lists a skill directory for browsing. Entries that cannot be read are left
/// out rather than failing the whole listing.
fn list_skill_files(base_dir: &Path) -> Vec<SkillFileEntry> {
    let mut files = Vec::new();
    collect_skill_files(base_dir, base_dir, &mut files);
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    files
}

fn collect_skill_files(base_dir: &Path, dir: &Path, files: &mut Vec<SkillFileEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let (Ok(file_type), Ok(meta)) = (entry.file_type(), entry.metadata()) else {
            continue;
        };
        let path = entry.path();
        let Some(relative_path) = skill_relative_display_path(base_dir, &path) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_dir() && !is_hidden(&entry) {
            files.push(SkillFileEntry {
                path: relative_path,
                name,
                kind: SkillFileKind::Directory,
                size: None,
                updated_at: modified_at_ms(&meta),
            });
            collect_skill_files(base_dir, &path, files);
        } else if file_type.is_file() {
            files.push(SkillFileEntry {
                path: relative_path,
                name,
                kind: SkillFileKind::File,
                size: Some(meta.len()),
                updated_at: modified_at_ms(&meta),
            });
        }
    }
}

fn read_skill_file(id: &str, base_dir: &Path, path: &str) -> Result<SkillFileContent, BoxError> {
    let relative_path = normalize_skill_relative_path(path)?;
    let file_path = base_dir.join(&relative_path);
    let meta = std::fs::symlink_metadata(&file_path)
        .map_err(|err| format!("failed to inspect skill file {path}: {err}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("skill file path must point to a regular file".into());
    }
    let base_dir = std::fs::canonicalize(base_dir)?;
    let canonical_file = std::fs::canonicalize(&file_path)?;
    if !canonical_file.starts_with(&base_dir) {
        return Err("skill file path cannot escape the skill directory".into());
    }

    let truncated = meta.len() > MAX_SKILL_VIEW_FILE_BYTES;
    let mut bytes = Vec::new();
    if truncated {
        let file = std::fs::File::open(&file_path)?;
        let mut limited = file.take(MAX_SKILL_VIEW_FILE_BYTES);
        limited.read_to_end(&mut bytes)?;
        // The cut can land inside a multi-byte character; drop the partial
        // tail so the rest still decodes.
        if let Err(err) = std::str::from_utf8(&bytes)
            && err.error_len().is_none()
        {
            bytes.truncate(err.valid_up_to());
        }
    } else {
        bytes = std::fs::read(&file_path)?;
    }
    let Some(text) = anda_core::text_from_bytes(&bytes) else {
        return Err("skill file is not readable as UTF-8 or the platform text encoding".into());
    };
    Ok(SkillFileContent {
        id: id.to_string(),
        path: skill_relative_display_path(&base_dir, &canonical_file)
            .unwrap_or_else(|| relative_path.display().to_string()),
        content: text.into_owned(),
        size: meta.len(),
        updated_at: modified_at_ms(&meta),
        truncated,
    })
}

fn normalize_skill_relative_path(path: &str) -> Result<PathBuf, BoxError> {
    let normalized = path.trim().replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() {
        return Err("skill file path must be relative".into());
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => return Err("skill file path cannot escape the skill directory".into()),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("skill file path is required".into());
    }
    Ok(out)
}

fn skill_relative_display_path(base_dir: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(base_dir).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn modified_at_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn timestamp_for_path() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    for index in 2..1000 {
        let candidate = PathBuf::from(format!("{}-{index}", path.display()));
        if !candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(format!("{}-{}", path.display(), unix_ms()))
}

fn content_version(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

fn short_hash(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash = hex_lower(&hasher.finalize());
    hash[..12].to_string()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::{Tool, Usage};
    use anda_engine::{
        engine::EngineBuilder,
        extension::skill::{SkillToolHook, SkillsReadHook, SkillsReadTool, find_skill_files},
    };
    use std::{collections::HashMap, fs};
    use tempfile::tempdir;

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
    }

    fn write_skill(root: &Path, name: &str, description: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), skill_md(name, description)).unwrap();
    }

    /// Writes a skill with extra frontmatter lines after `description`.
    fn write_skill_with_frontmatter(root: &Path, name: &str, extra: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} skill\n{extra}---\n\n# {name}\n"),
        )
        .unwrap();
    }

    fn library(home: &Path) -> SkillLibrary {
        SkillLibrary::for_test(home.to_path_buf()).as_ref().clone()
    }

    fn find<'a>(skills: &'a [ManagedSkill], id: &str) -> &'a ManagedSkill {
        skills
            .iter()
            .find(|skill| skill.id == id)
            .unwrap_or_else(|| panic!("{id} is not listed"))
    }

    fn has_code(skill: &ManagedSkill, code: &str) -> bool {
        skill.diagnostics.iter().any(|d| d.code == code)
    }

    /// Reads a skill through the registry's own reader tool, as the model does.
    async fn read_through_registry(lib: &SkillLibrary, name: &str) -> Result<String, BoxError> {
        let output = Tool::call_raw(
            lib.skill_manager().as_ref(),
            EngineBuilder::new().mock_ctx().base,
            json!({ "name": name }),
            vec![],
        )
        .await?;
        Ok(output.output["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    #[tokio::test]
    async fn scan_marks_shadowed_duplicates_and_active_winner() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "personal");
        write_skill(&temp.path().join("bundled-skills"), "learn", "bundled");
        write_skill(&temp.path().join("shared-skills"), "docx", "shared");

        lib.reload().await.unwrap();
        let skills = lib.list_managed_skills(true);
        let personal = find(&skills, "personal:learn");
        assert!(personal.active);
        assert_eq!(personal.source, SkillSourceKind::Personal);
        let bundled = find(&skills, "bundled:learn");
        assert!(!bundled.active);
        assert_eq!(bundled.shadowed_by.as_deref(), Some("personal:learn"));
        assert!(find(&skills, "shared:docx").active);
    }

    #[tokio::test]
    async fn list_and_detail_include_skill_usage_summary() {
        let temp = tempdir().unwrap();
        let usage = HashMap::from([
            (
                "sa_skill_learn".to_string(),
                Usage {
                    input_tokens: 10,
                    output_tokens: 7,
                    cached_tokens: 3,
                    requests: 2,
                },
            ),
            (
                "skill_learn".to_string(),
                Usage {
                    input_tokens: 5,
                    output_tokens: 1,
                    cached_tokens: 0,
                    requests: 1,
                },
            ),
        ]);
        let lib = library(temp.path()).with_tools_usage_reader(move || usage.clone());
        write_skill(&temp.path().join("skills"), "learn", "personal");

        lib.reload().await.unwrap();
        let skill = lib
            .list_managed_skills(true)
            .into_iter()
            .find(|skill| skill.id == "personal:learn")
            .unwrap();
        let summary = skill.usage.unwrap();
        assert_eq!(summary.callable, "sa_skill_learn");
        assert_eq!(summary.requests, 3);
        assert_eq!(summary.input_tokens, 15);
        assert_eq!(summary.output_tokens, 8);
        assert_eq!(summary.cached_tokens, 3);
        assert_eq!(summary.total_tokens, 23);

        let detail = lib.get_skill_detail("personal:learn").await.unwrap();
        assert_eq!(detail.skill.usage.unwrap().requests, 3);
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].path, "SKILL.md");
    }

    #[tokio::test]
    async fn inline_skill_reads_are_booked_as_skill_usage() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        write_skill(&personal, "learn", "inline");
        write_skill_with_frontmatter(&personal, "worker", "execution: subagent\n");
        lib.reload().await.unwrap();

        let ctx = EngineBuilder::new().mock_ctx().base;
        ctx.set_state(SkillToolHook::new(Arc::new(SkillUsageHook)));
        ctx.set_state(SkillsReadHook::new(Arc::new(SkillUsageHook)));
        let mgr = lib.skill_manager();

        let read = Tool::call_raw(
            mgr.as_ref(),
            ctx.clone(),
            json!({ "name": "learn" }),
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(read.tools_usage["skill_learn"].requests, 1);

        let paged = Tool::call_raw(
            &SkillsReadTool::new(mgr.clone()),
            ctx.clone(),
            json!({ "skill": "learn", "resource": null, "cursor": null }),
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(paged.tools_usage["skill_learn"].requests, 1);

        // A delegated skill is booked by its callable, not by being read.
        let delegated = Tool::call_raw(mgr.as_ref(), ctx, json!({ "name": "worker" }), vec![])
            .await
            .unwrap();
        assert!(delegated.tools_usage.is_empty());
    }

    #[tokio::test]
    async fn disabling_higher_priority_skill_promotes_next_copy() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "personal");
        write_skill(&temp.path().join("bundled-skills"), "learn", "bundled");

        lib.reload().await.unwrap();
        lib.set_skill_enabled("personal:learn".to_string(), false)
            .await
            .unwrap();

        let skills = lib.list_managed_skills(true);
        assert!(!find(&skills, "personal:learn").active);
        assert!(find(&skills, "bundled:learn").active);
        assert!(
            lib.prompt_skills()
                .iter()
                .any(|skill| skill.name == "learn")
        );
        // Both copies are inline, so neither is callable.
        assert_eq!(lib.subagent_set().definitions(None).len(), 0);
    }

    #[tokio::test]
    async fn only_skills_declaring_subagent_execution_become_callables() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        write_skill(&personal, "learn", "inline by default");
        write_skill_with_frontmatter(&personal, "worker", "execution: subagent\n");

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let learn = skills.iter().find(|s| s.name == "learn").unwrap();
        let worker = skills.iter().find(|s| s.name == "worker").unwrap();
        assert_eq!(learn.execution, SkillExecution::Inline);
        assert_eq!(worker.execution, SkillExecution::Subagent);
        assert!(learn.active && worker.active);

        // A `/skill` command finds both, by skill name or callable name, but
        // only the subagent skill has a callable.
        assert!(lib.has_active_skill("learn") && lib.has_active_skill("Worker"));
        assert!(lib.has_active_skill("skill_worker"));
        assert!(!lib.has_active_skill("HOME"));
        assert!(lib.skill_subagent("learn").is_none());
        assert_eq!(lib.skill_subagent("worker").unwrap().name, "skill_worker");

        // The inline skill is loaded and readable, but never dispatchable.
        assert!(!lib.subagent_set().contains_lowercase("skill_learn"));
        assert!(lib.subagent_set().get_lowercase("skill_learn").is_none());
        assert!(lib.subagent_set().contains_lowercase("skill_worker"));
        assert_eq!(lib.subagent_set().definitions(None).len(), 1);
        assert_eq!(
            lib.subagent_set()
                .definitions(Some(&["skill_learn".to_string()]))
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn declared_allowed_tools_are_an_upper_bound() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        write_skill_with_frontmatter(
            &personal,
            "narrow",
            "execution: subagent\nallowed-tools: shell\n",
        );
        write_skill_with_frontmatter(&personal, "wide", "execution: subagent\n");

        lib.reload().await.unwrap();

        // A skill that declared its own list gets exactly that list; the
        // configured defaults must not widen it back out.
        let narrow = lib.subagent_set().get_lowercase("skill_narrow").unwrap();
        assert_eq!(narrow.tools, vec!["shell".to_string()]);
        // A skill that declared nothing inherits the defaults the library
        // handed to the registry at construction.
        let wide = lib.subagent_set().get_lowercase("skill_wide").unwrap();
        assert!(wide.tools.len() > 1 && wide.tools.contains(&"shell".to_string()));
    }

    /// Writes a subagent skill whose body identifies which copy it is.
    fn write_subagent_skill(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {name} skill\nexecution: subagent\n---\n\n{body}\n"
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn disabling_a_skill_hides_it_from_the_registry_and_promotes_the_next_copy() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_subagent_skill(&temp.path().join("skills"), "worker", "Personal body.");
        write_subagent_skill(
            &temp.path().join("bundled-skills"),
            "worker",
            "Bundled body.",
        );
        lib.reload().await.unwrap();

        assert!(
            lib.subagent_set()
                .get_lowercase("skill_worker")
                .unwrap()
                .instructions
                .contains("Personal body.")
        );

        lib.set_skill_enabled("personal:worker".to_string(), false)
            .await
            .unwrap();

        // The bundled copy takes over rather than the name disappearing, and the
        // registry — not just the Dashboard listing — reflects it.
        assert!(
            lib.subagent_set()
                .get_lowercase("skill_worker")
                .unwrap()
                .instructions
                .contains("Bundled body.")
        );
        assert!(
            read_through_registry(&lib, "worker")
                .await
                .unwrap()
                .contains("Bundled body.")
        );

        // With both copies disabled the skill is gone from dispatch and from the
        // reader tool, instead of `skills_manager` handing back a callable name
        // that nothing will resolve.
        lib.set_skill_enabled("bundled:worker".to_string(), false)
            .await
            .unwrap();
        assert!(!lib.subagent_set().contains_lowercase("skill_worker"));
        let err = read_through_registry(&lib, "worker").await.unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn reload_keeps_the_live_session_registry_of_surviving_skills() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill_with_frontmatter(
            &temp.path().join("skills"),
            "worker",
            "execution: subagent\n",
        );

        lib.reload().await.unwrap();
        let before = lib.subagent_set().get_lowercase("skill_worker").unwrap();
        lib.reload().await.unwrap();
        let after = lib.subagent_set().get_lowercase("skill_worker").unwrap();

        // A running session is registered in one instance's registry; rebuilding
        // it on reload would strand that session.
        assert!(Arc::ptr_eq(&before.subsessions, &after.subsessions));
    }

    #[tokio::test]
    async fn resource_tags_on_an_inline_skill_are_flagged() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        write_skill_with_frontmatter(&personal, "tagged", "resource-tags: image\n");
        write_skill_with_frontmatter(
            &personal,
            "delegated",
            "execution: subagent\nresource-tags: image\n",
        );

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let tagged = skills.iter().find(|s| s.name == "tagged").unwrap();
        assert!(has_code(tagged, "resource_tags_ignored"));
        let delegated = skills.iter().find(|s| s.name == "delegated").unwrap();
        assert!(delegated.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn unknown_tools_are_only_checked_for_delegated_skills() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        // Written for another agent: the inline skill grants nothing, so its
        // tool names do not matter here.
        write_skill_with_frontmatter(&personal, "borrowed", "allowed-tools: Bash Read\n");
        write_skill_with_frontmatter(
            &personal,
            "delegated",
            "execution: subagent\nallowed-tools: Bash shell mcp_github_create_issue\n",
        );

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        assert!(find(&skills, "personal:borrowed").diagnostics.is_empty());
        let delegated = find(&skills, "personal:delegated");
        // MCP tools in Anda's own form connect later, so they pass unchecked.
        let unknown: Vec<&str> = delegated
            .diagnostics
            .iter()
            .filter(|d| d.code == "unknown_tool")
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(unknown, ["allowed-tools includes unknown tool Bash."]);
    }

    #[tokio::test]
    async fn mcp_tools_in_another_agents_form_are_flagged_only_when_uncallable() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let personal = temp.path().join("skills");
        write_skill_with_frontmatter(
            &personal,
            "delegated",
            "execution: subagent\nallowed-tools: mcp__GitHub__create-issue mcp__docs__search.v2 mcp__github__*\n",
        );

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let delegated = find(&skills, "personal:delegated");
        let flagged: Vec<&str> = delegated
            .diagnostics
            .iter()
            .filter(|d| d.code == "unsupported_mcp_tool_name")
            .map(|d| d.message.as_str())
            .collect();
        // The gate resolves mcp__GitHub__create-issue; the others cannot be
        // called by a delegated skill.
        assert_eq!(flagged.len(), 2, "{flagged:?}");
        assert!(
            flagged[0].contains("such as mcp_docs_search_v2"),
            "{}",
            flagged[0]
        );
        assert!(
            flagged[1].contains("mcp__<server>__<tool>"),
            "{}",
            flagged[1]
        );
        assert!(
            !delegated
                .diagnostics
                .iter()
                .any(|d| d.code == "unknown_tool")
        );
    }

    #[tokio::test]
    async fn a_partial_scan_keeps_the_skills_it_reached() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let shared = temp.path().join("shared-skills");
        write_skill(&shared, "docx", "shared");
        // Deeper than discovery walks, like a skill shipping `node_modules`.
        fs::create_dir_all(shared.join("tool/node_modules/@scope/pkg/dist/esm/internal")).unwrap();
        assert!(find_skill_files(&shared).await.is_err());

        lib.reload().await.unwrap();

        assert!(find(&lib.list_managed_skills(true), "shared:docx").active);
        let sources = lib.skill_sources();
        let shared_source = sources
            .iter()
            .find(|source| source.source == SkillSourceKind::Shared)
            .unwrap();
        assert!(
            shared_source.diagnostics.iter().any(|d| d.code == "limit"),
            "{:?}",
            shared_source.diagnostics
        );

        // Listed means it can be switched off, everywhere.
        lib.set_skill_enabled("shared:docx".to_string(), false)
            .await
            .unwrap();
        assert!(read_through_registry(&lib, "docx").await.is_err());
        assert!(lib.prompt_skills().is_empty());
    }

    #[tokio::test]
    async fn same_name_skills_in_one_source_get_their_own_ids_and_conflict() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let shared = temp.path().join("shared-skills");
        write_skill(&shared, "pdf", "top level");
        write_skill(&shared.join("vendor"), "pdf", "vendored copy");

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let ids: Vec<&str> = skills
            .iter()
            .filter(|skill| skill.name == "pdf")
            .map(|skill| skill.id.as_str())
            .collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "shared:pdf");
        assert!(ids[1].starts_with("shared:pdf@"), "{ids:?}");
        // The registry refuses to pick one, so neither is reported as in use.
        for id in &ids {
            let skill = find(&skills, id);
            assert!(!skill.active && has_code(skill, "conflict"), "{skill:?}");
        }
        assert!(lib.prompt_skills().is_empty());
        assert!(read_through_registry(&lib, "pdf").await.is_err());

        let vendored = ids[1].to_string();
        lib.set_skill_enabled(vendored.clone(), false)
            .await
            .unwrap();
        let skills = lib.list_managed_skills(true);
        assert!(find(&skills, "shared:pdf").active);
        assert!(find(&skills, &vendored).disabled);
        assert!(read_through_registry(&lib, "pdf").await.is_ok());
    }

    #[tokio::test]
    async fn legacy_disabled_manifest_entry_still_disables_personal_shadow() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "personal");
        write_skill(&temp.path().join("bundled-skills"), "learn", "bundled");
        fs::write(
            temp.path().join(MANIFEST_FILE_NAME),
            serde_json::to_string(&json!({
                "version": 1,
                "disabled": {
                    "legacy:learn": {
                        "disabled_at": 1,
                        "reason": "disabled before source normalization"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let personal = find(&skills, "personal:learn");
        assert!(personal.disabled);
        assert!(!personal.active);
        assert!(find(&skills, "bundled:learn").active);

        lib.set_skill_enabled("personal:learn".to_string(), true)
            .await
            .unwrap();

        let manifest: SkillManifest = serde_json::from_str(
            &fs::read_to_string(temp.path().join(MANIFEST_FILE_NAME)).unwrap(),
        )
        .unwrap();
        assert!(manifest.disabled.is_empty());
        let skills = lib.list_managed_skills(true);
        let personal = find(&skills, "personal:learn");
        assert!(personal.active);
        assert!(!personal.disabled);
    }

    #[tokio::test]
    async fn a_broken_manifest_is_reported_and_left_untouched() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "personal");
        let manifest_path = temp.path().join(MANIFEST_FILE_NAME);

        // A plain reload has nothing to record.
        lib.reload().await.unwrap();
        assert!(!manifest_path.exists());

        let broken = "{ \"version\": 1, \"disabled\": { \"personal:learn\": {}, } }";
        fs::write(&manifest_path, broken).unwrap();
        let err = lib.reload().await.unwrap_err();
        assert!(err.to_string().contains("is invalid"), "{err}");
        let err = lib
            .set_skill_enabled("personal:learn".to_string(), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("is invalid"), "{err}");
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), broken);
    }

    #[tokio::test]
    async fn create_update_and_delete_personal_skill_use_safe_storage() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        lib.reload().await.unwrap();

        let created = lib
            .create_skill(
                "my-skill".to_string(),
                "My workflow: \"daily\"\nSecond line".to_string(),
                "# My Skill\n".to_string(),
            )
            .await
            .unwrap();
        assert_eq!(created.skill.id, "personal:my-skill");
        assert_eq!(
            created.skill.description,
            "My workflow: \"daily\"\nSecond line"
        );
        let skill_dir = temp.path().join("skills/my-skill");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(skill_dir.join("references/guide.md"), "# Guide\n").unwrap();

        let updated_content = skill_md("my-skill", "Updated");
        let updated = lib
            .update_skill(
                "personal:my-skill".to_string(),
                updated_content,
                Some(created.skill.version),
            )
            .await
            .unwrap();
        assert_eq!(updated.skill.description, "Updated");
        // Only the file the update replaced is kept.
        let backups: Vec<PathBuf> = fs::read_dir(temp.path().join("skill-backups/my-skill"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        let backed_up: Vec<String> = fs::read_dir(&backups[0])
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(backed_up, ["SKILL.md"]);
        assert!(
            fs::read_to_string(backups[0].join("SKILL.md"))
                .unwrap()
                .contains("Second line")
        );

        let deleted = lib
            .delete_personal_skill("personal:my-skill".to_string())
            .await
            .unwrap();
        assert_eq!(deleted["deleted"], json!(true));
        assert!(!skill_dir.exists());
        assert!(temp.path().join("skill-trash/my-skill").is_dir());
    }

    #[tokio::test]
    async fn clone_read_only_skill_into_personal_root() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let bundled = temp.path().join("bundled-skills");
        write_skill(&bundled, "pdf", "PDF work");
        fs::create_dir_all(bundled.join("pdf/.git")).unwrap();
        fs::write(bundled.join("pdf/.git/config"), "[core]\n").unwrap();
        let long_name = "a".repeat(64);
        write_skill(&bundled, &long_name, "Long name");
        lib.reload().await.unwrap();

        let cloned = lib
            .clone_skill("bundled:pdf".to_string(), Some("pdf-custom".to_string()))
            .await
            .unwrap();
        assert_eq!(cloned.skill.id, "personal:pdf-custom");
        assert!(cloned.content.contains("origin"));
        assert!(temp.path().join("skills/pdf-custom/SKILL.md").is_file());
        assert!(!temp.path().join("skills/pdf-custom/.git").exists());

        let auto = lib
            .clone_skill("bundled:pdf".to_string(), None)
            .await
            .unwrap();
        assert_eq!(auto.skill.id, "personal:pdf-copy");
        // `<64 chars>-copy` is not a valid name, so nothing is written.
        assert!(
            lib.clone_skill(format!("bundled:{long_name}"), None)
                .await
                .is_err()
        );
        assert!(
            !temp
                .path()
                .join(format!("skills/{long_name}-copy"))
                .exists()
        );
    }

    #[tokio::test]
    async fn detail_lists_and_reads_skill_directory_files() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "Learning workflow");
        let skill_dir = temp.path().join("skills").join("learn");
        let references = skill_dir.join("references");
        fs::create_dir_all(&references).unwrap();
        fs::write(references.join("guide.md"), "# Guide\n").unwrap();
        fs::create_dir_all(skill_dir.join(".venv/lib")).unwrap();
        fs::write(skill_dir.join(".venv/lib/site.py"), "").unwrap();
        lib.reload().await.unwrap();

        let detail = lib.get_skill_detail("personal:learn").await.unwrap();
        assert_eq!(detail.skill.directory, skill_dir.display().to_string());
        let paths: Vec<&str> = detail.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["SKILL.md", "references", "references/guide.md"]);
        assert_eq!(detail.files[1].kind, SkillFileKind::Directory);

        let file = lib
            .get_skill_file("personal:learn", "references/guide.md")
            .await
            .unwrap();
        assert_eq!(file.path, "references/guide.md");
        assert_eq!(file.content, "# Guide\n");
        assert!(
            lib.get_skill_file("personal:learn", "../outside.md")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_truncated_file_keeps_whole_characters() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "Learning workflow");
        // The 1 MiB cut lands in the middle of the two-byte `é`.
        let limit = MAX_SKILL_VIEW_FILE_BYTES as usize;
        let content = format!("{}é tail", "a".repeat(limit - 1));
        fs::write(temp.path().join("skills/learn/big.md"), &content).unwrap();
        lib.reload().await.unwrap();

        let file = lib
            .get_skill_file("personal:learn", "big.md")
            .await
            .unwrap();
        assert!(file.truncated);
        assert_eq!(file.content.len(), limit - 1);
        assert_eq!(file.size, content.len() as u64);
    }

    #[test]
    fn bundled_skills_declare_the_intended_execution_mode() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills");
        let mut parsed = Vec::new();
        for entry in fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            let skill_md = dir.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let content = fs::read_to_string(&skill_md).unwrap();
            parsed.push(
                parse_skill_md(dir.clone(), &content)
                    .unwrap_or_else(|err| panic!("{} is invalid: {err}", skill_md.display())),
            );
        }
        assert!(
            parsed.len() >= 10,
            "expected the bundled skills to be present, found {}",
            parsed.len()
        );

        // Only work that runs to completion without the conversation delegates;
        // everything else follows the inline default so it keeps the user, the
        // chat history, and the turn's attachments in reach.
        let mut subagents: Vec<&str> = parsed
            .iter()
            .filter(|skill| skill.is_subagent())
            .map(|skill| skill.frontmatter.name.as_str())
            .collect();
        subagents.sort_unstable();
        assert_eq!(subagents, ["auto-research", "claude-code", "codex"]);

        // `allowed-tools` is an upper bound, so a delegated skill that declares
        // one gets nothing else: trimming this list silently removes capability.
        let auto_research = parsed
            .iter()
            .find(|skill| skill.frontmatter.name == "auto-research")
            .unwrap();
        for tool in ["shell", "read_file", "write_file", "subagents_manager"] {
            assert!(
                auto_research.tools.iter().any(|name| name == tool),
                "auto-research must keep {tool} in its allowed-tools"
            );
        }
    }

    #[tokio::test]
    async fn skills_api_lists_records() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("bundled-skills"), "pdf", "PDF work");
        lib.reload().await.unwrap();

        // Strict tool calls send every field, with `null` for the unused ones.
        let output = Tool::call_raw(
            &lib,
            EngineBuilder::new().mock_ctx().base,
            json!({
                "type": "ListSkills",
                "include_inactive": null,
                "id": null,
                "path": null,
                "name": null,
                "new_name": null,
                "description": null,
                "content": null,
                "expected_version": null,
                "enabled": null
            }),
            vec![],
        )
        .await
        .unwrap();
        let value = serde_json::to_value(output.output).unwrap();
        assert!(value["result"].is_array());
        // The dashboard types the mode as `'inline' | 'subagent'`, so it has to
        // reach the wire as that lowercase word.
        assert_eq!(value["result"][0]["execution"], json!("inline"));
    }

    #[tokio::test]
    async fn update_rejects_external_edit() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "review", "original");
        lib.reload().await.unwrap();
        let original = lib.get_skill_detail("personal:review").await.unwrap();
        fs::write(
            temp.path().join("skills/review/SKILL.md"),
            skill_md("review", "external editor changed this"),
        )
        .unwrap();
        let result = lib
            .update_skill(
                "personal:review".into(),
                skill_md("review", "stale dashboard overwrite"),
                Some(original.skill.version),
            )
            .await;
        assert!(
            result.is_err(),
            "stale dashboard version overwrote an external edit"
        );
    }

    #[tokio::test]
    async fn invalid_skill_not_callable() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let dir = temp.path().join("skills/wrong-dir");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: actual-name\ndescription: mismatch\nexecution: subagent\n---\nbody\n",
        )
        .unwrap();
        lib.reload().await.unwrap();
        let skill = lib
            .list_managed_skills(true)
            .into_iter()
            .find(|s| s.name == "actual-name")
            .unwrap();
        assert!(!skill.active);
        assert!(
            !lib.subagent_set().contains_lowercase("skill_actual_name"),
            "dashboard-invalid skill is still executable"
        );
    }

    #[tokio::test]
    async fn a_skill_the_registry_rejects_is_listed_as_an_error() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        let dir = temp.path().join("skills/broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "no frontmatter at all\n").unwrap();
        lib.reload().await.unwrap();

        let skills = lib.list_managed_skills(true);
        let broken = find(&skills, "personal:broken");
        assert!(!broken.active && broken.has_error(), "{broken:?}");
        // The broken file is still there to read and fix.
        let detail = lib.get_skill_detail("personal:broken").await.unwrap();
        assert!(detail.content.contains("no frontmatter"));
    }
}
