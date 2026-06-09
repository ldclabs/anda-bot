use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
};

use objc2::{
    AnyThread, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSApplication, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar,
    NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSObjectProtocol, NSSize, NSString};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult, text},
    settings,
};

const LAUNCH_AGENT_LABEL: &str = "ai.anda.anda-bot.launcher";
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo-tray.png");
const LAUNCHER_APP_ICON_ICNS: &[u8] = include_bytes!("../../../assets/logo.icns");
const LAUNCHER_APP_NAME: &str = "Anda Bot.app";
const LAUNCHER_APP_EXECUTABLE: &str = "Anda Bot";
const LAUNCHER_APP_ICON: &str = "AndaBot";
const LAUNCHER_APP_ICON_FILE: &str = "AndaBot.icns";
const CHECK_UPDATE_MENU_TAG: isize = 1009;
const STATUS_PID_MENU_TAG: isize = 1012;
const STATUS_GATEWAY_MENU_TAG: isize = 1013;

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

    unsafe impl NSMenuDelegate for Delegate {}

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
                        &text().app_title,
                        &core::restart_daemon(ctx).unwrap_or_else(error_result),
                    ),
                    Ok(false) => {}
                    Err(err) => show_error(&text().settings_title, &err.to_string()),
                }
            }
        }

        #[unsafe(method(showStatus:))]
        fn show_status(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    &text().app_title,
                    &core::daemon_status(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(restartDaemon:))]
        fn restart_daemon(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_result(
                    &text().app_title,
                    &core::restart_daemon(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(generateBrowserExtensionToken:))]
        fn generate_browser_extension_token(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_browser_extension_token_result(
                    &core::generate_browser_extension_token(ctx).unwrap_or_else(error_result),
                );
            }
        }

        #[unsafe(method(checkUpdate:))]
        fn check_update(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                run_manual_update_check(ctx.clone());
            }
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                match toggle_launch_at_login(ctx) {
                    Ok(message) => show_info(&text().app_title, &message),
                    Err(err) => show_error(&text().app_title, &err.to_string()),
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

        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            refresh_status_menu_items(menu);
            refresh_update_menu_item(menu);
        }

        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, menu: &NSMenu) {
            refresh_status_menu_items(menu);
            refresh_update_menu_item(menu);
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

    let mtm = MainThreadMarker::new().ok_or_else(|| text().main_thread_required)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let menu = build_menu(mtm, &delegate);
    let status_item =
        NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let status_button = status_item.button(mtm);
    let status_image = status_bar_icon();
    if let Some(button) = status_button.as_ref() {
        if let Some(image) = status_image.as_ref() {
            button.setImage(Some(image));
        } else {
            button.setTitle(nsstring("Anda").as_ref());
        }
    } else if let Some(image) = status_image.as_ref() {
        #[allow(deprecated)]
        status_item.setImage(Some(image));
    } else {
        #[allow(deprecated)]
        status_item.setTitle(Some(nsstring("Anda").as_ref()));
    }
    status_item.setMenu(Some(&menu));
    status_item.setVisible(true);

    let _keep_alive = (delegate, menu, status_item, status_button, status_image);
    start_startup_tasks(ctx.clone());
    app.run();
    Ok(())
}

pub fn show_error(title: &str, message: &str) {
    show_alert(title, message);
}

fn build_menu(mtm: MainThreadMarker, delegate: &Delegate) -> Retained<NSMenu> {
    let copy = text();
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), nsstring(&copy.app_title).as_ref());
    menu.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    add_item(&menu, mtm, &copy.open_anda, sel!(openAnda:), delegate);
    add_item(&menu, mtm, &copy.open_logs, sel!(openLogs:), delegate);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_disabled_item(&menu, mtm, &copy.status);
    let status = core::cached_daemon_status();
    let status_pid = add_disabled_item(&menu, mtm, &status_pid_title(&status));
    status_pid.setTag(STATUS_PID_MENU_TAG);
    let status_gateway = add_disabled_item(&menu, mtm, &status_gateway_title(&status));
    status_gateway.setTag(STATUS_GATEWAY_MENU_TAG);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        &menu,
        mtm,
        &copy.restart_daemon,
        sel!(restartDaemon:),
        delegate,
    );
    add_item(
        &menu,
        mtm,
        &copy.browser_extension_token,
        sel!(generateBrowserExtensionToken:),
        delegate,
    );
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_settings_submenu(&menu, mtm, delegate, &copy);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let check_update = add_item(
        &menu,
        mtm,
        &core::check_update_menu_label(),
        sel!(checkUpdate:),
        delegate,
    );
    check_update.setTag(CHECK_UPDATE_MENU_TAG);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(&menu, mtm, &copy.quit, sel!(quit:), delegate);
    menu
}

