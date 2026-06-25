use std::{
    env,
    ffi::{OsStr, c_void},
    io::Write,
    mem::size_of,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO};
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
            DestroyWindow, DispatchMessageW, FindWindowExW, GetCursorPos, GetMessageW, HICON,
            HMENU, ICONINFO, IDI_APPLICATION, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
            MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW,
            PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
            SMTO_ABORTIFHUNG, SMTO_BLOCK, SW_SHOWNORMAL, SendMessageTimeoutW, SetForegroundWindow,
            TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_COMMAND, WM_DESTROY,
            WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{
    core::{self, CommandResult, LauncherContext, LauncherResult, text},
    settings,
};

const CLASS_NAME: &str = "AndaBotLauncherWindow";
const LAUNCHER_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo.png");
const LAUNCHER_ICON_FILE: &str = "anda.ico";
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_LAUNCHER_ACTIVATE: u32 = WM_APP + 2;
const EXISTING_WINDOW_PING_TIMEOUT_MS: u32 = 1500;
const EXISTING_WINDOW_STARTUP_WAIT_MS: u64 = 3000;
const EXISTING_WINDOW_POLL_MS: u64 = 100;
const CREATE_NO_WINDOW: u32 = 0x08000000;
const AUTOSTART_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const AUTOSTART_RUN_VALUE: &str = "AndaBotLauncher";
const LEGACY_DAEMON_TASK_NAME: &str = "Anda Bot";
const LEGACY_LAUNCHER_TASK_NAME: &str = "Anda Bot Launcher";

const ID_OPEN: usize = 1001;
const ID_SETTINGS: usize = 1002;
const ID_RESTART: usize = 1006;
const ID_AUTOSTART: usize = 1007;
const ID_LOGS: usize = 1008;
const ID_CHECK_UPDATE: usize = 1009;
const ID_QUIT: usize = 1010;
const ID_BROWSER_TOKEN: usize = 1011;
const ID_RELOAD_MODELS: usize = 1012;
const ID_LANGUAGE_BASE: usize = 1100;

static CTX: OnceLock<LauncherContext> = OnceLock::new();
static LAUNCHER_ICON: OnceLock<(usize, bool)> = OnceLock::new();
// `RegisterWindowMessage("TaskbarCreated")` id. The shell broadcasts this to
// every top-level window when the taskbar is (re)created — most notably after
// Explorer.exe restarts, which destroys all tray icons. Receiving it is the
// signal to re-add ours.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

