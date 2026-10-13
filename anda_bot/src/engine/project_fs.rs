//! `read_file` and `search_file` in the project directory a chat runs in.
//!
//! `anda_engine`'s workspace file tools reach only the roots they were built
//! with: request metadata may narrow those roots but never add one. So in a
//! CLI or desktop chat the shell ran in the project while these tools could
//! not see it. The project is granted out of band ([`CliWorkspaceGrants`]);
//! the wrapper resolves that grant per call, as the shell does, and derives
//! the tool with the project as its first root.
//!
//! The project is opened no wider than the shell would read it unasked: not
//! at all under request_approval, and on_risk refuses secret-like files. The
//! edit and write tools keep their own roots, since they have no approval
//! card and on_risk asks before the shell writes to a project.

use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolGroupInfo, ToolOutput};
use anda_engine::{
    context::BaseCtx,
    extension::fs::{ReadFileTool, SearchFileTool},
};
use std::path::{Path, PathBuf};

use super::{
    action::{ApprovalMode, approval_scope, live_request_meta, references_sensitive_path},
    shell_runtime::{CliWorkspaceGrants, call_workspace},
};
use crate::util::request_meta::keys;

/// A workspace file tool that can be rebuilt over other roots.
pub(crate) trait RootedFsTool: Tool<BaseCtx> + Sized {
    /// Appended to the description: what the tool does in a project chat.
    const PROJECT_NOTE: &'static str;

    fn with_roots(roots: Vec<PathBuf>) -> Self;

    /// The file whose content the call returns to the model, if any.
    fn read_path(_args: &Self::Args) -> Option<&str> {
        None
    }
}

impl RootedFsTool for ReadFileTool {
    const PROJECT_NOTE: &'static str = "In a chat opened on a project directory (anda CLI or Anda Desktop), it also reads that directory, ahead of these: relative paths resolve there first. Files there that look like secrets (.env, keys, tokens) may be refused; the shell can read them after asking the user. edit_file, write_file and apply_patch do not reach the project: change files there with the shell.";

    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self::with_workspaces(roots)
    }

    fn read_path(args: &Self::Args) -> Option<&str> {
        Some(&args.path)
    }
}

impl RootedFsTool for SearchFileTool {
    const PROJECT_NOTE: &'static str = "In a chat opened on a project directory (anda CLI or Anda Desktop), it also searches that directory, ahead of these.";

    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self::with_workspaces(roots)
    }
}

/// `T` over the configured roots, plus the chat's registered project.
pub(crate) struct ProjectFsTool<T> {
    tool: T,
    roots: Vec<PathBuf>,
    grants: CliWorkspaceGrants,
    description: String,
}

impl<T: RootedFsTool> ProjectFsTool<T> {
    pub(crate) fn new(roots: Vec<PathBuf>, grants: CliWorkspaceGrants) -> Self {
        let tool = T::with_roots(roots.clone());
        let description = format!("{} {}", tool.description(), T::PROJECT_NOTE);
        Self {
            tool,
            roots,
            grants,
            description,
        }
    }
}

impl<T: RootedFsTool> Tool<BaseCtx> for ProjectFsTool<T> {
    type Args = T::Args;
    type Output = T::Output;

