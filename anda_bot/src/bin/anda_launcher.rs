#![cfg_attr(windows, windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

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
    let result = run();
    if let Err(err) = result {
        platform::show_error(&core::text().launcher_title, &err.to_string());
        std::process::exit(1);
    }
}

fn run() -> core::LauncherResult<()> {
    let ctx = core::LauncherContext::detect()?;
    let Some(_lock) = core::acquire_launcher_instance_lock()? else {
        return Ok(());
    };
    platform::run(ctx)
}
