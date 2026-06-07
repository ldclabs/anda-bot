use std::{
    ffi::OsStr,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::Command,
    ptr,
    sync::OnceLock,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::CREATE_NEW_CONSOLE},
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
            Shell_NotifyIconW, ShellExecuteW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
            DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
            GetMessageW, HMENU, IDI_APPLICATION, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION,
            MB_OK, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW, PostQuitMessage, RegisterClassW,
            SW_SHOWNORMAL, SetForegroundWindow, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
            WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
            WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult},
    settings,
};

const CLASS_NAME: &str = "AndaBotLauncherWindow";
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const AUTOSTART_TASK_NAME: &str = "Anda Bot Launcher";

const ID_OPEN: usize = 1001;
const ID_SETTINGS: usize = 1002;
const ID_STATUS: usize = 1003;
const ID_START: usize = 1004;
const ID_STOP: usize = 1005;
const ID_RESTART: usize = 1006;
const ID_AUTOSTART: usize = 1007;
const ID_LOGS: usize = 1008;
const ID_QUIT: usize = 1009;

static CTX: OnceLock<LauncherContext> = OnceLock::new();

pub fn run(ctx: LauncherContext) -> LauncherResult<()> {
    CTX.set(ctx.clone()).ok();

    if core::config_needs_setup(&ctx) {
        if settings::run_wizard(&ctx)? {
            show_result("Anda Bot", &core::start_daemon(&ctx)?);
        }
    } else {
        let _ = core::start_daemon(&ctx);
    }

    unsafe {
        let class_name = wide_null(CLASS_NAME);
        let hinstance = GetModuleHandleW(ptr::null());
        let wndclass = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name.as_ptr(),
            hIcon: LoadIconW(ptr::null_mut(), IDI_APPLICATION),
            ..Default::default()
        };
        RegisterClassW(&wndclass);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide_null("Anda Bot Launcher").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            ptr::null(),
        );
        if hwnd.is_null() {
            return Err("could not create launcher window".into());
        }

        add_tray_icon(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

pub fn show_error(title: &str, message: &str) {
    message_box(title, message, MB_OK | MB_ICONERROR);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY if lparam as u32 == WM_RBUTTONUP || lparam as u32 == WM_LBUTTONUP => {
            show_tray_menu(hwnd);
            0
        }
        WM_COMMAND => {
            handle_command(hwnd, wparam & 0xffff);
            0
        }
        WM_DESTROY => {
            delete_tray_icon(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_command(hwnd: HWND, id: usize) {
    let Some(ctx) = CTX.get() else {
        return;
    };

    match id {
        ID_OPEN => open_anda_terminal(ctx),
        ID_SETTINGS => match settings::run_wizard(ctx) {
            Ok(true) => show_result(
                "Anda Bot",
                &core::restart_daemon(ctx).unwrap_or_else(error_result),
            ),
            Ok(false) => {}
            Err(err) => show_error("Anda Bot Settings", &err.to_string()),
        },
        ID_STATUS => show_result(
            "Anda Bot",
            &core::daemon_status(ctx).unwrap_or_else(error_result),
        ),
        ID_START => show_result(
            "Anda Bot",
            &core::start_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_STOP => show_result(
            "Anda Bot",
            &core::stop_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_RESTART => show_result(
            "Anda Bot",
            &core::restart_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_AUTOSTART => match toggle_autostart(ctx) {
            Ok(message) => message_box("Anda Bot", &message, MB_OK | MB_ICONINFORMATION),
            Err(err) => show_error("Anda Bot", &err.to_string()),
        },
        ID_LOGS => open_path(&ctx.logs_dir()),
        ID_QUIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        append_item(menu, ID_OPEN, "Open Anda");
        append_item(menu, ID_SETTINGS, "Settings...");
        append_separator(menu);
        append_item(menu, ID_STATUS, "Status");
        append_item(menu, ID_START, "Start daemon");
        append_item(menu, ID_STOP, "Stop daemon");
        append_item(menu, ID_RESTART, "Restart daemon");
        append_separator(menu);
        let autostart_label = if launcher_autostart_installed() {
            "Disable launch at login"
        } else {
            "Launch at login"
        };
        append_item(menu, ID_AUTOSTART, autostart_label);
        append_item(menu, ID_LOGS, "Open logs");
        append_separator(menu);
        append_item(menu, ID_QUIT, "Quit");

        let mut point = POINT::default();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            ptr::null(),
        );
        DestroyMenu(menu);
    }
}

unsafe fn add_tray_icon(hwnd: HWND) {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: LoadIconW(ptr::null_mut(), IDI_APPLICATION),
        ..Default::default()
    };
    copy_wide_fixed(&mut data.szTip, "Anda Bot");
    Shell_NotifyIconW(NIM_ADD, &data);
}

unsafe fn delete_tray_icon(hwnd: HWND) {
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    Shell_NotifyIconW(NIM_DELETE, &data);
}

unsafe fn append_item(menu: HMENU, id: usize, text: &str) {
    AppendMenuW(menu, MF_STRING, id, wide_null(text).as_ptr());
}

unsafe fn append_separator(menu: HMENU) {
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
}

fn show_result(title: &str, result: &CommandResult) {
    let style = if result.success {
        MB_OK | MB_ICONINFORMATION
    } else {
        MB_OK | MB_ICONERROR
    };
    message_box(title, &result.message, style);
}

fn error_result(err: Box<dyn std::error::Error + Send + Sync>) -> CommandResult {
    CommandResult {
        success: false,
        message: err.to_string(),
    }
}

fn toggle_autostart(ctx: &LauncherContext) -> LauncherResult<String> {
    if launcher_autostart_installed() {
        run_schtasks(&["/Delete", "/TN", AUTOSTART_TASK_NAME, "/F"])?;
        Ok("Launch at login disabled.".to_string())
    } else {
        let command = windows_command_line(&[ctx.launcher_exe.clone()]);
        run_schtasks(&[
            "/Create",
            "/TN",
            AUTOSTART_TASK_NAME,
            "/SC",
            "ONLOGON",
            "/TR",
            &command,
            "/F",
        ])?;
        Ok("Launch at login enabled.".to_string())
    }
}

fn launcher_autostart_installed() -> bool {
    run_schtasks(&["/Query", "/TN", AUTOSTART_TASK_NAME]).is_ok()
}

fn run_schtasks(args: &[&str]) -> LauncherResult<()> {
    let output = Command::new("schtasks.exe")
        .args(args)
        .creation_flags(0x08000000)
        .output()?;
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
    Err(format!("schtasks.exe failed: {detail}").into())
}

fn open_anda_terminal(ctx: &LauncherContext) {
    let command = format!(
        "title Anda Bot && \"{}\" --home \"{}\"",
        ctx.anda_exe.display(),
        ctx.home.display()
    );
    let _ = Command::new("cmd.exe")
        .arg("/K")
        .arg(command)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn();
}

fn open_path(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            wide_null("open").as_ptr(),
            wide_null_os(path.as_os_str()).as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn message_box(title: &str, message: &str, style: u32) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide_null(message).as_ptr(),
            wide_null(title).as_ptr(),
            style,
        );
    }
}

fn windows_command_line(args: &[std::path::PathBuf]) -> String {
    args.iter()
        .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_windows_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return value.to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

fn copy_wide_fixed<const N: usize>(dest: &mut [u16; N], value: &str) {
    let wide = wide_null(value);
    let len = wide.len().saturating_sub(1).min(N.saturating_sub(1));
    dest[..len].copy_from_slice(&wide[..len]);
    dest[len] = 0;
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn wide_null_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
