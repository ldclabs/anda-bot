use std::{
    ffi::{OsStr, c_void},
    mem::size_of,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::Command,
    ptr,
    sync::OnceLock,
};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject, HBITMAP, HGDIOBJ,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        },
        Threading::CREATE_NEW_CONSOLE,
    },
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
            Shell_NotifyIconW, ShellExecuteW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateIconIndirect,
            CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
            DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, HMENU, ICONINFO,
            IDI_APPLICATION, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MF_SEPARATOR,
            MF_STRING, MSG, MessageBoxW, PostQuitMessage, RegisterClassW, SW_SHOWNORMAL,
            SetForegroundWindow, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP,
            WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult, text},
    settings,
};

const CLASS_NAME: &str = "AndaBotLauncherWindow";
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo.png");
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const AUTOSTART_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const AUTOSTART_RUN_VALUE: &str = "AndaBotLauncher";
const LEGACY_DAEMON_TASK_NAME: &str = "Anda Bot";
const LEGACY_LAUNCHER_TASK_NAME: &str = "Anda Bot Launcher";

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
static LAUNCHER_ICON: OnceLock<(usize, bool)> = OnceLock::new();

pub fn run(ctx: LauncherContext) -> LauncherResult<()> {
    CTX.set(ctx.clone()).ok();

    if core::config_needs_setup(&ctx) {
        if settings::run_initial_setup_wizard(&ctx)? {
            show_result(text().app_title, &core::start_daemon(&ctx)?);
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
            hIcon: launcher_icon(),
            ..Default::default()
        };
        RegisterClassW(&wndclass);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide_null(text().launcher_window_title).as_ptr(),
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
            return Err(text().create_window_failed.into());
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
            destroy_launcher_icon();
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
                text().app_title,
                &core::restart_daemon(ctx).unwrap_or_else(error_result),
            ),
            Ok(false) => {}
            Err(err) => show_error(text().settings_title, &err.to_string()),
        },
        ID_STATUS => show_result(
            text().app_title,
            &core::daemon_status(ctx).unwrap_or_else(error_result),
        ),
        ID_START => show_result(
            text().app_title,
            &core::start_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_STOP => show_result(
            text().app_title,
            &core::stop_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_RESTART => show_result(
            text().app_title,
            &core::restart_daemon(ctx).unwrap_or_else(error_result),
        ),
        ID_AUTOSTART => match toggle_autostart(ctx) {
            Ok(message) => message_box(text().app_title, &message, MB_OK | MB_ICONINFORMATION),
            Err(err) => show_error(text().app_title, &err.to_string()),
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
        let copy = text();
        let menu = CreatePopupMenu();
        append_item(menu, ID_OPEN, copy.open_anda);
        append_item(menu, ID_SETTINGS, copy.settings);
        append_separator(menu);
        append_item(menu, ID_STATUS, copy.status);
        append_item(menu, ID_START, copy.start_daemon);
        append_item(menu, ID_STOP, copy.stop_daemon);
        append_item(menu, ID_RESTART, copy.restart_daemon);
        append_separator(menu);
        let autostart_label = if launcher_autostart_installed() {
            copy.disable_launch_at_login
        } else {
            copy.launch_at_login
        };
        append_item(menu, ID_AUTOSTART, autostart_label);
        append_item(menu, ID_LOGS, copy.open_logs);
        append_separator(menu);
        append_item(menu, ID_QUIT, copy.quit);

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
        hIcon: launcher_icon(),
        ..Default::default()
    };
    copy_wide_fixed(&mut data.szTip, text().app_title);
    Shell_NotifyIconW(NIM_ADD, &data);
}

fn launcher_icon() -> HICON {
    let (icon, _) = LAUNCHER_ICON.get_or_init(|| match create_launcher_icon() {
        Some(icon) => (icon as usize, true),
        None => unsafe { (LoadIconW(ptr::null_mut(), IDI_APPLICATION) as usize, false) },
    });
    *icon as HICON
}

fn destroy_launcher_icon() {
    let Some((icon, owned)) = LAUNCHER_ICON.get() else {
        return;
    };
    if *owned && *icon != 0 {
        unsafe {
            DestroyIcon(*icon as HICON);
        }
    }
}

fn create_launcher_icon() -> Option<HICON> {
    let pixels = decode_launcher_icon_pixels()?;
    let width = i32::try_from(pixels.width).ok()?;
    let height = i32::try_from(pixels.height).ok()?;

    let color_bitmap = unsafe { create_color_bitmap(width, height, &pixels.bgra)? };
    let mask_bitmap = unsafe { create_mask_bitmap(width, height, &pixels.mask) };
    if mask_bitmap.is_null() {
        unsafe {
            DeleteObject(color_bitmap as HGDIOBJ);
        }
        return None;
    }

    let icon_info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bitmap,
        hbmColor: color_bitmap,
    };
    let icon = unsafe { CreateIconIndirect(&icon_info) };
    unsafe {
        DeleteObject(color_bitmap as HGDIOBJ);
        DeleteObject(mask_bitmap as HGDIOBJ);
    }

    if icon.is_null() { None } else { Some(icon) }
}

