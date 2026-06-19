use std::{
    cell::RefCell,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
};

use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSControlStateValueOn, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar,
    NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSDistributedNotificationCenter, NSNotification,
    NSNotificationSuspensionBehavior, NSObject, NSObjectProtocol, NSSize, NSString,
};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult, text},
    settings,
};

const LAUNCH_AGENT_LABEL: &str = "ai.anda.anda-bot.launcher";
// Posted by a second launch so the already-running instance can rebuild a
// status-bar icon that the system dropped from the menu bar.
const REACTIVATE_NOTIFICATION: &str = "ai.anda.anda-bot.launcher.reactivate";
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo-tray.png");
const LAUNCHER_APP_ICON_ICNS: &[u8] = include_bytes!("../../../assets/logo.icns");
const LAUNCHER_APP_NAME: &str = "Anda Bot.app";
const LAUNCHER_APP_EXECUTABLE: &str = "Anda Bot";
const LAUNCHER_APP_ICON: &str = "AndaBot";
const LAUNCHER_APP_ICON_FILE: &str = "AndaBot.icns";
const CHECK_UPDATE_MENU_TAG: isize = 1009;
const STATUS_PID_MENU_TAG: isize = 1012;
const STATUS_GATEWAY_MENU_TAG: isize = 1013;
const STATUS_CONVERSATIONS_MENU_TAG: isize = 1014;
const STATUS_MEMORY_NODES_MENU_TAG: isize = 1015;
const STATUS_MEMORY_LINKS_MENU_TAG: isize = 1016;
const LANGUAGE_MENU_TAG_BASE: isize = 1100;

static CTX: OnceLock<LauncherContext> = OnceLock::new();

#[derive(Debug, Default)]
struct DelegateIvars {
    status_bar: RefCell<Option<StatusBarState>>,
}

