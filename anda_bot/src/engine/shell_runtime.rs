use anda_core::{BoxError, Principal, RequestMeta, StateFeatures};
use anda_engine::{
    context::BaseCtx,
    extension::shell::{ExecArgs, ExecOutput, Executor, NativeRuntime},
};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use super::agent::SessionRequestMeta;
use crate::util::request_meta::keys;
use crate::util::windows_process::suppress_console_window;

const MAX_CLI_WORKSPACES: usize = 64;
const CLI_WORKSPACE_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Directories explicitly registered by the local owner through the daemon API.
/// Request metadata alone cannot add a directory to this set.
#[derive(Clone)]
pub(crate) struct CliWorkspaceGrants {
    owner: Principal,
    paths: Arc<RwLock<HashMap<PathBuf, Instant>>>,
}

impl CliWorkspaceGrants {
    pub(crate) fn new(owner: Principal) -> Self {
        Self {
            owner,
            paths: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(super) fn owner(&self) -> Principal {
        self.owner
    }

    /// Only an owner's live, out-of-band grants may expand media roots.
    pub(super) fn paths_for(&self, caller: &Principal) -> Vec<PathBuf> {
        if *caller != self.owner {
            return Vec::new();
        }
        let now = Instant::now();
        self.paths
            .read()
            .iter()
            .filter(|(_, expiry)| **expiry > now)
            .map(|(path, _)| path.clone())
            .collect()
    }

    pub(crate) async fn register(&self, workspace: &Path) -> Result<PathBuf, BoxError> {
        let workspace = canonical_directory(workspace).await?;
        let now = Instant::now();
        let mut paths = self.paths.write();
        paths.retain(|_, expiry| *expiry > now);
        if paths.len() >= MAX_CLI_WORKSPACES && !paths.contains_key(&workspace) {
            return Err(
                "too many registered CLI workspaces; restart the daemon to clear them".into(),
            );
        }
        paths.insert(workspace.clone(), now + CLI_WORKSPACE_LIFETIME);
        Ok(workspace)
    }

    async fn resolve(&self, workspace: &Path) -> Result<PathBuf, BoxError> {
        let workspace = canonical_directory(workspace).await?;
        let allowed = self
            .paths
            .read()
            .get(&workspace)
            .is_some_and(|expiry| *expiry > Instant::now());
        if !allowed {
            return Err(format!(
                "CLI workspace {} is not registered; reconnect the interactive CLI",
                workspace.display()
            )
            .into());
        }
        Ok(workspace)
    }
}

fn cli_workspace_request(meta: &RequestMeta) -> Result<Option<(PathBuf, PathBuf)>, BoxError> {
    let Some(source) = meta.get_extra_as::<String>(keys::SOURCE) else {
        return Ok(None);
    };
    // Desktop chats have stable opaque sources so several chats can share a
    // project. The owner and out-of-band registration checks below still apply.
    if source.starts_with("desktop:") {
        return Ok(meta
            .get_extra_as::<PathBuf>(keys::WORKSPACE)
            .map(|workspace| (workspace.clone(), workspace)));
    }
    let source_workspace = match source.strip_prefix("cli:") {
        Some(path) if Path::new(path).is_absolute() => path,
        Some(path) => match path.strip_prefix("voice:") {
            Some(path) if Path::new(path).is_absolute() => path,
            _ => return Ok(None),
        },
        None if Path::new(&source).is_absolute() => &source,
        None => return Ok(None),
    };
    let workspace = meta
        .get_extra_as::<PathBuf>(keys::WORKSPACE)
        .ok_or("CLI request is missing its workspace")?;
    Ok(Some((source_workspace.into(), workspace)))
}

impl CliWorkspaceGrants {
    pub(crate) async fn authorize_cron_workspace(
        &self,
        caller: &Principal,
        meta: &RequestMeta,
    ) -> Result<Option<PathBuf>, BoxError> {
        let Some((source, workspace)) = cli_workspace_request(meta)? else {
            return Ok(None);
        };
        if meta
            .get_extra_as::<bool>(keys::EXTERNAL_USER)
            .unwrap_or(false)
        {
            return Err("External IM users cannot use a registered local workspace".into());
        }
        if *caller != self.owner {
            return Err("only the local owner may use a registered CLI workspace".into());
        }
        let resolved = self.resolve(&workspace).await?;
        if canonical_directory(&source).await? != resolved {
            return Err("CLI source and workspace do not match".into());
        }
        Ok(Some(resolved))
    }
}

async fn canonical_directory(workspace: &Path) -> Result<PathBuf, BoxError> {
    if !workspace.is_absolute() {
        return Err(format!(
            "workspace must be an absolute path: {}",
            workspace.display()
        )
        .into());
    }
    let resolved = tokio::fs::canonicalize(workspace)
        .await
        .map_err(|err| format!("cannot resolve workspace {}: {err}", workspace.display()))?;
    if !tokio::fs::metadata(&resolved).await?.is_dir() {
        return Err(format!("workspace is not a directory: {}", workspace.display()).into());
    }
    Ok(resolved)
}

pub struct NativeShellRuntime {
    inner: NativeRuntime,
    cli_workspaces: Option<CliWorkspaceGrants>,
}

impl NativeShellRuntime {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            inner: NativeRuntime::new(workspace),
            cli_workspaces: None,
        }
    }