pub fn run(ctx: LauncherContext) -> LauncherResult<()> {
    CTX.set(ctx.clone()).ok();

    unsafe {
        let class_name = wide_null(CLASS_NAME);
        let hinstance = GetModuleHandleW(ptr::null());
        // Register before the window exists so wnd_proc can recognize the
        // broadcast as soon as it can arrive.
        TASKBAR_CREATED
            .set(RegisterWindowMessageW(wide_null("TaskbarCreated").as_ptr()))
            .ok();
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
            wide_null(&text().launcher_window_title).as_ptr(),
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
        start_startup_tasks(ctx.clone());

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

pub fn activate_existing_instance() -> LauncherResult<bool> {
    Ok(activate_existing_window())
}

pub fn wait_for_existing_instance() -> LauncherResult<bool> {
    let deadline = Instant::now() + Duration::from_millis(EXISTING_WINDOW_STARTUP_WAIT_MS);
    loop {
        if activate_existing_window() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(EXISTING_WINDOW_POLL_MS));
    }
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
    if Some(msg) == taskbar_created_message() {
        // Explorer restarted and wiped the tray; vend our icon again.
        unsafe {
            add_tray_icon(hwnd);
        }
        return 0;
    }

    match msg {
        WM_LAUNCHER_ACTIVATE => {
            // A second launch (the user reopening the shortcut) — make sure the
            // icon is present before showing the menu, so reopening also
            // restores an icon that went missing for any other reason.
            unsafe {
                add_tray_icon(hwnd);
            }
            show_tray_menu(hwnd);
            0
        }
        WM_TRAY if lparam as u32 == WM_RBUTTONUP || lparam as u32 == WM_LBUTTONUP => {
            show_tray_menu(hwnd);
            0
        }
        WM_COMMAND => {
            handle_command(hwnd, wparam & 0xffff);
            0
        }
        WM_DESTROY => {
            unsafe {
                delete_tray_icon(hwnd);
            }
            destroy_launcher_icon();
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// The registered "TaskbarCreated" id, or None if registration failed — the
// filter keeps a failed (zero) registration from ever matching WM_NULL.
fn taskbar_created_message() -> Option<u32> {
    TASKBAR_CREATED
        .get()
        .copied()
        .filter(|&message| message != 0)
}

fn handle_command(hwnd: HWND, id: usize) {
    let Some(ctx) = CTX.get() else {
        return;
    };

    match id {
        ID_OPEN => open_anda_terminal_async(ctx.clone()),
        ID_SETTINGS => run_settings_wizard_async(ctx.clone()),
        ID_RESTART => show_command_result_async(text().app_title.clone(), ctx.clone(), |ctx| {
            core::restart_daemon(ctx)
        }),
        ID_RELOAD_MODELS => {
            show_command_result_async(text().app_title.clone(), ctx.clone(), |ctx| {
                core::reload_models(ctx)
            })
        }
        ID_BROWSER_TOKEN => show_browser_extension_token_result_async(ctx.clone()),
        ID_CHECK_UPDATE => run_manual_update_check(hwnd, ctx.clone()),
        ID_AUTOSTART => toggle_autostart_async(ctx.clone()),
        ID_LOGS => open_logs_async(ctx.clone()),
        ID_QUIT => unsafe {
            DestroyWindow(hwnd);
        },
        id if (ID_LANGUAGE_BASE..ID_LANGUAGE_BASE + core::LauncherLanguage::ALL.len())
            .contains(&id) =>
        {
            select_language(ctx, id - ID_LANGUAGE_BASE);
        }
        _ => {}
    }
}

fn select_language(ctx: &LauncherContext, index: usize) {
    let Some(language) = core::LauncherLanguage::ALL.get(index).copied() else {
        return;
    };
    if let Err(err) = core::set_launcher_language(&ctx.home, language) {
        show_error(&text().app_title, &err.to_string());
    }
}

fn activate_existing_window() -> bool {
    let Some(hwnd) = responsive_launcher_window() else {
        return false;
    };

    unsafe { PostMessageW(hwnd, WM_LAUNCHER_ACTIVATE, 0, 0) != 0 }
}

fn responsive_launcher_window() -> Option<HWND> {
    let class_name = wide_null(CLASS_NAME);
    let mut previous = ptr::null_mut();
    loop {
        let hwnd =
            unsafe { FindWindowExW(ptr::null_mut(), previous, class_name.as_ptr(), ptr::null()) };
        if hwnd.is_null() {
            return None;
        }
        if window_responds(hwnd) {
            return Some(hwnd);
        }
        previous = hwnd;
    }
}

fn window_responds(hwnd: HWND) -> bool {
    let mut result = 0;
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_NULL,
            0,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            EXISTING_WINDOW_PING_TIMEOUT_MS,
            &mut result,
        ) != 0
    }
}

fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let copy = text();
        let menu = CreatePopupMenu();
        append_item(menu, ID_OPEN, &copy.open_anda);
        append_item(menu, ID_LOGS, &copy.open_logs);
        append_separator(menu);
        let status = core::cached_daemon_status();
        append_disabled_item(menu, &copy.status);
        append_disabled_item(menu, &status_pid_title(&status));
        append_disabled_item(menu, &status_gateway_title(&status));
        append_disabled_item(menu, &status_conversations_title(&status));
        append_disabled_item(menu, &status_memory_nodes_title(&status));
        append_disabled_item(menu, &status_memory_links_title(&status));
        append_separator(menu);
        append_item(menu, ID_RESTART, &copy.restart_daemon);
        append_item(menu, ID_BROWSER_TOKEN, &copy.browser_extension_token);
        append_separator(menu);
        let settings_menu = CreatePopupMenu();
        append_item(settings_menu, ID_SETTINGS, &copy.model_settings);
        append_item(settings_menu, ID_RELOAD_MODELS, &copy.reload_models);
        let autostart_label = if launcher_autostart_installed() {
            &copy.disable_launch_at_login
        } else {
            &copy.launch_at_login
        };
        append_item(settings_menu, ID_AUTOSTART, autostart_label);
        let language_menu = CreatePopupMenu();
        let current_language = core::launcher_language();
        for (index, language) in core::LauncherLanguage::ALL.into_iter().enumerate() {
            let flags = if language == current_language {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            AppendMenuW(
                language_menu,
                flags,
                ID_LANGUAGE_BASE + index,
                wide_null(language.native_name()).as_ptr(),
            );
        }
        append_submenu(settings_menu, language_menu, &copy.language);
        append_submenu(menu, settings_menu, &copy.settings);
        append_separator(menu);
        append_item(menu, ID_CHECK_UPDATE, &core::check_update_menu_label());
        append_separator(menu);
        append_item(menu, ID_QUIT, &copy.quit);

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
    copy_wide_fixed(&mut data.szTip, &text().app_title);
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &data);
    }
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
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

unsafe fn append_item(menu: HMENU, id: usize, text: &str) {
    unsafe {
        AppendMenuW(menu, MF_STRING, id, wide_null(text).as_ptr());
    }
}

unsafe fn append_disabled_item(menu: HMENU, text: &str) {
    unsafe {
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide_null(text).as_ptr());
    }
}