#[derive(Debug)]
struct StatusBarState {
    _menu: Retained<NSMenu>,
    _status_item: Retained<NSStatusItem>,
    _status_button: Option<Retained<NSStatusBarButton>>,
    _status_image: Option<Retained<NSImage>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.install_status_bar();
            self.register_reactivation_observer();
        }
    }

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
                run_settings_wizard_async(ctx.clone());
            }
        }

        #[unsafe(method(restartDaemon:))]
        fn restart_daemon(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                restart_daemon_async(ctx.clone());
            }
        }

        #[unsafe(method(reloadModels:))]
        fn reload_models(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                reload_models_async(ctx.clone());
            }
        }

        #[unsafe(method(generateBrowserExtensionToken:))]
        fn generate_browser_extension_token(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                show_browser_extension_token_result_async(ctx.clone());
            }
        }

        #[unsafe(method(checkUpdate:))]
        fn check_update(&self, _sender: &AnyObject) {
            if let Some(ctx) = CTX.get() {
                run_manual_update_check(ctx.clone());
            }
        }

        #[unsafe(method(selectLanguage:))]
        fn select_language(&self, sender: &AnyObject) {
            let Some(item) = sender.downcast_ref::<NSMenuItem>() else {
                return;
            };
            let Some(language) = usize::try_from(item.tag() - LANGUAGE_MENU_TAG_BASE)
                .ok()
                .and_then(|index| core::LauncherLanguage::ALL.get(index))
                .copied()
            else {
                return;
            };
            let Some(ctx) = CTX.get() else {
                return;
            };

            if let Err(err) = core::set_launcher_language(&ctx.home, language) {
                show_error(&text().app_title, &err.to_string());
                return;
            }
            self.rebuild_menu();
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

        // Triggered by a second launch (see `activate_running_instance`) when
        // the menu bar dropped our status item but this process is still alive.
        #[unsafe(method(reactivateStatusBar:))]
        fn reactivate_status_bar(&self, _notification: &NSNotification) {
            self.replace_status_bar();
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn install_status_bar(&self) {
        if self.ivars().status_bar.borrow().is_some() {
            return;
        }
        self.replace_status_bar();
    }

    // Tears down any existing status item and vends a fresh one. Used for the
    // initial install and to recover when macOS drops the item from the menu
    // bar (e.g. after a menu-bar rebuild), which leaves the launcher running
    // but invisible. Recreating — rather than toggling visibility on the stale
    // item — is what reliably brings the icon back.
    fn replace_status_bar(&self) {
        let status_bar = NSStatusBar::systemStatusBar();
        if let Some(old) = self.ivars().status_bar.borrow_mut().take() {
            status_bar.removeStatusItem(&old._status_item);
        }

        let mtm = self.mtm();
        let menu = build_menu(mtm, self);
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
        let status_button = status_item.button(mtm);
        let status_image = status_bar_icon();

        configure_status_item(&status_item, status_button.as_ref(), status_image.as_ref());
        status_item.setMenu(Some(&menu));
        status_item.setVisible(true);

        *self.ivars().status_bar.borrow_mut() = Some(StatusBarState {
            _menu: menu,
            _status_item: status_item,
            _status_button: status_button,
            _status_image: status_image,
        });
    }

    // Lets a second launch reach this process so it can rebuild a dropped
    // status item. Delivered on the main run loop, so the handler can safely
    // touch AppKit.
    fn register_reactivation_observer(&self) {
        let center = NSDistributedNotificationCenter::defaultCenter();
        unsafe {
            center.addObserver_selector_name_object_suspensionBehavior(
                self.as_ref(),
                sel!(reactivateStatusBar:),
                Some(nsstring(REACTIVATE_NOTIFICATION).as_ref()),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
    }

    // Rebuilds every menu item in place so a language switch retitles the
    // whole menu without replacing the status item.
    fn rebuild_menu(&self) {
        let guard = self.ivars().status_bar.borrow();
        let Some(state) = guard.as_ref() else {
            return;
        };
        let menu = &state._menu;
        menu.removeAllItems();
        menu.setTitle(nsstring(&text().app_title).as_ref());
        populate_menu(menu, self.mtm(), self);
    }
}

pub fn run(ctx: LauncherContext) -> LauncherResult<()> {
    CTX.set(ctx.clone()).ok();

    let mtm = MainThreadMarker::new().ok_or_else(|| text().main_thread_required)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let _keep_alive = delegate;
    start_startup_tasks(ctx.clone());
    app.run();
    Ok(())
}

// Called from a second launch while another launcher already holds the
// single-instance lock. The user most likely relaunched because the menu bar
// icon vanished while the launcher kept running, so restore it before exiting.
pub fn activate_running_instance() {
    // A launchd kickstart restarts the launcher in the proper GUI session and
    // reliably brings the icon back — this is exactly the manual
    // `launchctl kickstart -k gui/<uid>/<label>` recovery. Prefer it whenever
    // the LaunchAgent is installed (the common launch-at-login case).
    if launch_agent_installed() && kickstart_launch_agent() {
        return;
    }

    // No LaunchAgent to restart (launch-at-login disabled): fall back to asking
    // the running instance to rebuild its status item in place.
    let center = NSDistributedNotificationCenter::defaultCenter();
    unsafe {
        center.postNotificationName_object(nsstring(REACTIVATE_NOTIFICATION).as_ref(), None);
    }
    // The post hands off to distnoted synchronously, but give it a brief moment
    // to fan out before this short-lived process tears down.
    thread::sleep(std::time::Duration::from_millis(200));
}

fn configure_status_item(
    status_item: &NSStatusItem,
    status_button: Option<&Retained<NSStatusBarButton>>,
    status_image: Option<&Retained<NSImage>>,
) {
    if let Some(button) = status_button {
        if let Some(image) = status_image {
            button.setImage(Some(image));
        } else {
            button.setTitle(nsstring("Anda").as_ref());
        }
    } else if let Some(image) = status_image {
        #[allow(deprecated)]
        status_item.setImage(Some(image));
    } else {
        #[allow(deprecated)]
        status_item.setTitle(Some(nsstring("Anda").as_ref()));
    }
}

pub fn show_error(title: &str, message: &str) {
    show_alert(title, message);
}

fn build_menu(mtm: MainThreadMarker, delegate: &Delegate) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), nsstring(&text().app_title).as_ref());
    menu.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    populate_menu(&menu, mtm, delegate);
    menu
}