    pub(super) fn with_cli_workspaces(mut self, grants: CliWorkspaceGrants) -> Self {
        self.cli_workspaces = Some(grants);
        self
    }

    pub fn insecure(self) -> Self {
        Self {
            inner: self.inner.insecure(),
            cli_workspaces: self.cli_workspaces,
        }
    }

    async fn cli_workspace(&self, ctx: &BaseCtx) -> Result<Option<PathBuf>, BoxError> {
        let meta: RequestMeta = ctx
            .get_state::<SessionRequestMeta>()
            .map(|state| state.get())
            .unwrap_or_else(|| ctx.meta().clone());
        let Some((source_workspace, workspace)) = cli_workspace_request(&meta)? else {
            return Ok(None);
        };
        let saved_grant = match ctx.get_state::<SessionRequestMeta>() {
            Some(session) => session.cron_workspace(),
            None => ctx.get_state::<crate::cron::CronWorkspaceGrant>(),
        };
        let resolved = if let Some(grant) = saved_grant {
            let resolved = canonical_directory(&workspace).await?;
            if grant.caller != *ctx.caller()
                || grant.path != resolved
                || canonical_directory(&source_workspace).await? != resolved
            {
                return Err("Scheduled workspace does not match its saved authorization".into());
            }
            resolved
        } else {
            let grants = self
                .cli_workspaces
                .as_ref()
                .ok_or("CLI workspace registration is unavailable")?;
            grants
                .authorize_cron_workspace(ctx.caller(), &meta)
                .await?
                .ok_or("CLI request is missing its workspace")?
        };

        // NativeRuntime also reads the context's original metadata. A resumed
        // session may have been created in a nested directory, which would
        // otherwise override this newer, registered parent directory.
        if let Some(frozen) = ctx.meta().get_extra_as::<PathBuf>(keys::WORKSPACE)
            && let Ok(frozen) = tokio::fs::canonicalize(frozen).await
            && frozen != resolved
            && frozen.starts_with(&resolved)
        {
            return Err(
                "CLI workspace changed inside an active session; start a new conversation".into(),
            );
        }
        Ok(Some(resolved))
    }
}

#[async_trait]
impl Executor for NativeShellRuntime {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn os(&self) -> &str {
        self.inner.os()
    }

    fn workspace(&self) -> &PathBuf {
        self.inner.workspace()
    }

    fn shell(&self) -> &str {
        self.inner.shell()
    }