unsafe fn append_submenu(menu: HMENU, submenu: HMENU, text: &str) {
    unsafe {
        AppendMenuW(
            menu,
            MF_POPUP | MF_STRING,
            submenu as usize,
            wide_null(text).as_ptr(),
        );
    }
}

unsafe fn append_separator(menu: HMENU) {
    unsafe {
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    }
}

fn show_result(title: &str, result: &CommandResult) {
    let style = if result.success {
        MB_OK | MB_ICONINFORMATION
    } else {
        MB_OK | MB_ICONERROR
    };
    message_box(title, &result.message, style);
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

fn show_browser_extension_token_result(result: &CommandResult) {
    if result.success && copy_to_clipboard(&result.message).is_ok() {
        let token = core::browser_extension_bearer_token(&result.message);
        let copied = CommandResult {
            success: result.success,
            message: format!(
                "{}\r\n\r\n{}",
                text().browser_extension_token_copied,
                result.message
            ),
        };
        if show_browser_extension_token_dialog(&copied.message, token.as_deref()).is_err() {
            show_result(&text().browser_extension_token_title, &copied);
        }
    } else {
        show_result(&text().browser_extension_token_title, result);
    }
}

fn show_browser_extension_token_dialog(message: &str, token: Option<&str>) -> LauncherResult<()> {
    let script = browser_extension_token_dialog_script(message, token);
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-STA",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
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
    Err(text().command_failed(&detail).into())
}

fn browser_extension_token_dialog_script(message: &str, token: Option<&str>) -> String {
    let copy_button_visible = if token.is_some() { "$true" } else { "$false" };
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

$form = New-Object System.Windows.Forms.Form
$form.Text = '{title}'
$form.StartPosition = 'CenterScreen'
$form.ClientSize = New-Object System.Drawing.Size(680, 400)
$form.MinimizeBox = $false
$form.MaximizeBox = $false
$form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog

$text = New-Object System.Windows.Forms.TextBox
$text.Multiline = $true
$text.ReadOnly = $true
$text.ScrollBars = [System.Windows.Forms.ScrollBars]::Vertical
$text.WordWrap = $true
$text.BorderStyle = [System.Windows.Forms.BorderStyle]::None
$text.BackColor = $form.BackColor
$text.Font = New-Object System.Drawing.Font('Segoe UI', 10)
$text.Location = New-Object System.Drawing.Point(20, 20)
$text.Size = New-Object System.Drawing.Size(640, 300)
$text.Text = '{message}'
$form.Controls.Add($text)

$ok = New-Object System.Windows.Forms.Button
$ok.Text = '{ok}'
$ok.Size = New-Object System.Drawing.Size(96, 32)
$ok.Location = New-Object System.Drawing.Point(564, 344)
$ok.DialogResult = [System.Windows.Forms.DialogResult]::OK
$form.Controls.Add($ok)
$form.AcceptButton = $ok

if ({copy_button_visible}) {{
  $copy = New-Object System.Windows.Forms.Button
  $copy.Text = '{copy_token}'
  $copy.Size = New-Object System.Drawing.Size(120, 32)
  $copy.Location = New-Object System.Drawing.Point(430, 344)
  $copy.Add_Click({{
    [System.Windows.Forms.Clipboard]::SetText('{token}')
    [System.Windows.Forms.MessageBox]::Show('{token_copied}', '{title}', 'OK', 'Information') | Out-Null
    $form.DialogResult = [System.Windows.Forms.DialogResult]::OK
    $form.Close()
  }})
  $form.Controls.Add($copy)
}}

$form.TopMost = $true
[void]$form.ShowDialog()
"#,
        title = ps_single(&text().browser_extension_token_title),
        message = ps_single(message),
        ok = ps_single(&text().ok),
        copy_token = ps_single(&text().browser_extension_token_copy_button),
        token_copied = ps_single(&text().browser_extension_token_only_copied),
        token = ps_single(token.unwrap_or_default()),
        copy_button_visible = copy_button_visible,
    )
}