fn populate_menu(menu: &NSMenu, mtm: MainThreadMarker, delegate: &Delegate) {
    let copy = text();
    add_item(menu, mtm, &copy.open_anda, sel!(openAnda:), delegate);
    add_item(menu, mtm, &copy.open_logs, sel!(openLogs:), delegate);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_disabled_item(menu, mtm, &copy.status);
    let status = core::cached_daemon_status();
    let status_pid = add_disabled_item(menu, mtm, &status_pid_title(&status));
    status_pid.setTag(STATUS_PID_MENU_TAG);
    let status_gateway = add_disabled_item(menu, mtm, &status_gateway_title(&status));
    status_gateway.setTag(STATUS_GATEWAY_MENU_TAG);
    let status_conversations = add_disabled_item(menu, mtm, &status_conversations_title(&status));
    status_conversations.setTag(STATUS_CONVERSATIONS_MENU_TAG);
    let status_memory_nodes = add_disabled_item(menu, mtm, &status_memory_nodes_title(&status));
    status_memory_nodes.setTag(STATUS_MEMORY_NODES_MENU_TAG);
    let status_memory_links = add_disabled_item(menu, mtm, &status_memory_links_title(&status));
    status_memory_links.setTag(STATUS_MEMORY_LINKS_MENU_TAG);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        menu,
        mtm,
        &copy.restart_daemon,
        sel!(restartDaemon:),
        delegate,
    );
    add_item(
        menu,
        mtm,
        &copy.browser_extension_token,
        sel!(generateBrowserExtensionToken:),
        delegate,
    );
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_settings_submenu(menu, mtm, delegate, &copy);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let check_update = add_item(
        menu,
        mtm,
        &core::check_update_menu_label(),
        sel!(checkUpdate:),
        delegate,
    );
    check_update.setTag(CHECK_UPDATE_MENU_TAG);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(menu, mtm, &copy.quit, sel!(quit:), delegate);
}

fn refresh_status_menu_items(menu: &NSMenu) {
    let status = core::cached_daemon_status();
    if let Some(item) = menu.itemWithTag(STATUS_PID_MENU_TAG) {
        item.setTitle(nsstring(&status_pid_title(&status)).as_ref());
    }
    if let Some(item) = menu.itemWithTag(STATUS_GATEWAY_MENU_TAG) {
        item.setTitle(nsstring(&status_gateway_title(&status)).as_ref());
    }
    if let Some(item) = menu.itemWithTag(STATUS_CONVERSATIONS_MENU_TAG) {
        item.setTitle(nsstring(&status_conversations_title(&status)).as_ref());
    }
    if let Some(item) = menu.itemWithTag(STATUS_MEMORY_NODES_MENU_TAG) {
        item.setTitle(nsstring(&status_memory_nodes_title(&status)).as_ref());
    }
    if let Some(item) = menu.itemWithTag(STATUS_MEMORY_LINKS_MENU_TAG) {
        item.setTitle(nsstring(&status_memory_links_title(&status)).as_ref());
    }
}

fn refresh_update_menu_item(menu: &NSMenu) {
    if let Some(item) = menu.itemWithTag(CHECK_UPDATE_MENU_TAG) {
        item.setTitle(nsstring(&core::check_update_menu_label()).as_ref());
    }
}

fn status_pid_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    status_value_title(
        &copy.status_pid,
        status.pid.as_deref(),
        &copy.status_unavailable,
    )
}