    async fn execute(
        &self,
        ctx: BaseCtx,
        input: ExecArgs,
        mut envs: HashMap<String, String>,
    ) -> Result<ExecOutput, BoxError> {
        let cli_workspace = self.cli_workspace(&ctx).await?;
        augment_command_path(&mut envs);
        let mut command = NativeRuntime::build_shell_command(&input.command);
        suppress_console_window(&mut command);
        if let Some(workspace) = cli_workspace {
            // This root was registered out of band by the authenticated owner.
            // NativeRuntime still applies its normal request-narrowing check.
            NativeRuntime::new(workspace)
                .insecure()
                .execute_command(ctx, self.name(), command, envs, Some(input))
                .await
        } else {
            self.inner
                .execute_command(ctx, self.name(), command, envs, Some(input))
                .await
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn augment_command_path(envs: &mut HashMap<String, String>) {
    let base_path = envs
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok());

    if let Some(path) = enriched_path_value(base_path.as_deref(), default_tool_path_candidates()) {
        envs.insert("PATH".to_string(), path);
    }
}

#[cfg(target_os = "windows")]
fn augment_command_path(_envs: &mut HashMap<String, String>) {}

#[cfg(not(target_os = "windows"))]
fn default_tool_path_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(dir) = current_exe.parent()
    {
        paths.push(dir.to_path_buf());
    }

    if let Some(home_dir) = std::env::home_dir() {
        paths.push(home_dir.join(".local").join("bin"));
        paths.push(home_dir.join(".cargo").join("bin"));
    }

    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);

    paths
}