fn refresh_status_menu_items(menu: &NSMenu) {
    let status = core::cached_daemon_status();
    if let Some(item) = menu.itemWithTag(STATUS_PID_MENU_TAG) {
        item.setTitle(nsstring(&status_pid_title(&status)).as_ref());
    }
    if let Some(item) = menu.itemWithTag(STATUS_GATEWAY_MENU_TAG) {
        item.setTitle(nsstring(&status_gateway_title(&status)).as_ref());
    }
}

fn refresh_update_menu_item(menu: &NSMenu) {
    if let Some(item) = menu.itemWithTag(CHECK_UPDATE_MENU_TAG) {
        item.setTitle(nsstring(&core::check_update_menu_label()).as_ref());
    }
}

fn status_pid_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    format!(
        "{}: {}",
        copy.status_pid,
        status.pid.as_deref().unwrap_or(&copy.status_unavailable)
    )
}

fn status_gateway_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    format!(
        "{}: {}",
        copy.status_gateway_url,
        status
            .gateway_url
            .as_deref()
            .unwrap_or(&copy.status_unavailable)
    )
}

fn add_settings_submenu(
    menu: &NSMenu,
    mtm: MainThreadMarker,
    delegate: &Delegate,
    copy: &core::LauncherText,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            nsstring(&copy.settings).as_ref(),
            None,
            nsstring("").as_ref(),
        )
    };
    let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), nsstring(&copy.settings).as_ref());
    add_item(
        &submenu,
        mtm,
        &copy.model_settings,
        sel!(settings:),
        delegate,
    );
    let launch_title = if launch_agent_installed() {
        &copy.disable_launch_at_login
    } else {
        &copy.launch_at_login
    };
    add_item(
        &submenu,
        mtm,
        launch_title,
        sel!(toggleLaunchAtLogin:),
        delegate,
    );
    item.setSubmenu(Some(&submenu));
    menu.addItem(&item);
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
) -> Retained<NSMenuItem> {
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
    item
}

fn add_disabled_item(menu: &NSMenu, mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            nsstring(title).as_ref(),
            None,
            nsstring("").as_ref(),
        )
    };
    item.setEnabled(false);
    menu.addItem(&item);
    item
}

fn show_result(title: &str, result: &CommandResult) {
    show_alert(title, &result.message);
}

fn show_info(title: &str, message: &str) {
    show_alert(title, message);
}

fn show_browser_extension_token_result(result: &CommandResult) {
    if result.success && copy_to_clipboard(&result.message).is_ok() {
        let token = core::browser_extension_bearer_token(&result.message);
        let copied = CommandResult {
            success: result.success,
            message: format!(
                "{}\n\n{}",
                text().browser_extension_token_copied,
                result.message
            ),
        };
        show_browser_extension_token_alert(&copied.message, token.as_deref());
    } else {
        show_result(&text().browser_extension_token_title, result);
    }
}

fn show_browser_extension_token_alert(message: &str, token: Option<&str>) {
    if let Some(mtm) = MainThreadMarker::new() {
        let alert = NSAlert::init(NSAlert::alloc(mtm));
        alert.setMessageText(nsstring(&text().browser_extension_token_title).as_ref());
        alert.setInformativeText(nsstring(message).as_ref());
        alert.addButtonWithTitle(nsstring(&text().ok).as_ref());
        if token.is_some() {
            alert
                .addButtonWithTitle(nsstring(&text().browser_extension_token_copy_button).as_ref());
        }

        let response = alert.runModal();
        if response == NSAlertSecondButtonReturn
            && let Some(token) = token
        {
            if let Err(err) = copy_to_clipboard(token) {
                show_error(&text().browser_extension_token_title, &err.to_string());
            } else {
                show_info(
                    &text().browser_extension_token_title,
                    &text().browser_extension_token_only_copied,
                );
            }
        }
    } else {
        eprintln!("{}: {message}", text().browser_extension_token_title);
    }
}

fn copy_to_clipboard(value: &str) -> LauncherResult<()> {
    let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().ok_or("failed to open clipboard stdin")?;
    stdin.write_all(value.as_bytes())?;
    drop(stdin);

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(text().command_exited(status).into())
    }
}

fn start_startup_tasks(ctx: LauncherContext) {
    thread::spawn(move || {
        if let Err(err) = ensure_application_entrypoint(&ctx) {
            eprintln!("failed to ensure macOS application entrypoint: {err}");
        }
        if let Err(err) = run_startup_setup(&ctx) {
            show_background_dialog(&text().app_title, &err.to_string());
        }
        start_status_loop(ctx.clone());
        start_auto_update_loop(ctx);
    });
}

