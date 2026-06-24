use anda_core::BoxError;
use anda_engine::{
    context::BaseCtx,
    extension::shell::{ExecArgs, ExecOutput, Executor, NativeRuntime},
};
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf};

use crate::util::windows_process::suppress_console_window;

pub struct NativeShellRuntime {
    inner: NativeRuntime,
}

impl NativeShellRuntime {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            inner: NativeRuntime::new(workspace),
        }
    }

    pub fn insecure(self) -> Self {
        Self {
            inner: self.inner.insecure(),
        }
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
        augment_command_path(&mut envs);
        let mut command = NativeRuntime::build_shell_command(&input.command);
        suppress_console_window(&mut command);
        self.inner
            .execute_command(ctx, self.name(), command, envs, Some(input))
            .await
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
}