fn open_anda_terminal_async(ctx: LauncherContext) {
    thread::spawn(move || {
        open_anda_terminal(&ctx);
    });
}

// Serialized through the menu-action gate so rapid repeated tray clicks
// cannot run concurrent `anda` commands or stack result dialogs.
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
        Ok(true) => show_result(
            &text().app_title,
            &core::reload_models_or_start_daemon(&ctx),
        ),
        Ok(false) => {}
        Err(err) => show_error(&text().settings_title, &err.to_string()),
    });
}

fn show_command_result_async<F>(title: String, ctx: LauncherContext, command: F)
where
    F: FnOnce(&LauncherContext) -> LauncherResult<CommandResult> + Send + 'static,
{
    spawn_menu_action(move || {
        let result = command(&ctx).unwrap_or_else(error_result);
        show_result(&title, &result);
    });
}

fn show_browser_extension_token_result_async(ctx: LauncherContext) {
    spawn_menu_action(move || {
        show_browser_extension_token_result(
            &core::generate_browser_extension_token(&ctx).unwrap_or_else(error_result),
        );
    });
}

fn toggle_autostart_async(ctx: LauncherContext) {
    spawn_menu_action(move || match toggle_autostart(&ctx) {
        Ok(message) => message_box(&text().app_title, &message, MB_OK | MB_ICONINFORMATION),
        Err(err) => show_error(&text().app_title, &err.to_string()),
    });
}

fn open_logs_async(ctx: LauncherContext) {
    thread::spawn(move || {
        open_path(&ctx.logs_dir());
    });
}

fn copy_to_clipboard(value: &str) -> LauncherResult<()> {
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Set-Clipboard -Value ([Console]::In.ReadToEnd())",
        ])
        .stdin(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
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
        if let Err(err) = ensure_launch_entrypoints(&ctx) {
            eprintln!("failed to ensure Windows launch entrypoints: {err}");
        }
        if let Err(err) = run_startup_setup(&ctx) {
            show_error(&text().app_title, &err.to_string());
        }
        start_status_loop(ctx.clone());
        start_auto_update_loop(ctx);
    });
}