fn run_startup_setup(ctx: &LauncherContext) -> LauncherResult<()> {
    if core::config_needs_setup(ctx) {
        if settings::run_initial_setup_wizard(ctx)? {
            let _ = core::start_daemon(ctx);
        }
    } else {
        let _ = core::start_daemon(ctx);
    }
    Ok(())
}

fn start_status_loop(ctx: LauncherContext) {
    thread::spawn(move || {
        loop {
            core::refresh_daemon_status_cache(&ctx);
            thread::sleep(core::daemon_status_poll_interval());
        }
    });
}

fn start_auto_update_loop(ctx: LauncherContext) {
    thread::spawn(move || {
        let mut prompted_tag: Option<String> = None;
        loop {
            match core::check_update_if_due(&ctx) {
                Ok(state) => {
                    core::record_update_state(&state);
                    if state.downloaded_update_available() {
                        let tag = state.latest_tag.clone();
                        if tag != prompted_tag {
                            prompted_tag = tag;
                            prompt_update_ready(ctx.clone(), state);
                        }
                    }
                }
                Err(err) => eprintln!("{}: {err}", text().update_check_failed_title),
            }
            thread::sleep(core::auto_update_poll_interval());
        }
    });
}

fn run_manual_update_check(ctx: LauncherContext) {
    if !core::begin_update_check() {
        show_background_notification(
            &text().update_check_result_title,
            &core::check_update_menu_label(),
        );
        return;
    }

    show_background_notification(
        &text().update_check_result_title,
        &core::check_update_menu_label(),
    );
    thread::spawn(move || match core::check_update_now(&ctx) {
        Ok(state) if state.downloaded_update_available() => {
            core::finish_update_check(Some(state.clone()));
            prompt_update_ready(ctx, state);
        }
        Ok(state) => {
            core::finish_update_check(Some(state.clone()));
            show_background_dialog(&text().update_check_result_title, &state.check_message());
        }
        Err(err) => {
            core::finish_update_check(None);
            show_background_dialog(
                &text().update_check_failed_title,
                &text().update_check_failed_message(&err.to_string()),
            );
        }
    });
}

fn prompt_update_ready(ctx: LauncherContext, state: core::LauncherAutoUpdateState) {
    let latest = state.latest_tag_label();
    if !confirm_update_restart(&latest) {
        return;
    }

    show_background_notification(&text().update_restart_title, &text().update_restart_started);
    let result = core::install_update_and_restart(&ctx).unwrap_or_else(error_result);
    if result.success {
        show_background_dialog(&text().update_restart_title, &result.message);
    } else {
        show_background_dialog(
            &text().update_restart_title,
            &text().update_restart_failed_message(&result.message),
        );
    }
}

fn confirm_update_restart(latest_tag: &str) -> bool {
    let script = format!(
        "display dialog {} with title {} buttons {{{}, {}}} default button {} cancel button {} with icon note",
        applescript_string(&text().update_restart_confirm(latest_tag)),
        applescript_string(&text().update_ready_title),
        applescript_string(&text().cancel),
        applescript_string(&text().install_restart_update),
        applescript_string(&text().install_restart_update),
        applescript_string(&text().cancel),
    );
    Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .is_ok_and(|status| status.success())
}

fn show_background_dialog(title: &str, message: &str) {
    let script = format!(
        "display dialog {} with title {} buttons {{{}}} default button {} with icon note",
        applescript_string(message),
        applescript_string(title),
        applescript_string(&text().ok),
        applescript_string(&text().ok),
    );
    if Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .is_err()
    {
        eprintln!("{title}: {message}");
    }
}

fn show_background_notification(title: &str, message: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(message),
        applescript_string(title),
    );
    let _ = Command::new("osascript").arg("-e").arg(script).status();
}

fn show_alert(title: &str, message: &str) {
    if let Some(mtm) = MainThreadMarker::new() {
        let alert = NSAlert::init(NSAlert::alloc(mtm));
        alert.setMessageText(nsstring(title).as_ref());
        alert.setInformativeText(nsstring(message).as_ref());
        alert.addButtonWithTitle(nsstring(&text().ok).as_ref());
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
        Ok(text().launch_at_login_disabled)
    } else {
        let path = launch_agent_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| text().resolve_launch_agents_failed)?;
        fs::create_dir_all(parent)?;
        fs::write(&path, launch_agent_plist(ctx))?;
        let _ = launchctl_bootout(&path);
        let _ = launchctl_bootstrap(&path);
        Ok(text().launch_at_login_enabled)
    }
}