    fn name(&self) -> String {
        self.tool.name()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            description: self.description.clone(),
            ..self.tool.definition()
        }
    }

    fn group(&self) -> Option<ToolGroupInfo> {
        self.tool.group()
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let Some(project) = call_workspace(Some(&self.grants), &ctx).await? else {
            return self.tool.call(ctx, args, resources).await;
        };
        match approval_scope(&ctx).mode {
            ApprovalMode::RequestApproval => {
                let name = self.tool.name();
                return self.tool.call(ctx, args, resources).await.map_err(|err| {
                    BoxError::from(format!(
                        "{err}. This chat asks before every tool call, so {name} does not open its project directory {}; read files there with the shell, which asks the user first",
                        project.display()
                    ))
                });
            }
            ApprovalMode::OnRisk | ApprovalMode::Custom => {
                if let Some(path) = T::read_path(&args) {
                    refuse_secret(&project, path).await?;
                }
            }
            ApprovalMode::FullAccess => {}
        }
        // An absolute path counts as inside a root only when it is spelled
        // under that root, so also root the project as the request names it
        // (through a symlink, say): the model sees and uses that spelling.
        let named = live_request_meta(&ctx).get_extra_as::<PathBuf>(keys::WORKSPACE);
        let roots = [Some(project), named]
            .into_iter()
            .flatten()
            .chain(self.roots.iter().cloned())
            .collect();
        T::with_roots(roots).call(ctx, args, resources).await
    }
}

