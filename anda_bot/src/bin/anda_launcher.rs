#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "anda_launcher/core.rs"]
mod core;
#[path = "anda_launcher/settings.rs"]
mod settings;

#[cfg(target_os = "macos")]
#[path = "anda_launcher/macos.rs"]
mod platform;

#[cfg(windows)]
#[path = "anda_launcher/windows.rs"]
mod platform;

#[cfg(not(any(target_os = "macos", windows)))]
#[path = "anda_launcher/unsupported.rs"]
mod platform;

fn main() {
    let result = core::LauncherContext::detect().and_then(platform::run);
    if let Err(err) = result {
        platform::show_error("Anda Launcher", &err.to_string());
        std::process::exit(1);
    }
}
