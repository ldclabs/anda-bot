use std::{fs, process::Command, sync::OnceLock};

use objc2::{
    AnyThread, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSImage, NSMenu,
    NSMenuItem, NSStatusBar,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSObjectProtocol, NSSize, NSString};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult, text},
    settings,
};

const LAUNCH_AGENT_LABEL: &str = "ai.anda.anda-bot.launcher";
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo-tray.png");

static CTX: OnceLock<LauncherContext> = OnceLock::new();

#[derive(Debug, Default)]
struct DelegateIvars;

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {}

    impl Delegate {
        #[unsafe(method(openAnda:))]
        fn open_anda(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                open_anda_terminal(ctx);
            }
        }

        #[unsafe(method(settings:))]
        fn settings(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                match settings::run_wizard(ctx) {
                    Ok(true) => show_result(
                        text().app_title,
                        &core::restart_daemon(ctx).unwrap_or_else(error_result),
                    ),
                    Ok(false) => {}
                    Err(err) => show_error(text().settings_title, &err.to_string()),
                }
            }
        }

        #[unsafe(method(startDaemon:))]
        fn start_daemon(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    text().app_title,
                    &core::start_daemon(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(showStatus:))]
        fn show_status(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    text().app_title,
                    &core::daemon_status(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(stopDaemon:))]
        fn stop_daemon(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    text().app_title,
                    &core::stop_daemon(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(restartDaemon:))]
        fn restart_daemon(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    text().app_title,
                    &core::restart_daemon(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                match toggle_launch_at_login(ctx) {
                    Ok(message) => show_info(text().app_title, &message),
                    Err(err) => show_error(text().app_title, &err.to_string()),
                }
            }
        }

        #[unsafe(method(openLogs:))]
        fn open_logs(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                open_path(&ctx.logs_dir());
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars);
        unsafe { msg_send![super(this), init] }
    }
}

pub fn run(ctx: LauncherContext) -> LauncherResult<()> {
    CTX.set(ctx.clone()).ok();

    if core::config_needs_setup(&ctx) {
        if settings::run_initial_setup_wizard(&ctx)? {
            let _ = core::start_daemon(&ctx);
        }
    } else {
        let _ = core::start_daemon(&ctx);
    }

    let mtm = MainThreadMarker::new().ok_or_else(|| text().main_thread_required.to_string())?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let menu = build_menu(mtm, &delegate);
    let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(32.0);
    if let Some(image) = status_bar_icon() {
        #[allow(deprecated)]
        status_item.setImage(Some(&image));
    } else {
        #[allow(deprecated)]
        status_item.setTitle(Some(nsstring("Anda").as_ref()));
    }
    status_item.setMenu(Some(&menu));

    let _keep_alive = (delegate, menu, status_item);
    app.run();
    Ok(())
}

pub fn show_error(title: &str, message: &str) {
    show_alert(title, message);
}

fn build_menu(mtm: MainThreadMarker, delegate: &Delegate) -> Retained<NSMenu> {
    let copy = text();
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), nsstring(copy.app_title).as_ref());
    add_item(&menu, mtm, copy.open_anda, sel!(openAnda:), delegate);
    add_item(&menu, mtm, copy.settings, sel!(settings:), delegate);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(&menu, mtm, copy.status, sel!(showStatus:), delegate);
    add_item(&menu, mtm, copy.start_daemon, sel!(startDaemon:), delegate);
    add_item(&menu, mtm, copy.stop_daemon, sel!(stopDaemon:), delegate);
    add_item(
        &menu,
        mtm,
        copy.restart_daemon,
        sel!(restartDaemon:),
        delegate,
    );
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let launch_title = if launch_agent_installed() {
        copy.disable_launch_at_login
    } else {
        copy.launch_at_login
    };
    add_item(
        &menu,
        mtm,
        launch_title,
        sel!(toggleLaunchAtLogin:),
        delegate,
    );
    add_item(&menu, mtm, copy.open_logs, sel!(openLogs:), delegate);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(&menu, mtm, copy.quit, sel!(quit:), delegate);
    menu
}