/// Refuses a secret-like file inside the project, which on_risk would ask
/// about before the shell read it. The check follows symlinks; a path that
/// does not resolve is left to the tool, which reports it or reads it from
/// another root.
async fn refuse_secret(project: &Path, path: &str) -> Result<(), BoxError> {
    let Ok(resolved) = tokio::fs::canonicalize(project.join(path)).await else {
        return Ok(());
    };
    let Ok(inside) = resolved.strip_prefix(project) else {
        return Ok(());
    };
    if references_sensitive_path(&inside.to_string_lossy()) {
        return Err(format!(
            "{path} in the project directory looks like a secret (.env, keys, tokens, credentials), and this chat asks before reading those. If the task needs it, read it with the shell, which asks the user first"
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::agent::SessionRequestMeta;
    use anda_core::{Principal, RequestMeta};
    use anda_engine::extension::fs::{ReadFileArgs, SearchFileArgs};
    use serde_json::json;

    struct Dirs {
        _temp: tempfile::TempDir,
        default: PathBuf,
        project: PathBuf,
    }

    /// A configured root holding `notes.md`, and a project outside it holding
    /// `CHANGELOG.md` and `.env`.
    async fn dirs() -> Dirs {
        let temp = tempfile::tempdir().unwrap();
        let default = temp.path().join("default");
        let project = temp.path().join("project");
        tokio::fs::create_dir_all(&default).await.unwrap();
        tokio::fs::create_dir_all(&project).await.unwrap();
        tokio::fs::write(default.join("notes.md"), "notes")
            .await
            .unwrap();
        tokio::fs::write(project.join("CHANGELOG.md"), "# Changelog")
            .await
            .unwrap();
        tokio::fs::write(project.join(".env"), "API_KEY=1")
            .await
            .unwrap();
        Dirs {
            _temp: temp,
            default,
            project,
        }
    }

    fn request_ctx(
        caller: Principal,
        source: String,
        workspace: &Path,
        mode: Option<&str>,
    ) -> BaseCtx {
        let mut meta = RequestMeta::default();
        meta.extra.insert(keys::SOURCE.to_string(), json!(source));
        meta.extra.insert(
            keys::WORKSPACE.to_string(),
            json!(workspace.to_string_lossy()),
        );
        if let Some(mode) = mode {
            meta.extra
                .insert(keys::APPROVAL_MODE.to_string(), json!(mode));
        }
        let ctx = anda_engine::engine::EngineBuilder::new()
            .mock_ctx()
            .base
            .with_caller(caller);
        ctx.set_state(SessionRequestMeta::new(meta));
        ctx
    }

    fn cli_ctx(caller: Principal, project: &Path, mode: Option<&str>) -> BaseCtx {
        request_ctx(caller, format!("cli:{}", project.display()), project, mode)
    }

    async fn read(
        tool: &ProjectFsTool<ReadFileTool>,
        ctx: &BaseCtx,
        path: &str,
    ) -> Result<String, BoxError> {
        let args = ReadFileArgs {
            path: path.to_string(),
            ..Default::default()
        };
        Ok(tool
            .call(ctx.clone(), args, Vec::new())
            .await?
            .output
            .content)
    }

    #[tokio::test]
    async fn read_file_reaches_only_a_registered_project() {
        let dirs = dirs().await;
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        let tool = ProjectFsTool::<ReadFileTool>::new(vec![dirs.default.clone()], grants.clone());
        let ctx = cli_ctx(owner, &dirs.project, None);

        let err = read(&tool, &ctx, "CHANGELOG.md").await.unwrap_err();
        assert!(err.to_string().contains("not registered"), "{err}");

        grants.register(&dirs.project).await.unwrap();
        assert_eq!(
            read(&tool, &ctx, "CHANGELOG.md").await.unwrap(),
            "# Changelog"
        );
        let absolute = dirs.project.join("CHANGELOG.md");
        assert_eq!(
            read(&tool, &ctx, &absolute.to_string_lossy())
                .await
                .unwrap(),
            "# Changelog"
        );
        // The configured roots stay reachable behind the project.
        assert_eq!(read(&tool, &ctx, "notes.md").await.unwrap(), "notes");

        // Another caller cannot use the owner's registration.
        let stranger = cli_ctx(Principal::anonymous(), &dirs.project, None);
        let err = read(&tool, &stranger, "CHANGELOG.md").await.unwrap_err();
        assert!(err.to_string().contains("only the local owner"), "{err}");

        // Other sources keep the configured roots, whatever workspace they name.
        let im = request_ctx(owner, "telegram".to_string(), &dirs.project, None);
        assert!(read(&tool, &im, "CHANGELOG.md").await.is_err());
        assert_eq!(read(&tool, &im, "notes.md").await.unwrap(), "notes");
    }

    #[tokio::test]
    async fn project_reads_follow_the_approval_mode() {
        let dirs = dirs().await;
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        grants.register(&dirs.project).await.unwrap();
        let tool = ProjectFsTool::<ReadFileTool>::new(vec![dirs.default.clone()], grants);

        let on_risk = cli_ctx(owner, &dirs.project, None);
        for path in [".env".into(), dirs.project.join(".env").to_string_lossy()] {
            let err = read(&tool, &on_risk, &path).await.unwrap_err();
            assert!(err.to_string().contains("looks like a secret"), "{err}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dirs.project.join(".env"), dirs.project.join("config"))
                .unwrap();
            let err = read(&tool, &on_risk, "config").await.unwrap_err();
            assert!(err.to_string().contains("looks like a secret"), "{err}");
        }

        let full_access = cli_ctx(owner, &dirs.project, Some("full_access"));
        assert_eq!(
            read(&tool, &full_access, ".env").await.unwrap(),
            "API_KEY=1"
        );

        let ask_always = cli_ctx(owner, &dirs.project, Some("request_approval"));
        let err = read(&tool, &ask_always, "CHANGELOG.md").await.unwrap_err();
        assert!(
            err.to_string().contains("asks before every tool call"),
            "{err}"
        );
        assert_eq!(read(&tool, &ask_always, "notes.md").await.unwrap(), "notes");
    }

    #[tokio::test]
    async fn search_file_lists_the_project_first() {
        let dirs = dirs().await;
        let owner = Principal::management_canister();
        let grants = CliWorkspaceGrants::new(owner);
        grants.register(&dirs.project).await.unwrap();
        let tool = ProjectFsTool::<SearchFileTool>::new(vec![dirs.default.clone()], grants);
        assert!(tool.description().ends_with(SearchFileTool::PROJECT_NOTE));
        assert_eq!(
            tool.definition().description,
            tool.description(),
            "the model sees the note"
        );

        let args = SearchFileArgs {
            pattern: "*.md".to_string(),
            ..Default::default()
        };
        let found = tool
            .call(cli_ctx(owner, &dirs.project, None), args, Vec::new())
            .await
            .unwrap()
            .output
            .paths;
        assert_eq!(found, vec!["CHANGELOG.md", "notes.md"]);
    }
}