struct IconPixels {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    mask: Vec<u8>,
}

fn decode_launcher_icon_pixels() -> Option<IconPixels> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(LAUNCHER_ICON_PNG));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut frame = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut frame).ok()?;
    let frame = &frame[..info.buffer_size()];

    let width = usize::try_from(info.width).ok()?;
    let height = usize::try_from(info.height).ok()?;
    let mask_stride = width.div_ceil(16) * 2;
    let mut bgra = Vec::with_capacity(width.checked_mul(height)?.checked_mul(4)?);
    let mut mask = vec![0u8; mask_stride.checked_mul(height)?];

    for y in 0..height {
        let row_start = y.checked_mul(info.line_size)?;
        let row = frame.get(row_start..row_start.checked_add(info.line_size)?)?;
        for x in 0..width {
            let (red, green, blue, alpha) = match info.color_type {
                png::ColorType::Grayscale => {
                    let gray = *row.get(x)?;
                    (gray, gray, gray, 255)
                }
                png::ColorType::Rgb => {
                    let start = x.checked_mul(3)?;
                    (
                        *row.get(start)?,
                        *row.get(start + 1)?,
                        *row.get(start + 2)?,
                        255,
                    )
                }
                png::ColorType::GrayscaleAlpha => {
                    let start = x.checked_mul(2)?;
                    let gray = *row.get(start)?;
                    (gray, gray, gray, *row.get(start + 1)?)
                }
                png::ColorType::Rgba => {
                    let start = x.checked_mul(4)?;
                    (
                        *row.get(start)?,
                        *row.get(start + 1)?,
                        *row.get(start + 2)?,
                        *row.get(start + 3)?,
                    )
                }
                png::ColorType::Indexed => return None,
            };
            bgra.extend_from_slice(&[blue, green, red, alpha]);
            if alpha < 128 {
                let mask_index = y.checked_mul(mask_stride)?.checked_add(x / 8)?;
                mask[mask_index] |= 0x80 >> (x % 8);
            }
        }
    }

    Some(IconPixels {
        width: info.width,
        height: info.height,
        bgra,
        mask,
    })
}

unsafe fn create_color_bitmap(width: i32, height: i32, bgra: &[u8]) -> Option<HBITMAP> {
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: bgra.len() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            ptr::null_mut(),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        )
    };
    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            unsafe {
                DeleteObject(bitmap as HGDIOBJ);
            }
        }
        return None;
    }

    unsafe {
        ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
    }
    Some(bitmap)
}

