use crate::core::{LauncherContext, LauncherResult, text};

pub fn run(_ctx: LauncherContext) -> LauncherResult<()> {
    Err(text().unsupported_platform.into())
}

pub fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}

pub fn activate_running_instance() {}