fn status_gateway_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    status_value_title(
        &copy.status_gateway_url,
        status.gateway_url.as_deref(),
        &copy.status_unavailable,
    )
}

fn status_conversations_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    status_value_title(
        &copy.status_conversations,
        status.conversations.as_deref(),
        &copy.status_unavailable,
    )
}

fn status_memory_nodes_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    status_value_title(
        &copy.status_memory_nodes,
        status.memory_nodes.as_deref(),
        &copy.status_unavailable,
    )
}

fn status_memory_links_title(status: &core::LauncherDaemonStatus) -> String {
    let copy = text();
    status_value_title(
        &copy.status_memory_links,
        status.memory_links.as_deref(),
        &copy.status_unavailable,
    )
}

fn status_value_title(label: &str, value: Option<&str>, unavailable: &str) -> String {
    format!("{}: {}", label, value.unwrap_or(unavailable))
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
    add_item(
        &submenu,
        mtm,
        &copy.reload_models,
        sel!(reloadModels:),
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
    add_language_submenu(&submenu, mtm, delegate, copy);
    item.setSubmenu(Some(&submenu));
    menu.addItem(&item);
}

fn add_language_submenu(
    menu: &NSMenu,
    mtm: MainThreadMarker,
    delegate: &Delegate,
    copy: &core::LauncherText,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            nsstring(&copy.language).as_ref(),
            None,
            nsstring("").as_ref(),
        )
    };
    let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), nsstring(&copy.language).as_ref());
    let current_language = core::launcher_language();
    for (index, language) in core::LauncherLanguage::ALL.into_iter().enumerate() {
        let language_item = add_item(
            &submenu,
            mtm,
            language.native_name(),
            sel!(selectLanguage:),
            delegate,
        );
        language_item.setTag(LANGUAGE_MENU_TAG_BASE + index as isize);
        if language == current_language {
            language_item.setState(NSControlStateValueOn);
        }
    }
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

fn show_info(title: &str, message: &str) {
    show_alert(title, message);
}

fn show_browser_extension_token_result(result: &CommandResult) {
    let title = text().browser_extension_token_title;
    if result.success && copy_to_clipboard(&result.message).is_ok() {
        let token = core::browser_extension_bearer_token(&result.message);
        let message = format!(
            "{}\n\n{}",
            text().browser_extension_token_copied,
            result.message
        );
        show_browser_extension_token_dialog(&title, &message, token.as_deref());
    } else {
        show_background_dialog(&title, &result.message);
    }
}