#[cfg(not(target_os = "windows"))]
fn enriched_path_value(
    base_path: Option<&str>,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<String> {
    let mut paths = base_path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();

    for candidate in candidates {
        if !candidate.as_os_str().is_empty() && !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }

    std::env::join_paths(paths)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cli_meta(workspace: &Path) -> RequestMeta {
        let mut meta = RequestMeta::default();
        meta.extra.insert(
            keys::SOURCE.to_string(),
            json!(format!("cli:{}", workspace.display())),
        );
        meta.extra.insert(
            keys::WORKSPACE.to_string(),
            json!(workspace.to_string_lossy()),
        );
        meta
    }

    fn pwd_command() -> &'static str {
        if cfg!(windows) { "cd" } else { "pwd" }
    }

    fn assert_same_directory(actual: Option<&str>, expected: &Path) {
        let actual = actual.expect("shell should report its working directory");
        let actual = Path::new(actual.trim())
            .canonicalize()
            .expect("reported working directory should resolve");
        let expected = expected
            .canonicalize()
            .expect("expected working directory should resolve");
        assert_eq!(actual, expected);
    }

    #[test]
    fn new_exposes_native_runtime_metadata() {
        let workspace = PathBuf::from("/tmp/anda-shell-test");
        let runtime = NativeShellRuntime::new(workspace.clone());

        assert_eq!(runtime.workspace(), &workspace);
        assert!(!runtime.name().is_empty());
        assert!(!runtime.os().is_empty());
        assert!(!runtime.shell().is_empty());
    }

    #[test]
    fn insecure_preserves_workspace() {
        let workspace = PathBuf::from("/tmp/anda-shell-test");
        let runtime = NativeShellRuntime::new(workspace.clone()).insecure();

        assert_eq!(runtime.workspace(), &workspace);
    }

    #[tokio::test]
    async fn cli_shell_uses_only_an_owner_registered_directory() {
        let temp = tempfile::tempdir().unwrap();
        let default = temp.path().join("default");
        let project = temp.path().join("project");
        let second_project = temp.path().join("second-project");
        tokio::fs::create_dir_all(&default).await.unwrap();
        tokio::fs::create_dir_all(&project).await.unwrap();
        tokio::fs::create_dir_all(&second_project).await.unwrap();
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        let runtime = NativeShellRuntime::new(default)
            .with_cli_workspaces(grants.clone())
            .insecure();
        let ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        ctx.set_state(SessionRequestMeta::new(cli_meta(&project)));

        assert!(
            runtime
                .cli_workspace(&ctx)
                .await
                .unwrap_err()
                .to_string()
                .contains("not registered")
        );
        let marker = temp.path().join("default").join("ran");
        assert!(
            runtime
                .execute(
                    ctx.clone(),
                    ExecArgs {
                        command: format!("echo ran > {}", marker.display()),
                        ..Default::default()
                    },
                    HashMap::new(),
                )
                .await
                .is_err()
        );
        assert!(!marker.exists());
        grants.register(&project).await.unwrap();
        let project = project.canonicalize().unwrap();
        assert_eq!(
            runtime.cli_workspace(&ctx).await.unwrap(),
            Some(project.clone())
        );

        let output = runtime
            .execute(
                ctx,
                ExecArgs {
                    command: pwd_command().to_string(),
                    ..Default::default()
                },
                HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(output.workspace.as_deref(), project.to_str());
        assert_same_directory(output.stdout.as_deref(), &project);

        let raw_source_ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        let mut raw_source_meta = cli_meta(&project);
        raw_source_meta
            .extra
            .insert(keys::SOURCE.to_string(), json!(project.to_string_lossy()));
        raw_source_ctx.set_state(SessionRequestMeta::new(raw_source_meta));
        assert_eq!(
            runtime.cli_workspace(&raw_source_ctx).await.unwrap(),
            Some(project.clone())
        );

        let trailing_source_ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        let mut trailing_meta = cli_meta(&project);
        trailing_meta.extra.insert(
            keys::SOURCE.to_string(),
            json!(format!(
                "cli:{}{}",
                project.display(),
                std::path::MAIN_SEPARATOR
            )),
        );
        trailing_source_ctx.set_state(SessionRequestMeta::new(trailing_meta));
        assert_eq!(
            runtime.cli_workspace(&trailing_source_ctx).await.unwrap(),
            Some(project.clone())
        );

        let voice_ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        let mut voice_meta = cli_meta(&project);
        voice_meta.extra.insert(
            keys::SOURCE.to_string(),
            json!(format!("cli:voice:{}", project.display())),
        );
        voice_ctx.set_state(SessionRequestMeta::new(voice_meta));
        assert_eq!(
            runtime.cli_workspace(&voice_ctx).await.unwrap(),
            Some(project.clone())
        );

        grants.register(&second_project).await.unwrap();
        let second_ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        second_ctx.set_state(SessionRequestMeta::new(cli_meta(&second_project)));
        let second_output = runtime
            .execute(
                second_ctx,
                ExecArgs {
                    command: pwd_command().to_string(),
                    ..Default::default()
                },
                HashMap::new(),
            )
            .await
            .unwrap();
        let second_project = second_project.canonicalize().unwrap();
        assert_eq!(second_output.workspace.as_deref(), second_project.to_str());
        assert_same_directory(second_output.stdout.as_deref(), &second_project);
    }

    #[tokio::test]
    async fn desktop_workspace_requires_owner_and_explicit_registration() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("desktop-project");
        tokio::fs::create_dir_all(&project).await.unwrap();
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        let mut meta = cli_meta(&project);
        meta.extra
            .insert(keys::SOURCE.to_string(), json!("desktop:chat-1"));
        assert!(
            grants
                .authorize_cron_workspace(&owner, &meta)
                .await
                .is_err()
        );
        grants.register(&project).await.unwrap();
        assert_eq!(
            grants
                .authorize_cron_workspace(&owner, &meta)
                .await
                .unwrap(),
            Some(project.canonicalize().unwrap())
        );
        assert!(
            grants
                .authorize_cron_workspace(&Principal::anonymous(), &meta)
                .await
                .is_err()
        );
        meta.extra
            .insert(keys::EXTERNAL_USER.to_string(), json!(true));
        assert!(
            grants
                .authorize_cron_workspace(&owner, &meta)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn forged_cli_metadata_cannot_use_another_caller_or_unregistered_path() {
        let temp = tempfile::tempdir().unwrap();
        let default = temp.path().join("default");
        let project = temp.path().join("project");
        let unregistered = temp.path().join("unregistered");
        tokio::fs::create_dir_all(&default).await.unwrap();
        tokio::fs::create_dir_all(&project).await.unwrap();
        tokio::fs::create_dir_all(&unregistered).await.unwrap();
        let grants = CliWorkspaceGrants::new(Principal::management_canister());
        grants.register(&project).await.unwrap();
        let runtime = NativeShellRuntime::new(default).with_cli_workspaces(grants);
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        ctx.set_state(SessionRequestMeta::new(cli_meta(&project)));
        assert!(
            runtime
                .cli_workspace(&ctx)
                .await
                .unwrap_err()
                .to_string()
                .contains("only the local owner")
        );

        let owner_grants = CliWorkspaceGrants::new(Principal::management_canister());
        owner_grants.register(&project).await.unwrap();
        let runtime =
            NativeShellRuntime::new(temp.path().join("default")).with_cli_workspaces(owner_grants);
        let owner_ctx = ctx.with_caller(Principal::management_canister());
        owner_ctx.set_state(SessionRequestMeta::new(cli_meta(&unregistered)));
        assert!(
            runtime
                .cli_workspace(&owner_ctx)
                .await
                .unwrap_err()
                .to_string()
                .contains("not registered")
        );
    }

    #[tokio::test]
    async fn other_sources_keep_the_configured_workspace_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let default = temp.path().join("default");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&default).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        let grants = CliWorkspaceGrants::new(Principal::management_canister());
        grants.register(&outside).await.unwrap();
        let runtime = NativeShellRuntime::new(default.clone()).with_cli_workspaces(grants);
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let mut meta = cli_meta(&outside);
        meta.extra
            .insert(keys::SOURCE.to_string(), json!("telegram"));
        ctx.set_state(SessionRequestMeta::new(meta));
        let output = runtime
            .execute(
                ctx,
                ExecArgs {
                    command: pwd_command().to_string(),
                    ..Default::default()
                },
                HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(output.workspace.as_deref(), default.to_str());
        assert_same_directory(output.stdout.as_deref(), &default);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn enriched_path_keeps_existing_entries_and_adds_tool_dirs() {
        let path = enriched_path_value(
            Some("/usr/bin:/bin"),
            [
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/Users/example/.cargo/bin"),
            ],
        )
        .expect("path should join");

        let paths = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(paths[0], PathBuf::from("/usr/bin"));
        assert_eq!(paths[1], PathBuf::from("/bin"));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(paths.contains(&PathBuf::from("/Users/example/.cargo/bin")));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_path() == std::path::Path::new("/usr/bin"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn cron_workspace_grant_survives_restart_but_cannot_be_forged_or_reused_by_another_caller()
     {
        let temp = tempfile::tempdir().unwrap();
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        let path = grants.register(temp.path()).await.unwrap();
        let meta = cli_meta(&path);
        let authorized = grants
            .authorize_cron_workspace(&owner, &meta)
            .await
            .unwrap()
            .unwrap();
        let runtime = NativeShellRuntime::new(path.clone())
            .with_cli_workspaces(CliWorkspaceGrants::new(owner));
        let ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(owner);
        let session = SessionRequestMeta::new(meta);
        ctx.set_state(session.clone());
        assert!(runtime.cli_workspace(&ctx).await.is_err());
        let grant = crate::cron::CronWorkspaceGrant {
            caller: owner,
            path: authorized.clone(),
        };
        session.set_cron_workspace(Some(grant.clone()));
        assert_eq!(runtime.cli_workspace(&ctx).await.unwrap(), Some(authorized));
        assert!(
            runtime
                .cli_workspace(&ctx.with_caller(Principal::anonymous()))
                .await
                .is_err()
        );
        // A normal follow-up must not recover the original frozen cron grant.
        ctx.set_state(grant);
        session.set_cron_workspace(None);
        assert!(runtime.cli_workspace(&ctx).await.is_err());
    }
}
