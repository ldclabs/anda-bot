use crate::core::{LauncherContext, LauncherResult};

pub fn run(_ctx: LauncherContext) -> LauncherResult<()> {
    Err("Anda Launcher currently supports Windows and macOS.".into())
}

pub fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}