// Shown via osascript so it works from the background threads that run the
// token command; NSAlert is main-thread only.
fn show_browser_extension_token_dialog(title: &str, message: &str, token: Option<&str>) {
    let Some(token) = token else {
        show_background_dialog(title, message);
        return;
    };

    let copy = text();
    let script = format!(
        "display dialog {} with title {} buttons {{{}, {}}} default button {} with icon note",
        applescript_string(message),
        applescript_string(title),
        applescript_string(&copy.ok),
        applescript_string(&copy.browser_extension_token_copy_button),
        applescript_string(&copy.ok),
    );
    let output = match Command::new("osascript").arg("-e").arg(script).output() {
        Ok(output) => output,
        Err(_) => {
            eprintln!("{title}: {message}");
            return;
        }
    };

    let copy_clicked = output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .contains(copy.browser_extension_token_copy_button.as_str());
    if copy_clicked {
        match copy_to_clipboard(token) {
            Ok(()) => show_background_dialog(title, &copy.browser_extension_token_only_copied),
            Err(err) => show_background_dialog(title, &err.to_string()),
        }
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

// Menu actions run `anda` commands that can block for seconds (daemon
// restart) or as long as the user keeps a wizard open, so they must stay off
// the AppKit main thread; results are reported through osascript dialogs,
// which are safe from any thread.
fn spawn_menu_action(action: impl FnOnce() + Send + 'static) {
    thread::spawn(move || {
        let Some(_guard) = core::try_begin_menu_action() else {
            return;
        };
        action();
    });
}

fn run_settings_wizard_async(ctx: LauncherContext) {
    spawn_menu_action(move || match settings::run_wizard(&ctx) {
        Ok(true) => {
            let result = core::reload_models_or_start_daemon(&ctx);
            show_background_dialog(&text().app_title, &result.message);
        }
        Ok(false) => {}
        Err(err) => show_background_dialog(&text().settings_title, &err.to_string()),
    });
}

fn restart_daemon_async(ctx: LauncherContext) {
    spawn_menu_action(move || {
        let result = core::restart_daemon(&ctx).unwrap_or_else(error_result);
        show_background_dialog(&text().app_title, &result.message);
    });
}

fn reload_models_async(ctx: LauncherContext) {
    spawn_menu_action(move || {
        let result = core::reload_models(&ctx).unwrap_or_else(error_result);
        show_background_dialog(&text().app_title, &result.message);
    });
}

fn show_browser_extension_token_result_async(ctx: LauncherContext) {
    spawn_menu_action(move || {
        show_browser_extension_token_result(
            &core::generate_browser_extension_token(&ctx).unwrap_or_else(error_result),
        );
    });
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
    // Hold the menu-action gate so a menu click cannot race the initial
    // setup wizard with a second wizard or daemon command.
    let _guard = core::begin_menu_action();
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
        loop {
            if !core::begin_update_check() {
                thread::sleep(core::auto_update_poll_interval());
                continue;
            }

            match core::check_update_if_due(&ctx) {
                Ok(state) => {
                    core::finish_update_check(Some(state));
                }
                Err(err) => {
                    core::finish_update_check(None);
                    eprintln!("{}: {err}", text().update_check_failed_title);
                }
            }
            thread::sleep(core::auto_update_poll_interval());
        }
    });
}

fn run_manual_update_check(ctx: LauncherContext) {
    if let Some(state) = core::downloaded_update_state() {
        // Installing stops, updates, and restarts the daemon — far too slow
        // for the main thread.
        thread::spawn(move || prompt_update_ready(ctx, state));
        return;
    }

    if !core::begin_update_check() {
        return;
    }

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
    if !core::begin_update_restart_prompt(&state) {
        return;
    }

    let latest = state.latest_tag_label();
    if !confirm_update_restart(&latest) {
        core::finish_update_restart_prompt(&state);
        return;
    }

    // Serialize with other daemon-touching menu actions so a concurrent
    // restart cannot interleave with the install.
    let _guard = core::begin_menu_action();
    show_background_notification(&text().update_restart_title, &text().update_restart_started);
    let result = core::install_update_and_restart(&ctx).unwrap_or_else(error_result);
    if result.success {
        core::finish_update_restart_success(&state);
        if let Err(err) = restart_launcher_after_update(&ctx) {
            show_background_dialog(
                &text().update_restart_title,
                &text().update_restart_failed_message(&err.to_string()),
            );
        }
    } else {
        show_background_dialog(
            &text().update_restart_title,
            &text().update_restart_failed_message(&result.message),
        );
        core::finish_update_restart_prompt(&state);
    }
}

fn restart_launcher_after_update(ctx: &LauncherContext) -> LauncherResult<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.75; exec \"$1\"")
        .arg("anda-launcher-restart")
        .arg(&ctx.launcher_exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to restart launcher: {err}"))?;
    std::process::exit(0);
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

// Kills and restarts the launcher service so it comes back up cleanly in the
// GUI session. `-k` restarts a running job and simply starts a stopped one.
fn kickstart_launch_agent() -> bool {
    Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(format!(
            "gui/{}/{}",
            unsafe { libc::geteuid() },
            LAUNCH_AGENT_LABEL
        ))
        .status()
        .is_ok_and(|status| status.success())
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