unsafe fn create_mask_bitmap(width: i32, height: i32, mask: &[u8]) -> HBITMAP {
    unsafe { CreateBitmap(width, height, 1, 1, mask.as_ptr().cast()) }
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
        delete_run_autostart()?;
        delete_legacy_scheduled_tasks();
        Ok(text().launch_at_login_disabled.to_string())
    } else {
        set_run_autostart(ctx)?;
        delete_legacy_scheduled_tasks();
        Ok(text().launch_at_login_enabled.to_string())
    }
}

fn launcher_autostart_installed() -> bool {
    run_autostart_exists()
}

fn set_run_autostart(ctx: &LauncherContext) -> LauncherResult<()> {
    let command = windows_command_line([
        ctx.launcher_exe.as_os_str(),
        OsStr::new("--home"),
        ctx.home.as_os_str(),
    ]);
    let value = wide_null(&command);
    let value_name = wide_null(AUTOSTART_RUN_VALUE);
    let key = RegistryKey::create(AUTOSTART_RUN_KEY, KEY_SET_VALUE)?;
    let status = unsafe {
        RegSetValueExW(
            key.raw(),
            value_name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast::<u8>(),
            (value.len() * size_of::<u16>()) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        return Ok(());
    }

    Err(win32_registry_error("set launch-at-login value", status).into())
}

fn delete_run_autostart() -> LauncherResult<()> {
    let value_name = wide_null(AUTOSTART_RUN_VALUE);
    let Some(key) = RegistryKey::open_optional(AUTOSTART_RUN_KEY, KEY_SET_VALUE)? else {
        return Ok(());
    };
    let status = unsafe { RegDeleteValueW(key.raw(), value_name.as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }

    Err(win32_registry_error("delete launch-at-login value", status).into())
}

fn run_autostart_exists() -> bool {
    let value_name = wide_null(AUTOSTART_RUN_VALUE);
    let Ok(Some(key)) = RegistryKey::open_optional(AUTOSTART_RUN_KEY, KEY_QUERY_VALUE) else {
        return false;
    };
    let mut value_type = 0;
    let mut value_len = 0;
    let status = unsafe {
        RegQueryValueExW(
            key.raw(),
            value_name.as_ptr(),
            ptr::null(),
            &mut value_type,
            ptr::null_mut(),
            &mut value_len,
        )
    };

    status == ERROR_SUCCESS && value_type == REG_SZ && value_len > 0
}

fn delete_legacy_scheduled_tasks() {
    let _ = Command::new("schtasks.exe")
        .args(["/Delete", "/TN", LEGACY_DAEMON_TASK_NAME, "/F"])
        .creation_flags(0x08000000)
        .status();
    let _ = Command::new("schtasks.exe")
        .args(["/Delete", "/TN", LEGACY_LAUNCHER_TASK_NAME, "/F"])
        .creation_flags(0x08000000)
        .status();
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn create(path: &str, access: u32) -> Result<Self, std::io::Error> {
        let mut key = ptr::null_mut();
        let path = wide_null(path);
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                ptr::null(),
                0,
                access,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(win32_registry_error("open HKCU Run key", status))
        }
    }

    fn open_optional(path: &str, access: u32) -> Result<Option<Self>, std::io::Error> {
        let mut key = ptr::null_mut();
        let path = wide_null(path);
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
        if status == ERROR_SUCCESS {
            Ok(Some(Self(key)))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(win32_registry_error("open HKCU Run key", status))
        }
    }

    fn raw(&self) -> HKEY {
        self.0
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn win32_registry_error(action: &str, code: u32) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Other,
        format!(
            "Windows registry error while trying to {action}: {code} ({})",
            std::io::Error::from_raw_os_error(code as i32)
        ),
    )
}

fn open_anda_terminal(ctx: &LauncherContext) {
    let _ = Command::new("cmd.exe")
        .arg("/D")
        .arg("/K")
        .arg(&ctx.anda_exe)
        .arg("--home")
        .arg(&ctx.home)
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

fn windows_command_line<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> String {
    args.into_iter()
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
