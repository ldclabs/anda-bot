use anda_core::{
    Agent, BoxError, FunctionDefinition, Resource, Tool, ToolOutput, Usage, select_resources,
};
use anda_engine::{
    context::BaseCtx,
    extension::skill::{
        Skill, SkillManager, find_skill_files, format_skill_md, normalise_skill_agent_name,
        parse_skill_md, validate_skill_name,
    },
    subagent::{SubAgent, SubAgentSet},
    unix_ms,
};
use anda_kip::Response;
use chrono::{SecondsFormat, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, HashMap},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    Personal,
    Bundled,
    Shared,
    Legacy,
}

impl SkillSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            SkillSourceKind::Personal => "personal",
            SkillSourceKind::Bundled => "bundled",
            SkillSourceKind::Shared => "shared",
            SkillSourceKind::Legacy => "legacy",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SkillSourceKind::Personal => "Personal",
            SkillSourceKind::Bundled => "Bundled",
            SkillSourceKind::Shared => "Shared",
            SkillSourceKind::Legacy => "Legacy local copy",
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
}

#[derive(Debug, Clone)]
struct SkillSource {
    kind: SkillSourceKind,
    priority: u32,
    path: PathBuf,
    editable: bool,
}

impl SkillSource {
    fn info(&self) -> SkillSourceInfo {
        SkillSourceInfo {
            source: self.kind,
            source_label: self.kind.label().to_string(),
            priority: self.priority,
            path: self.path.display().to_string(),
            editable: self.editable,
            exists: self.path.is_dir(),
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

    fn info(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: SkillDiagnosticSeverity::Info,
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
    #[serde(default)]
    pub pinned: Vec<String>,
    #[serde(default)]
    pub last_reload_ms: u64,
}

impl Default for SkillManifest {
    fn default() -> Self {
        Self {
            version: 1,
            disabled: BTreeMap::new(),
            pinned: Vec::new(),
            last_reload_ms: 0,
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
    pub file_count: usize,
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

    fn is_user_owned(&self) -> bool {
        matches!(
            self.source,
            SkillSourceKind::Personal | SkillSourceKind::Legacy
        )
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
        #[serde(default)]
        include_inactive: bool,
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
    parsed: Option<Skill>,
    base_dir: PathBuf,
}

#[derive(Clone, Default)]
struct SkillLibraryState {
    records: Vec<SkillRecord>,
}

#[derive(Clone)]
pub struct SkillLibrary {
    home_dir: PathBuf,
    personal_dir: PathBuf,
    bundled_dir: PathBuf,
    shared_dirs: Vec<PathBuf>,
    manifest_path: PathBuf,
    backups_dir: PathBuf,
    trash_dir: PathBuf,
    skill_manager: Arc<SkillManager>,
    default_skill_tools: Vec<String>,
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
        default_skill_tools: Vec<String>,
        known_tools: BTreeSet<String>,
    ) -> Self {
        Self {
            manifest_path: home_dir.join(MANIFEST_FILE_NAME),
            backups_dir: home_dir.join(BACKUPS_DIR_NAME),
            trash_dir: home_dir.join(TRASH_DIR_NAME),
            home_dir,
            personal_dir,
            bundled_dir,
            shared_dirs,
            skill_manager,
            default_skill_tools,
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
            "read_file".to_string(),
            "search_file".to_string(),
            "note".to_string(),
            "tools_select".to_string(),
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
            default_skill_tools.clone(),
            BTreeSet::from_iter(default_skill_tools),
        ))
    }

    pub fn skill_sources(&self) -> Vec<SkillSourceInfo> {
        self.sources()
            .into_iter()
            .map(|source| source.info())
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

    pub fn get_skill_detail(&self, id: &str) -> Result<ManagedSkillDetail, BoxError> {
        let tools_usage = (self.tools_usage_reader)();
        self.record_by_id(id)
            .map(|record| ManagedSkillDetail {
                files: list_skill_files(&record.base_dir).unwrap_or_default(),
                content: record.content,
                skill: attach_usage(record.managed, &tools_usage),
            })
            .ok_or_else(|| format!("skill not found: {id}").into())
    }

    pub fn get_skill_file(&self, id: &str, path: &str) -> Result<SkillFileContent, BoxError> {
        let record = self
            .record_by_id(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        read_skill_file(id, &record.base_dir, path)
    }

    pub async fn reload(&self) -> Result<Vec<ManagedSkill>, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let manifest = self.load_manifest().await?;
        self.reload_locked(manifest).await
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
        self.reload_locked(manifest).await?;
        self.get_skill_detail(&personal_skill_id(&name))
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
        if !record.managed.is_user_owned() {
            return Err("only Personal skills can be updated from the Dashboard".into());
        }
        if let Some(expected_version) = expected_version
            && expected_version != record.managed.version
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

        self.ensure_existing_user_skill_dir(&record.base_dir)
            .await?;
        self.backup_skill_dir(&record.managed.name, &record.base_dir)
            .await?;
        atomic_write_text(&record.base_dir.join("SKILL.md"), &content).await?;
        let manifest = self.load_manifest().await?;
        self.reload_locked(manifest).await?;
        self.get_skill_detail(&id)
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
        let Some(mut parsed) = record.parsed.clone() else {
            return Err("only valid skills can be cloned".into());
        };
        let new_name = match new_name {
            Some(name) => normalize_skill_name(name)?,
            None => self.available_clone_name(&record.managed.name),
        };
        let target_dir = self.personal_dir.join(&new_name);
        self.ensure_new_personal_skill_dir(&target_dir).await?;

        copy_dir_regular_files(&record.base_dir, &target_dir)?;
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
        self.reload_locked(manifest).await?;
        self.get_skill_detail(&personal_or_legacy_skill_id(
            &new_name,
            self.bundled_has_skill(&new_name),
        ))
    }

    pub async fn set_skill_enabled(
        &self,
        id: String,
        enabled: bool,
    ) -> Result<Vec<ManagedSkill>, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let record = self
            .record_by_id(&id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        let mut manifest = self.load_manifest().await?;
        if enabled {
            manifest.disabled.remove(&id);
            if record.managed.source == SkillSourceKind::Legacy {
                manifest
                    .disabled
                    .remove(&personal_skill_id(&record.managed.name));
            }
        } else {
            manifest.disabled.insert(
                id,
                DisabledSkill {
                    disabled_at: unix_ms(),
                    reason: "User disabled from Dashboard".to_string(),
                },
            );
        }
        self.reload_locked(manifest).await
    }

    pub async fn delete_personal_skill(&self, id: String) -> Result<Value, BoxError> {
        let _guard = self.operation_lock.lock().await;
        let record = self
            .record_by_id(&id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        if !record.managed.is_user_owned() {
            return Err("only Personal skills can be deleted from the Dashboard".into());
        }
        self.ensure_existing_user_skill_dir(&record.base_dir)
            .await?;

        let trash_parent = self
            .trash_dir
            .join(&record.managed.name)
            .join(timestamp_for_path());
        let trash_dir = unique_path(trash_parent);
        if let Some(parent) = trash_dir.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&record.base_dir, &trash_dir)
            .await
            .map_err(|err| format!("failed to move skill to trash: {err}"))?;

        let mut manifest = self.load_manifest().await?;
        manifest.disabled.remove(&id);
        self.reload_locked(manifest).await?;
        Ok(json!({
            "deleted": true,
            "id": id,
            "trash_path": trash_dir.display().to_string()
        }))
    }

    pub fn validate_skill(&self, content: String) -> SkillValidationResult {
        validate_skill_content(None, &content)
    }

    async fn reload_locked(
        &self,
        mut manifest: SkillManifest,
    ) -> Result<Vec<ManagedSkill>, BoxError> {
        manifest.version = 1;
        manifest.last_reload_ms = unix_ms();
        let mut records = self.scan_records(&manifest).await;
        apply_effective_state(&mut records, &manifest);
        self.write_manifest(&manifest).await?;
        *self.state.write() = SkillLibraryState { records };
        if let Err(err) = self.skill_manager.load().await {
            log::warn!("failed to reload raw skills_manager after library scan: {err}");
        }
        Ok(self.list_managed_skills(true))
    }

    fn sources(&self) -> Vec<SkillSource> {
        let mut sources = vec![
            SkillSource {
                kind: SkillSourceKind::Personal,
                priority: 0,
                path: self.personal_dir.clone(),
                editable: true,
            },
            SkillSource {
                kind: SkillSourceKind::Bundled,
                priority: 1,
                path: self.bundled_dir.clone(),
                editable: false,
            },
        ];
        for dir in &self.shared_dirs {
            sources.push(SkillSource {
                kind: SkillSourceKind::Shared,
                priority: 2,
                path: dir.clone(),
                editable: false,
            });
        }
        sources
    }

    async fn scan_records(&self, manifest: &SkillManifest) -> Vec<SkillRecord> {
        let mut records = Vec::new();
        for source in self.sources() {
            if !source.path.is_dir() {
                continue;
            }
            let files = match find_skill_files(&source.path).await {
                Ok(files) => files,
                Err(err) => {
                    log::warn!("failed to scan skills at {}: {err}", source.path.display());
                    continue;
                }
            };
            for path in files {
                records.push(self.scan_skill_file(&source, &path).await);
            }
        }
        classify_legacy_personal_records(&mut records);
        for record in &mut records {
            record.managed.disabled = is_disabled(manifest, &record.managed);
        }
        records.sort_by(|left, right| {
            left.managed
                .priority
                .cmp(&right.managed.priority)
                .then_with(|| left.managed.name.cmp(&right.managed.name))
                .then_with(|| left.managed.path.cmp(&right.managed.path))
        });
        records
    }

    async fn scan_skill_file(&self, source: &SkillSource, path: &Path) -> SkillRecord {
        let base_dir = path.parent().unwrap_or(source.path.as_path()).to_path_buf();
        let fallback_name = fallback_skill_name(&base_dir);
        let mut diagnostics = Vec::new();
        let mut content = String::new();
        let mut parsed = None;
        let mut size = None;
        let mut updated_at = None;

        match tokio::fs::symlink_metadata(path).await {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.is_file() {
                    diagnostics.push(SkillDiagnostic::error(
                        "not_regular_file",
                        "SKILL.md must be a regular file",
                    ));
                }
                size = Some(meta.len());
                if meta.len() > MAX_SKILL_FILE_BYTES {
                    diagnostics.push(SkillDiagnostic::error(
                        "file_too_large",
                        format!("SKILL.md must be at most {MAX_SKILL_FILE_BYTES} bytes"),
                    ));
                }
                updated_at = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis() as u64);
            }
            Err(err) => diagnostics.push(SkillDiagnostic::error(
                "metadata_failed",
                format!("failed to inspect SKILL.md: {err}"),
            )),
        }

        if diagnostics
            .iter()
            .all(|d| d.severity != SkillDiagnosticSeverity::Error)
        {
            match tokio::fs::read(path).await {
                Ok(bytes) => {
                    let version = content_version_bytes(&bytes);
                    match anda_core::text_from_bytes(&bytes) {
                        Some(text) => {
                            content = text.into_owned();
                            match parse_skill_md(base_dir.clone(), &content) {
                                Ok(skill) => {
                                    if source.kind == SkillSourceKind::Personal
                                        && base_dir.file_name()
                                            != Some(OsStr::new(&skill.frontmatter.name))
                                    {
                                        diagnostics.push(SkillDiagnostic::error(
                                            "name_directory_mismatch",
                                            "personal skill frontmatter name must match its directory",
                                        ));
                                    }
                                    diagnostics.extend(unknown_tool_diagnostics(
                                        &skill.tools,
                                        self.known_tools.as_ref(),
                                    ));
                                    parsed = Some(skill);
                                }
                                Err(err) => diagnostics.push(SkillDiagnostic::error(
                                    "parse_failed",
                                    format!("invalid SKILL.md: {err}"),
                                )),
                            }
                        }
                        None => diagnostics.push(SkillDiagnostic::error(
                            "decode_failed",
                            "SKILL.md must be readable as UTF-8 or the platform text encoding",
                        )),
                    }
                    return build_record(RecordBuildInput {
                        source,
                        path,
                        base_dir,
                        fallback_name,
                        diagnostics,
                        content,
                        parsed,
                        size,
                        updated_at,
                        version,
                    });
                }
                Err(err) => diagnostics.push(SkillDiagnostic::error(
                    "read_failed",
                    format!("failed to read SKILL.md: {err}"),
                )),
            }
        }

        build_record(RecordBuildInput {
            source,
            path,
            base_dir,
            fallback_name,
            diagnostics,
            content: content.clone(),
            parsed,
            size,
            updated_at,
            version: content_version(&content),
        })
    }

    async fn load_manifest(&self) -> Result<SkillManifest, BoxError> {
        match tokio::fs::read(&self.manifest_path).await {
            Ok(bytes) => {
                let Some(text) = anda_core::text_from_bytes(&bytes) else {
                    return Ok(SkillManifest::default());
                };
                Ok(serde_json::from_str(&text).unwrap_or_default())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SkillManifest::default()),
            Err(err) => Err(format!(
                "failed to read skill manifest {}: {err}",
                self.manifest_path.display()
            )
            .into()),
        }
    }

    async fn write_manifest(&self, manifest: &SkillManifest) -> Result<(), BoxError> {
        tokio::fs::create_dir_all(&self.home_dir).await?;
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

    async fn ensure_existing_user_skill_dir(&self, dir: &Path) -> Result<(), BoxError> {
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

    async fn backup_skill_dir(&self, skill_name: &str, dir: &Path) -> Result<PathBuf, BoxError> {
        let backup = unique_path(self.backups_dir.join(skill_name).join(timestamp_for_path()));
        if let Some(parent) = backup.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        copy_dir_regular_files(dir, &backup)?;
        Ok(backup)
    }

    fn available_clone_name(&self, base_name: &str) -> String {
        let mut candidate = format!("{base_name}-copy");
        let mut suffix = 2usize;
        while self.personal_dir.join(&candidate).exists() {
            candidate = format!("{base_name}-copy-{suffix}");
            suffix += 1;
        }
        candidate
    }

    fn bundled_has_skill(&self, name: &str) -> bool {
        self.state.read().records.iter().any(|record| {
            record.managed.source == SkillSourceKind::Bundled && record.managed.name == name
        })
    }

    fn active_record_for_agent(&self, lowercase_name: &str) -> Option<SkillRecord> {
        self.state
            .read()
            .records
            .iter()
            .find(|record| {
                record.managed.active
                    && record
                        .managed
                        .agent_name
                        .eq_ignore_ascii_case(lowercase_name)
            })
            .cloned()
    }

    fn active_records(&self) -> Vec<SkillRecord> {
        self.state
            .read()
            .records
            .iter()
            .filter(|record| record.managed.active)
            .cloned()
            .collect()
    }

    fn subagent_from_record(&self, record: &SkillRecord) -> Option<SubAgent> {
        let skill = record.parsed.as_ref()?;
        let mut agent = SubAgent::from(skill);
        for tool in &self.default_skill_tools {
            if !agent.tools.contains(tool) {
                agent.tools.push(tool.clone());
            }
        }
        Some(agent)
    }
}

impl SubAgentSet for SkillLibrary {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn contains_lowercase(&self, lowercase_name: &str) -> bool {
        self.active_record_for_agent(lowercase_name).is_some()
    }

    fn get_lowercase(&self, lowercase_name: &str) -> Option<SubAgent> {
        self.active_record_for_agent(lowercase_name)
            .and_then(|record| self.subagent_from_record(&record))
    }

    fn definitions(&self, names: Option<&[String]>) -> Vec<FunctionDefinition> {
        let names = names.map(|names| {
            names
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect::<BTreeSet<_>>()
        });
        self.active_records()
            .iter()
            .filter(|record| {
                names
                    .as_ref()
                    .map(|names| names.contains(&record.managed.agent_name.to_ascii_lowercase()))
                    .unwrap_or(true)
            })
            .filter_map(|record| self.subagent_from_record(record))
            .map(|agent| agent.definition())
            .collect()
    }

    fn select_resources(&self, name: &str, resources: &mut Vec<Resource>) -> Vec<Resource> {
        if resources.is_empty() {
            return Vec::new();
        }

        self.active_record_for_agent(&name.to_ascii_lowercase())
            .and_then(|record| self.subagent_from_record(&record))
            .map(|agent| {
                let supported_tags = agent.supported_resource_tags();
                select_resources(resources, &supported_tags)
            })
            .unwrap_or_default()
    }
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
                json!(self.list_managed_skills(include_inactive))
            }
            SkillsApiArgs::GetSkill { id } => json!(self.get_skill_detail(&id)?),
            SkillsApiArgs::GetSkillFile { id, path } => json!(self.get_skill_file(&id, &path)?),
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

struct RecordBuildInput<'a> {
    source: &'a SkillSource,
    path: &'a Path,
    base_dir: PathBuf,
    fallback_name: String,
    diagnostics: Vec<SkillDiagnostic>,
    content: String,
    parsed: Option<Skill>,
    size: Option<u64>,
    updated_at: Option<u64>,
    version: String,
}

fn build_record(input: RecordBuildInput<'_>) -> SkillRecord {
    let RecordBuildInput {
        source,
        path,
        base_dir,
        fallback_name,
        diagnostics,
        content,
        parsed,
        size,
        updated_at,
        version,
    } = input;

    let (name, agent_name, description, compatibility, allowed_tools, metadata) =
        match parsed.as_ref() {
            Some(skill) => (
                skill.frontmatter.name.clone(),
                skill.agent_name.clone(),
                skill.frontmatter.description.clone(),
                skill.frontmatter.compatibility.clone(),
                skill.tools.clone(),
                json!(skill.frontmatter.metadata),
            ),
            None => {
                let agent_name = validate_skill_name(&fallback_name)
                    .map(|_| normalise_skill_agent_name(&fallback_name))
                    .unwrap_or_else(|_| format!("skill_invalid_{}", short_hash(path)));
                (
                    fallback_name,
                    agent_name,
                    String::new(),
                    None,
                    Vec::new(),
                    json!({}),
                )
            }
        };
    let id = skill_id(source.kind, &name);
    SkillRecord {
        managed: ManagedSkill {
            id,
            source: source.kind,
            source_label: source.kind.label().to_string(),
            priority: source.priority,
            name,
            agent_name,
            description,
            compatibility,
            allowed_tools,
            metadata,
            path: path.display().to_string(),
            directory: base_dir.display().to_string(),
            editable: source.editable,
            active: false,
            disabled: false,
            shadowed_by: None,
            diagnostics,
            updated_at,
            size,
            file_count: count_skill_files(&base_dir),
            usage: None,
            version,
        },
        content,
        parsed,
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
    let callable = skill_usage_key(&agent_name);
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

fn skill_usage_key(agent_name: &str) -> String {
    format!("sa_{}", agent_name.to_ascii_lowercase())
}

fn classify_legacy_personal_records(records: &mut [SkillRecord]) {
    let bundled_names = records
        .iter()
        .filter(|record| record.managed.source == SkillSourceKind::Bundled)
        .map(|record| record.managed.name.clone())
        .collect::<BTreeSet<_>>();
    for record in records.iter_mut() {
        if record.managed.source == SkillSourceKind::Personal
            && bundled_names.contains(&record.managed.name)
        {
            record.managed.source = SkillSourceKind::Legacy;
            record.managed.source_label = SkillSourceKind::Legacy.label().to_string();
            record.managed.id = skill_id(SkillSourceKind::Legacy, &record.managed.name);
            record.managed.editable = true;
            record.managed.diagnostics.push(SkillDiagnostic::info(
                "legacy_local_copy",
                "This Personal skill shadows a bundled skill with the same name.",
            ));
        }
    }
}

fn apply_effective_state(records: &mut [SkillRecord], manifest: &SkillManifest) {
    let mut winners: BTreeMap<String, String> = BTreeMap::new();
    for record in records.iter_mut() {
        record.managed.active = false;
        record.managed.shadowed_by = None;
        record.managed.disabled = is_disabled(manifest, &record.managed);
        if record.managed.has_error() || record.managed.disabled {
            continue;
        }
        if let Some(winner) = winners.get(&record.managed.agent_name) {
            record.managed.shadowed_by = Some(winner.clone());
            record.managed.diagnostics.push(SkillDiagnostic::warning(
                "shadowed",
                format!("Shadowed by higher-priority skill {winner}."),
            ));
            continue;
        }
        record.managed.active = true;
        winners.insert(record.managed.agent_name.clone(), record.managed.id.clone());
    }
}

fn is_disabled(manifest: &SkillManifest, skill: &ManagedSkill) -> bool {
    manifest.disabled.contains_key(&skill.id)
        || (skill.source == SkillSourceKind::Legacy
            && manifest
                .disabled
                .contains_key(&personal_skill_id(&skill.name)))
}

fn skill_id(source: SkillSourceKind, name: &str) -> String {
    format!("{}:{name}", source.as_str())
}

fn personal_skill_id(name: &str) -> String {
    skill_id(SkillSourceKind::Personal, name)
}

fn personal_or_legacy_skill_id(name: &str, has_bundled: bool) -> String {
    if has_bundled {
        skill_id(SkillSourceKind::Legacy, name)
    } else {
        personal_skill_id(name)
    }
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

fn unknown_tool_diagnostics(
    allowed_tools: &[String],
    known_tools: &BTreeSet<String>,
) -> Vec<SkillDiagnostic> {
    allowed_tools
        .iter()
        .filter(|tool| {
            !known_tools.contains(tool.as_str())
                && !tool.starts_with("mcp__")
                && !tool.starts_with("plugin__")
        })
        .map(|tool| {
            SkillDiagnostic::warning(
                "unknown_tool",
                format!("allowed-tools includes unknown tool {tool}."),
            )
        })
        .collect()
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
    if cfg!(windows) && path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|err| format!("failed to replace {}: {err}", path.display()).into())
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
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_dir_regular_files(&from, &to)?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn count_skill_files(base_dir: &Path) -> usize {
    list_skill_files(base_dir)
        .map(|files| {
            files
                .iter()
                .filter(|file| file.kind == SkillFileKind::File)
                .count()
        })
        .unwrap_or(0)
}

fn list_skill_files(base_dir: &Path) -> Result<Vec<SkillFileEntry>, BoxError> {
    let mut files = Vec::new();
    collect_skill_files(base_dir, base_dir, &mut files)?;
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    Ok(files)
}

fn collect_skill_files(
    base_dir: &Path,
    dir: &Path,
    files: &mut Vec<SkillFileEntry>,
) -> Result<(), BoxError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let meta = entry.metadata()?;
        let path = entry.path();
        let Some(relative_path) = skill_relative_display_path(base_dir, &path) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_dir() {
            files.push(SkillFileEntry {
                path: relative_path,
                name,
                kind: SkillFileKind::Directory,
                size: None,
                updated_at: modified_at_ms(&meta),
            });
            collect_skill_files(base_dir, &path, files)?;
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
    Ok(())
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

fn content_version(content: &str) -> String {
    content_version_bytes(content.as_bytes())
}

fn content_version_bytes(bytes: &[u8]) -> String {
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
    use anda_engine::engine::EngineBuilder;
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

    fn library(home: &Path) -> SkillLibrary {
        SkillLibrary::for_test(home.to_path_buf()).as_ref().clone()
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
        let legacy = skills
            .iter()
            .find(|skill| skill.id == "legacy:learn")
            .unwrap();
        assert!(legacy.active);
        assert_eq!(legacy.source, SkillSourceKind::Legacy);
        let bundled = skills
            .iter()
            .find(|skill| skill.id == "bundled:learn")
            .unwrap();
        assert!(!bundled.active);
        assert_eq!(bundled.shadowed_by.as_deref(), Some("legacy:learn"));
        assert!(
            skills
                .iter()
                .any(|skill| skill.id == "shared:docx" && skill.active)
        );
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

        let detail = lib.get_skill_detail("personal:learn").unwrap();
        assert_eq!(detail.skill.usage.unwrap().requests, 3);
        assert_eq!(detail.skill.file_count, 1);
        assert!(
            detail
                .files
                .iter()
                .any(|file| file.path == "SKILL.md" && file.kind == SkillFileKind::File)
        );
    }

    #[tokio::test]
    async fn disabling_higher_priority_skill_promotes_next_copy() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("skills"), "learn", "personal");
        write_skill(&temp.path().join("bundled-skills"), "learn", "bundled");

        lib.reload().await.unwrap();
        lib.set_skill_enabled("legacy:learn".to_string(), false)
            .await
            .unwrap();

        let skills = lib.list_managed_skills(true);
        assert!(
            !skills
                .iter()
                .find(|skill| skill.id == "legacy:learn")
                .unwrap()
                .active
        );
        assert!(
            skills
                .iter()
                .find(|skill| skill.id == "bundled:learn")
                .unwrap()
                .active
        );
        assert!(
            lib.prompt_skills()
                .iter()
                .any(|skill| skill.name == "learn")
        );
        assert_eq!(lib.definitions(None).len(), 1);
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
        assert!(temp.path().join("skill-backups/my-skill").is_dir());

        let deleted = lib
            .delete_personal_skill("personal:my-skill".to_string())
            .await
            .unwrap();
        assert_eq!(deleted["deleted"], json!(true));
        assert!(!temp.path().join("skills/my-skill").exists());
        assert!(temp.path().join("skill-trash/my-skill").is_dir());
    }

    #[tokio::test]
    async fn clone_read_only_skill_into_personal_root() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("bundled-skills"), "pdf", "PDF work");
        lib.reload().await.unwrap();

        let cloned = lib
            .clone_skill("bundled:pdf".to_string(), Some("pdf-custom".to_string()))
            .await
            .unwrap();
        assert_eq!(cloned.skill.id, "personal:pdf-custom");
        assert!(cloned.content.contains("origin"));
        assert!(temp.path().join("skills/pdf-custom/SKILL.md").is_file());
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
        lib.reload().await.unwrap();

        let detail = lib.get_skill_detail("personal:learn").unwrap();
        assert_eq!(detail.skill.directory, skill_dir.display().to_string());
        assert_eq!(detail.skill.file_count, 2);
        assert!(
            detail
                .files
                .iter()
                .any(|file| file.path == "references" && file.kind == SkillFileKind::Directory)
        );
        assert!(
            detail
                .files
                .iter()
                .any(|file| file.path == "references/guide.md" && file.kind == SkillFileKind::File)
        );

        let file = lib
            .get_skill_file("personal:learn", "references/guide.md")
            .unwrap();
        assert_eq!(file.path, "references/guide.md");
        assert_eq!(file.content, "# Guide\n");
        assert!(
            lib.get_skill_file("personal:learn", "../outside.md")
                .is_err()
        );
    }

    #[tokio::test]
    async fn skills_api_lists_records() {
        let temp = tempdir().unwrap();
        let lib = library(temp.path());
        write_skill(&temp.path().join("bundled-skills"), "pdf", "PDF work");
        lib.reload().await.unwrap();

        let output = Tool::call_raw(
            &lib,
            EngineBuilder::new().mock_ctx().base,
            json!({
                "type": "ListSkills",
                "include_inactive": true,
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
    }
}