fn run_startup_setup(ctx: &LauncherContext) -> LauncherResult<()> {
    // Hold the menu-action gate so a tray click cannot race the initial
    // setup wizard with a second wizard or daemon command.
    let _guard = core::begin_menu_action();
    if core::config_needs_setup(ctx) {
        if settings::run_initial_setup_wizard(ctx)? {
            show_result(&text().app_title, &core::start_daemon(ctx)?);
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

fn run_manual_update_check(_hwnd: HWND, ctx: LauncherContext) {
    if let Some(state) = core::downloaded_update_state() {
        // Installing stops, updates, and restarts the daemon. Running it on
        // the message-loop thread would stop message pumping, which the
        // hung-launcher takeover in main() would misread as a dead instance
        // and allow a second launcher to start.
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
            message_box(
                &text().update_check_result_title,
                &state.check_message(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        Err(err) => {
            core::finish_update_check(None);
            message_box(
                &text().update_check_failed_title,
                &text().update_check_failed_message(&err.to_string()),
                MB_OK | MB_ICONERROR,
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
    let result = core::install_update_and_restart(&ctx).unwrap_or_else(error_result);
    if result.success {
        core::finish_update_restart_success(&state);
        if let Err(err) = restart_launcher_after_update(&ctx) {
            message_box(
                &text().update_restart_title,
                &text().update_restart_failed_message(&err.to_string()),
                MB_OK | MB_ICONERROR,
            );
        }
    } else {
        message_box(
            &text().update_restart_title,
            &text().update_restart_failed_message(&result.message),
            MB_OK | MB_ICONERROR,
        );
        core::finish_update_restart_prompt(&state);
    }
}

fn restart_launcher_after_update(ctx: &LauncherContext) -> LauncherResult<()> {
    let launcher = ps_single(&ctx.launcher_exe.to_string_lossy());
    let current_pid = std::process::id();
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$launcher = '{launcher}'
$launcherPid = {current_pid}
$previousWrite = $null
try {{ $previousWrite = (Get-Item -LiteralPath $launcher).LastWriteTimeUtc }} catch {{}}
try {{ Wait-Process -Id $launcherPid -Timeout 30 }} catch {{}}
$replaceDeadline = (Get-Date).AddSeconds(30)
while ($null -ne $previousWrite -and (Get-Date) -lt $replaceDeadline) {{
  try {{
    $currentWrite = (Get-Item -LiteralPath $launcher).LastWriteTimeUtc
    if ($currentWrite -ne $previousWrite) {{ break }}
  }} catch {{}}
  Start-Sleep -Milliseconds 500
}}
$deadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $deadline) {{
  try {{
    Start-Process -FilePath $launcher -ErrorAction Stop | Out-Null
    exit 0
  }} catch {{
    Start-Sleep -Milliseconds 500
  }}
}}
"#,
    );

    let mut command = Command::new("powershell.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to restart launcher: {err}"))?;
    std::process::exit(0);
}

fn confirm_update_restart(latest_tag: &str) -> bool {
    message_box_yes_no(
        &text().update_ready_title,
        &text().update_restart_confirm(latest_tag),
    )
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
        Ok(text().launch_at_login_disabled)
    } else {
        set_run_autostart(ctx)?;
        delete_legacy_scheduled_tasks();
        Ok(text().launch_at_login_enabled)
    }
}

fn launcher_autostart_installed() -> bool {
    run_autostart_exists()
}

fn ensure_launch_entrypoints(ctx: &LauncherContext) -> LauncherResult<()> {
    if !is_default_windows_install(&ctx.launcher_exe) {
        return Ok(());
    }

    let icon_path = ensure_launcher_icon_file(ctx)?;
    let script = windows_shortcut_script(ctx, &icon_path)?;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
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
    Err(text().command_failed(&detail).into())
}

fn is_default_windows_install(launcher_exe: &Path) -> bool {
    let Some(install_dir) = launcher_exe.parent() else {
        return false;
    };
    let Some(default_dir) = default_windows_install_dir() else {
        return false;
    };
    same_windows_path(install_dir, &default_dir)
}

fn default_windows_install_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Programs").join("AndaBot"))
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    normalize_windows_path(left).eq_ignore_ascii_case(&normalize_windows_path(right))
}

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string()
}

fn ensure_launcher_icon_file(ctx: &LauncherContext) -> LauncherResult<PathBuf> {
    let Some(install_dir) = ctx.launcher_exe.parent() else {
        return Err("could not resolve launcher install directory".into());
    };
    let icon_path = install_dir.join(LAUNCHER_ICON_FILE);
    std::fs::write(&icon_path, launcher_icon_ico())?;
    Ok(icon_path)
}

fn launcher_icon_ico() -> Vec<u8> {
    let (width, height) = png_ico_dimensions(LAUNCHER_ICON_PNG).unwrap_or((128, 128));
    let mut icon = Vec::with_capacity(22 + LAUNCHER_ICON_PNG.len());
    icon.extend_from_slice(&0u16.to_le_bytes());
    icon.extend_from_slice(&1u16.to_le_bytes());
    icon.extend_from_slice(&1u16.to_le_bytes());
    icon.extend_from_slice(&[width, height, 0, 0]);
    icon.extend_from_slice(&1u16.to_le_bytes());
    icon.extend_from_slice(&32u16.to_le_bytes());
    icon.extend_from_slice(&(LAUNCHER_ICON_PNG.len() as u32).to_le_bytes());
    icon.extend_from_slice(&22u32.to_le_bytes());
    icon.extend_from_slice(LAUNCHER_ICON_PNG);
    icon
}

fn png_ico_dimensions(png: &[u8]) -> Option<(u8, u8)> {
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some((ico_dimension_byte(width)?, ico_dimension_byte(height)?))
}

fn ico_dimension_byte(value: u32) -> Option<u8> {
    match value {
        1..=255 => Some(value as u8),
        256 => Some(0),
        _ => None,
    }
}

fn windows_shortcut_script(ctx: &LauncherContext, icon_path: &Path) -> LauncherResult<String> {
    let Some(install_dir) = ctx.launcher_exe.parent() else {
        return Err("could not resolve launcher install directory".into());
    };
    let launcher = ps_single(&ctx.launcher_exe.to_string_lossy());
    let icon = ps_single(&icon_path.to_string_lossy());
    let working_directory = ps_single(&install_dir.to_string_lossy());

    Ok(format!(
        r#"$ErrorActionPreference = 'Stop'
$launcher = '{launcher}'
$icon = '{icon}'
$workingDirectory = '{working_directory}'
$shell = New-Object -ComObject WScript.Shell
$targets = @(
  @{{ Directory = (Join-Path ([Environment]::GetFolderPath('Programs')) 'Anda Bot'); Name = 'Anda Bot.lnk' }},
  @{{ Directory = [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory); Name = 'Anda Bot.lnk' }}
)
foreach ($target in $targets) {{
  if ([string]::IsNullOrWhiteSpace($target.Directory)) {{ continue }}
  New-Item -ItemType Directory -Force -Path $target.Directory | Out-Null
  $shortcut = $shell.CreateShortcut((Join-Path $target.Directory $target.Name))
  $shortcut.TargetPath = $launcher
  $shortcut.Arguments = ''
  $shortcut.WorkingDirectory = $workingDirectory
  $shortcut.IconLocation = $icon
  $shortcut.WindowStyle = 7
  $shortcut.Save()
}}
"#
    ))
}

fn set_run_autostart(ctx: &LauncherContext) -> LauncherResult<()> {
    let command = windows_command_line([ctx.launcher_exe.as_os_str()]);
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
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = Command::new("schtasks.exe")
        .args(["/Delete", "/TN", LEGACY_LAUNCHER_TASK_NAME, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
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
    std::io::Error::other(format!(
        "Windows registry error while trying to {action}: {code} ({})",
        std::io::Error::from_raw_os_error(code as i32)
    ))
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

fn message_box_yes_no(title: &str, message: &str) -> bool {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide_null(message).as_ptr(),
            wide_null(title).as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
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

fn ps_single(value: &str) -> String {
    value.replace('\'', "''")
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
