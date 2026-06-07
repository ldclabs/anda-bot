#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) fn suppress_console_window(command: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn suppress_tokio_console_window(command: &mut tokio::process::Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}
