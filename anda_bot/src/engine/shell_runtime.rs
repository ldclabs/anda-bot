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
        envs: HashMap<String, String>,
    ) -> Result<ExecOutput, BoxError> {
        let mut command = NativeRuntime::build_shell_command(&input.command);
        suppress_console_window(&mut command);
        self.inner
            .execute_command(ctx, self.name(), command, envs, Some(input))
            .await
    }
}