fn launch_agent_installed() -> bool {
    launch_agent_path().is_ok_and(|path| path.exists())
}

fn ensure_application_entrypoint(ctx: &LauncherContext) -> LauncherResult<()> {
    let app_path = launcher_app_path()?;
    let contents = app_path.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources_dir = contents.join("Resources");
    let executable = macos_dir.join(LAUNCHER_APP_EXECUTABLE);

    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&resources_dir)?;
    fs::write(contents.join("Info.plist"), launcher_app_info_plist())?;
    fs::write(
        resources_dir.join(LAUNCHER_APP_ICON_FILE),
        launcher_icon_icns(),
    )?;
    fs::write(&executable, launcher_app_script(ctx))?;

    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(executable, permissions)?;
    let _ = Command::new("touch").arg(app_path).status();
    Ok(())
}

fn launcher_app_path() -> LauncherResult<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| text().detect_home_failed)?;
    Ok(home.join("Applications").join(LAUNCHER_APP_NAME))
}

fn launcher_app_info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>{executable}</string>
  <key>CFBundleIdentifier</key>
  <string>{label}</string>
  <key>CFBundleName</key>
  <string>Anda Bot</string>
  <key>CFBundleIconFile</key>
  <string>{icon}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
"#,
        executable = xml_escape(LAUNCHER_APP_EXECUTABLE),
        icon = xml_escape(LAUNCHER_APP_ICON),
        label = LAUNCH_AGENT_LABEL,
    )
}

fn launcher_icon_icns() -> Vec<u8> {
    LAUNCHER_APP_ICON_ICNS.to_vec()
}

fn launcher_app_script(ctx: &LauncherContext) -> String {
    let install_dir = ctx
        .launcher_exe
        .parent()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    format!(
        r#"#!/bin/sh
INSTALL_DIR={install_dir}
PATH="$INSTALL_DIR:${{HOME:-}}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
export PATH

for LAUNCHER in "$INSTALL_DIR/anda_launcher" "${{HOME:-}}/.local/bin/anda_launcher" "/opt/homebrew/bin/anda_launcher" "/usr/local/bin/anda_launcher"; do
  if [ -x "$LAUNCHER" ]; then
    export ANDA_LAUNCHER_EXE="$LAUNCHER"
    ANDA_CANDIDATE="$(dirname "$LAUNCHER")/anda"
    if [ -x "$ANDA_CANDIDATE" ]; then
      export ANDA_EXE="$ANDA_CANDIDATE"
    fi
    exec "$LAUNCHER" "$@"
  fi
done

osascript -e 'display dialog "Anda launcher could not be found. Reinstall Anda Bot." with title "Anda Bot" buttons {{"OK"}} default button "OK" with icon caution' >/dev/null 2>&1
exit 127
"#,
        install_dir = shell_single_quote(&install_dir),
    )
}

fn launch_agent_path() -> LauncherResult<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| text().detect_home_failed)?;
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

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
        assert!(!plist.contains("--home"));
        assert!(!plist.contains("/Users/me/.anda"));
    }

    #[test]
    fn launcher_icon_is_embedded_png() {
        assert!(LAUNCHER_ICON_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn launcher_app_script_execs_launcher_without_home_override() {
        let ctx = LauncherContext {
            launcher_exe: "/Users/me/bin/anda launcher".into(),
            anda_exe: "/Users/me/bin/anda".into(),
            home: "/Users/me/.anda-custom".into(),
        };

        let script = launcher_app_script(&ctx);
        assert!(script.contains("INSTALL_DIR='/Users/me/bin'"));
        assert!(script.contains("ANDA_LAUNCHER_EXE"));
        assert!(script.contains("ANDA_EXE"));
        assert!(script.contains("/opt/homebrew/bin/anda_launcher"));
        assert!(!script.contains("--home"));
    }

    #[test]
    fn launcher_app_info_plist_references_icon() {
        let plist = launcher_app_info_plist();

        assert!(plist.contains("<key>CFBundleIconFile</key>"));
        assert!(plist.contains("<string>AndaBot</string>"));
    }

    #[test]
    fn launcher_icon_icns_uses_embedded_asset() {
        let icon = launcher_icon_icns();

        assert_eq!(&icon[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icon[4..8].try_into().unwrap()) as usize,
            icon.len()
        );
        assert_eq!(icon, LAUNCHER_APP_ICON_ICNS);
    }

    #[test]
    fn shell_single_quote_handles_apostrophes() {
        assert_eq!(
            shell_single_quote("/tmp/a b/it's/anda_launcher"),
            "'/tmp/a b/it'\\''s/anda_launcher'"
        );
    }
}