fn status_bar_icon() -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(LAUNCHER_ICON_PNG);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setSize(NSSize::new(18.0, 18.0));
    image.setTemplate(true);
    Some(image)
}

fn add_item(
    menu: &NSMenu,
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    delegate: &Delegate,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            nsstring(title).as_ref(),
            Some(action),
            nsstring("").as_ref(),
        )
    };
    unsafe {
        item.setTarget(Some(delegate.as_ref()));
    }
    menu.addItem(&item);
}

fn show_result(title: &str, result: &CommandResult) {
    show_alert(title, &result.message);
}

fn show_info(title: &str, message: &str) {
    show_alert(title, message);
}

fn show_alert(title: &str, message: &str) {
    if let Some(mtm) = MainThreadMarker::new() {
        let alert = NSAlert::init(NSAlert::alloc(mtm));
        alert.setMessageText(nsstring(title).as_ref());
        alert.setInformativeText(nsstring(message).as_ref());
        alert.addButtonWithTitle(nsstring(text().ok).as_ref());
        alert.runModal();
    } else {
        eprintln!("{title}: {message}");
    }
}

fn error_result(err: Box<dyn std::error::Error + Send + Sync>) -> CommandResult {
    CommandResult {
        success: false,
        message: err.to_string(),
    }
}

fn toggle_launch_at_login(ctx: &LauncherContext) -> LauncherResult<String> {
    if launch_agent_installed() {
        let path = launch_agent_path()?;
        let _ = launchctl_bootout(&path);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        Ok(text().launch_at_login_disabled.to_string())
    } else {
        let path = launch_agent_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| text().resolve_launch_agents_failed.to_string())?;
        fs::create_dir_all(parent)?;
        fs::write(&path, launch_agent_plist(ctx))?;
        let _ = launchctl_bootout(&path);
        let _ = launchctl_bootstrap(&path);
        Ok(text().launch_at_login_enabled.to_string())
    }
}

fn launch_agent_installed() -> bool {
    launch_agent_path().is_ok_and(|path| path.exists())
}

fn launch_agent_path() -> LauncherResult<std::path::PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| text().detect_home_failed.to_string())?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

fn launch_agent_plist(ctx: &LauncherContext) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{launcher}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        launcher = xml_escape(&ctx.launcher_exe.to_string_lossy()),
    )
}

fn launchctl_bootstrap(path: &std::path::Path) -> LauncherResult<()> {
    run_command(
        Command::new("launchctl")
            .arg("bootstrap")
            .arg(format!("gui/{}", unsafe { libc::geteuid() }))
            .arg(path),
    )
}

fn launchctl_bootout(path: &std::path::Path) -> LauncherResult<()> {
    run_command(
        Command::new("launchctl")
            .arg("bootout")
            .arg(format!("gui/{}", unsafe { libc::geteuid() }))
            .arg(path),
    )
}

fn run_command(command: &mut Command) -> LauncherResult<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    })
    .trim()
    .to_string();
    Err(text().command_failed(&detail).into())
}

fn open_anda_terminal(ctx: &LauncherContext) {
    let command = format!(
        "tell application \"Terminal\" to do script {}",
        applescript_string(&format!(
            "\"{}\" --home \"{}\"",
            ctx.anda_exe.display(),
            ctx.home.display()
        ))
    );
    let _ = Command::new("osascript").arg("-e").arg(command).spawn();
}

fn open_path(path: &std::path::Path) {
    let _ = fs::create_dir_all(path);
    let _ = Command::new("open").arg(path).spawn();
}

fn nsstring(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_agent_plist_escapes_launcher_path() {
        let ctx = LauncherContext {
            launcher_exe: "/Applications/Anda & Bot/anda_launcher".into(),
            anda_exe: "/Applications/Anda Bot/anda".into(),
            home: "/Users/me/.anda".into(),
        };

        let plist = launch_agent_plist(&ctx);
        assert!(plist.contains("/Applications/Anda &amp; Bot/anda_launcher"));
        assert!(plist.contains(LAUNCH_AGENT_LABEL));
    }

    #[test]
    fn launcher_icon_is_embedded_png() {
        assert!(LAUNCHER_ICON_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
