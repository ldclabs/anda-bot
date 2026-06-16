use rust_i18n::t;
use serde::Deserialize;
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(windows)]
use std::{
    os::windows::{ffi::OsStrExt, process::CommandExt},
    ptr,
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    Globalization::GetUserDefaultLocaleName,
    System::Threading::{CreateMutexW, ReleaseMutex},
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub type LauncherResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../../assets/config.yaml");
const CODEX_API_BASE: &str = "https://chatgpt.com/backend-api/codex";
const ANDA_EXE_ENV: &str = "ANDA_EXE";
const ANDA_LAUNCHER_EXE_ENV: &str = "ANDA_LAUNCHER_EXE";
const BROWSER_EXTENSION_TOKEN_DAYS: &str = "365";
const UPDATE_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherLanguage {
    En,
    ZhHans,
    Ru,
    Ar,
    Fr,
    Es,
}

#[derive(Clone, Debug)]
pub struct LauncherContext {
    pub launcher_exe: PathBuf,
    pub anda_exe: PathBuf,
    pub home: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub family: &'static str,
    pub model: &'static str,
    pub api_base: &'static str,
    pub env_var: &'static str,
    pub bearer_auth: bool,
}

impl ProviderPreset {
    pub(crate) fn requires_api_key(&self) -> bool {
        self.api_base != CODEX_API_BASE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WizardConfig {
    pub provider_id: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherAutoUpdateState {
    pub status: String,
    pub current_tag: String,
    pub latest_tag: Option<String>,
    pub downloaded_path: Option<String>,
    pub error: Option<String>,
}

impl LauncherAutoUpdateState {
    pub fn current_tag_label(&self) -> String {
        normalize_version_tag(&self.current_tag).unwrap_or_else(current_version_tag)
    }

    pub fn downloaded_update_available(&self) -> bool {
        if self.status != "downloaded"
            || self
                .downloaded_path
                .as_deref()
                .is_none_or(|path| path.is_empty())
        {
            return false;
        }

        self.latest_tag
            .as_deref()
            .and_then(normalize_version_tag)
            .is_some_and(|latest| latest != self.current_tag_label())
    }

    pub fn latest_tag_label(&self) -> String {
        self.latest_tag
            .as_deref()
            .and_then(normalize_version_tag)
            .unwrap_or_else(|| text().latest_release)
    }

    pub fn check_message(&self) -> String {
        let copy = text();
        if self.downloaded_update_available() {
            let latest_tag = self.latest_tag_label();
            return copy.update_ready_message(&latest_tag);
        }

        match self.status.as_str() {
            "failed" => copy.update_check_failed_message(
                self.error
                    .as_deref()
                    .filter(|err| !err.trim().is_empty())
                    .unwrap_or(&copy.unknown_error),
            ),
            "checking" | "downloading" => copy.checking_update,
            "idle" => copy.update_not_checked,
            _ => copy.update_current_message(&self.current_tag_label()),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LauncherUpdateUiState {
    checking_since: Option<Instant>,
    last_state: Option<LauncherAutoUpdateState>,
    prompting_restart_tag: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherDaemonStatus {
    pub summary: String,
    pub pid: Option<String>,
    pub gateway_url: Option<String>,
    pub conversations: Option<String>,
    pub memory_nodes: Option<String>,
    pub memory_links: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct LauncherDaemonStatusJson {
    summary: Option<String>,
    pid: Option<serde_json::Value>,
    pid_file: Option<String>,
    gateway_url: Option<String>,
    conversations: Option<serde_json::Value>,
    memory_nodes: Option<serde_json::Value>,
    memory_links: Option<serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherText {
    locale: &'static str,
    latest_release: String,
    unknown_error: String,
    pub app_title: String,
    pub launcher_title: String,
    pub launcher_window_title: String,
    pub settings_title: String,
    pub setup_title: String,
    pub open_anda: String,
    pub settings: String,
    pub language: String,
    pub model_settings: String,
    pub reload_models: String,
    pub status: String,
    pub status_pid: String,
    pub status_gateway_url: String,
    pub status_conversations: String,
    pub status_memory_nodes: String,
    pub status_memory_links: String,
    pub status_checking: String,
    pub status_unavailable: String,
    pub start_daemon: String,
    pub stop_daemon: String,
    pub restart_daemon: String,
    pub browser_extension_token: String,
    pub browser_extension_token_title: String,
    pub browser_extension_token_copied: String,
    pub browser_extension_token_copy_button: String,
    pub browser_extension_token_only_copied: String,
    pub check_update: String,
    pub launch_at_login: String,
    pub disable_launch_at_login: String,
    pub open_logs: String,
    pub quit: String,
    pub ok: String,
    pub save: String,
    pub cancel: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub choose_provider_prompt: String,
    pub setup_required_message: String,
    pub launch_at_login_enabled: String,
    pub launch_at_login_disabled: String,
    pub settings_not_supported: String,
    pub unsupported_platform: String,
    pub main_thread_required: String,
    pub create_window_failed: String,
    pub resolve_launch_agents_failed: String,
    pub detect_home_failed: String,
    pub command_done: String,
    pub powershell_not_found: String,
    pub checking_update: String,
    pub update_ready_title: String,
    pub install_restart_update: String,
    pub update_check_result_title: String,
    pub update_check_failed_title: String,
    pub update_restart_title: String,
    pub update_restart_started: String,
    pub update_not_checked: String,
}

#[allow(dead_code)]
impl LauncherText {
    pub fn unsupported_provider(&self, provider_id: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.unsupported_provider",
            locale = locale,
            provider_id = provider_id
        )
        .into_owned()
    }

    pub fn unsupported_provider_from_wizard(&self, provider_id: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.unsupported_provider_from_wizard",
            locale = locale,
            provider_id = provider_id
        )
        .into_owned()
    }

    pub fn env_required(&self, env_var: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.env_required",
            locale = locale,
            env_var = env_var
        )
        .into_owned()
    }

    pub fn settings_wizard_failed(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.settings_wizard_failed",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn powershell_launch_failed(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.powershell_launch_failed",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn launcher_exe_detect_failed(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.launcher_exe_detect_failed",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn run_anda_failed(&self, path: &str, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.run_anda_failed",
            locale = locale,
            path = path,
            detail = detail
        )
        .into_owned()
    }

    pub fn command_exited(&self, status: std::process::ExitStatus) -> String {
        let locale = self.locale;
        t!(
            "launcher.command_exited",
            locale = locale,
            status = status.to_string()
        )
        .into_owned()
    }

    pub fn command_failed(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.command_failed",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn schtasks_failed(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.error.schtasks_failed",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn check_update_label(&self, current_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.check_update_versioned",
            locale = locale,
            current_tag = current_tag
        )
        .into_owned()
    }

    pub fn checking_update_label(&self, spinner: &str, current_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.checking_update_versioned",
            locale = locale,
            spinner = spinner,
            current_tag = current_tag
        )
        .into_owned()
    }

    pub fn update_downloaded_restart_message(&self, latest_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_downloaded_restart_message",
            locale = locale,
            latest_tag = latest_tag
        )
        .into_owned()
    }

    pub fn update_ready_message(&self, latest_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_ready_message",
            locale = locale,
            latest_tag = latest_tag
        )
        .into_owned()
    }

    pub fn update_restart_confirm(&self, latest_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_restart_confirm",
            locale = locale,
            latest_tag = latest_tag
        )
        .into_owned()
    }

    pub fn update_current_message(&self, current_tag: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_current_message",
            locale = locale,
            current_tag = current_tag
        )
        .into_owned()
    }

    pub fn update_check_failed_message(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_check_failed_message",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn update_restart_failed_message(&self, detail: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.update_restart_failed_message",
            locale = locale,
            detail = detail
        )
        .into_owned()
    }

    pub fn restart_recovery(&self, message: &str) -> String {
        let locale = self.locale;
        t!(
            "launcher.restart_recovery",
            locale = locale,
            message = message
        )
        .into_owned()
    }

    pub fn missing_home_arg(&self) -> String {
        let locale = self.locale;
        t!("launcher.missing_home_arg", locale = locale).into_owned()
    }
}

pub struct LauncherInstanceLock {
    #[cfg(unix)]
    file: fs::File,
    #[cfg(windows)]
    handle: HANDLE,
    #[cfg(not(any(unix, windows)))]
    _private: (),
}

impl Drop for LauncherInstanceLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }

        #[cfg(windows)]
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}

static LAUNCHER_LANGUAGE: Mutex<Option<LauncherLanguage>> = Mutex::new(None);
static UPDATE_UI_STATE: OnceLock<Mutex<LauncherUpdateUiState>> = OnceLock::new();
static DAEMON_STATUS_CACHE: OnceLock<Mutex<Option<LauncherDaemonStatus>>> = OnceLock::new();
static MENU_ACTION_GATE: Mutex<()> = Mutex::new(());

impl LauncherLanguage {
    pub const ALL: [LauncherLanguage; 6] = [
        LauncherLanguage::En,
        LauncherLanguage::ZhHans,
        LauncherLanguage::Ru,
        LauncherLanguage::Ar,
        LauncherLanguage::Fr,
        LauncherLanguage::Es,
    ];

    pub fn locale(self) -> &'static str {
        match self {
            LauncherLanguage::En => "en",
            LauncherLanguage::ZhHans => "zh-Hans",
            LauncherLanguage::Ru => "ru",
            LauncherLanguage::Ar => "ar",
            LauncherLanguage::Fr => "fr",
            LauncherLanguage::Es => "es",
        }
    }

    /// Shown in the language menu in the language itself, so every reader can
    /// find their own entry regardless of the current UI language.
    pub fn native_name(self) -> &'static str {
        match self {
            LauncherLanguage::En => "English",
            LauncherLanguage::ZhHans => "简体中文",
            LauncherLanguage::Ru => "Русский",
            LauncherLanguage::Ar => "العربية",
            LauncherLanguage::Fr => "Français",
            LauncherLanguage::Es => "Español",
        }
    }
}

pub fn text() -> LauncherText {
    text_for_language(launcher_language())
}

fn text_for_language(language: LauncherLanguage) -> LauncherText {
    let locale = language.locale();
    LauncherText {
        locale,
        latest_release: t!("launcher.latest_release", locale = locale).into_owned(),
        unknown_error: t!("launcher.unknown_error", locale = locale).into_owned(),
        app_title: t!("launcher.app_title", locale = locale).into_owned(),
        launcher_title: t!("launcher.launcher_title", locale = locale).into_owned(),
        launcher_window_title: t!("launcher.launcher_window_title", locale = locale).into_owned(),
        settings_title: t!("launcher.settings_title", locale = locale).into_owned(),
        setup_title: t!("launcher.setup_title", locale = locale).into_owned(),
        open_anda: t!("launcher.open_anda", locale = locale).into_owned(),
        settings: t!("launcher.settings", locale = locale).into_owned(),
        language: t!("launcher.language", locale = locale).into_owned(),
        model_settings: t!("launcher.model_settings", locale = locale).into_owned(),
        reload_models: t!("launcher.reload_models", locale = locale).into_owned(),
        status: t!("launcher.status", locale = locale).into_owned(),
        status_pid: t!("launcher.status_pid", locale = locale).into_owned(),
        status_gateway_url: t!("launcher.status_gateway_url", locale = locale).into_owned(),
        status_conversations: t!("launcher.status_conversations", locale = locale).into_owned(),
        status_memory_nodes: t!("launcher.status_memory_nodes", locale = locale).into_owned(),
        status_memory_links: t!("launcher.status_memory_links", locale = locale).into_owned(),
        status_checking: t!("launcher.status_checking", locale = locale).into_owned(),
        status_unavailable: t!("launcher.status_unavailable", locale = locale).into_owned(),
        start_daemon: t!("launcher.start_daemon", locale = locale).into_owned(),
        stop_daemon: t!("launcher.stop_daemon", locale = locale).into_owned(),
        restart_daemon: t!("launcher.restart_daemon", locale = locale).into_owned(),
        browser_extension_token: t!("launcher.browser_extension_token", locale = locale)
            .into_owned(),
        browser_extension_token_title: t!(
            "launcher.browser_extension_token_title",
            locale = locale
        )
        .into_owned(),
        browser_extension_token_copied: t!(
            "launcher.browser_extension_token_copied",
            locale = locale
        )
        .into_owned(),
        browser_extension_token_copy_button: t!(
            "launcher.browser_extension_token_copy_button",
            locale = locale
        )
        .into_owned(),
        browser_extension_token_only_copied: t!(
            "launcher.browser_extension_token_only_copied",
            locale = locale
        )
        .into_owned(),
        check_update: t!("launcher.check_update", locale = locale).into_owned(),
        launch_at_login: t!("launcher.launch_at_login", locale = locale).into_owned(),
        disable_launch_at_login: t!("launcher.disable_launch_at_login", locale = locale)
            .into_owned(),
        open_logs: t!("launcher.open_logs", locale = locale).into_owned(),
        quit: t!("launcher.quit", locale = locale).into_owned(),
        ok: t!("launcher.ok", locale = locale).into_owned(),
        save: t!("launcher.save", locale = locale).into_owned(),
        cancel: t!("launcher.cancel", locale = locale).into_owned(),
        provider: t!("launcher.provider", locale = locale).into_owned(),
        model: t!("launcher.model", locale = locale).into_owned(),
        api_key: t!("launcher.api_key", locale = locale).into_owned(),
        choose_provider_prompt: t!("launcher.choose_provider_prompt", locale = locale).into_owned(),
        setup_required_message: t!("launcher.setup_required_message", locale = locale).into_owned(),
        launch_at_login_enabled: t!("launcher.launch_at_login_enabled", locale = locale)
            .into_owned(),
        launch_at_login_disabled: t!("launcher.launch_at_login_disabled", locale = locale)
            .into_owned(),
        settings_not_supported: t!("launcher.settings_not_supported", locale = locale).into_owned(),
        unsupported_platform: t!("launcher.unsupported_platform", locale = locale).into_owned(),
        main_thread_required: t!("launcher.main_thread_required", locale = locale).into_owned(),
        create_window_failed: t!("launcher.create_window_failed", locale = locale).into_owned(),
        resolve_launch_agents_failed: t!("launcher.resolve_launch_agents_failed", locale = locale)
            .into_owned(),
        detect_home_failed: t!("launcher.detect_home_failed", locale = locale).into_owned(),
        command_done: t!("launcher.command_done", locale = locale).into_owned(),
        powershell_not_found: t!("launcher.powershell_not_found", locale = locale).into_owned(),
        checking_update: t!("launcher.checking_update", locale = locale).into_owned(),
        update_ready_title: t!("launcher.update_ready_title", locale = locale).into_owned(),
        install_restart_update: t!("launcher.install_restart_update", locale = locale).into_owned(),
        update_check_result_title: t!("launcher.update_check_result_title", locale = locale)
            .into_owned(),
        update_check_failed_title: t!("launcher.update_check_failed_title", locale = locale)
            .into_owned(),
        update_restart_title: t!("launcher.update_restart_title", locale = locale).into_owned(),
        update_restart_started: t!("launcher.update_restart_started", locale = locale).into_owned(),
        update_not_checked: t!("launcher.update_not_checked", locale = locale).into_owned(),
    }
}

pub fn launcher_language() -> LauncherLanguage {
    if let Some(language) = *lock_launcher_language() {
        return language;
    }

    // Resolved outside the lock: the home-detection fallback path calls
    // text(), which would re-enter this function and deadlock.
    let language = initial_launcher_language();
    *lock_launcher_language().get_or_insert(language)
}

pub fn set_launcher_language(home: &Path, language: LauncherLanguage) -> LauncherResult<()> {
    persist_launcher_language(home, language)?;
    *lock_launcher_language() = Some(language);
    Ok(())
}

fn lock_launcher_language() -> std::sync::MutexGuard<'static, Option<LauncherLanguage>> {
    LAUNCHER_LANGUAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn initial_launcher_language() -> LauncherLanguage {
    detect_anda_home()
        .ok()
        .and_then(|home| load_persisted_language(&home))
        .unwrap_or_else(detect_launcher_language)
}

fn ui_settings_path(home: &Path) -> PathBuf {
    home.join("launcher").join("ui.json")
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LauncherUiSettings {
    language: String,
}

pub fn load_persisted_language(home: &Path) -> Option<LauncherLanguage> {
    let content = fs::read_to_string(ui_settings_path(home)).ok()?;
    let settings = serde_json::from_str::<LauncherUiSettings>(&content).ok()?;
    language_from_tag(&settings.language)
}

fn persist_launcher_language(home: &Path, language: LauncherLanguage) -> LauncherResult<()> {
    let path = ui_settings_path(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&serde_json::json!({
        "language": language.locale(),
    }))?;
    fs::write(path, content)?;
    Ok(())
}

fn detect_launcher_language() -> LauncherLanguage {
    language_from_tags(system_locale_tags().iter().map(String::as_str))
}

fn language_from_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> LauncherLanguage {
    for tag in tags {
        if let Some(language) = language_from_tag(tag) {
            return language;
        }
    }
    LauncherLanguage::En
}

fn language_from_tag(tag: &str) -> Option<LauncherLanguage> {
    let normalized = tag
        .trim()
        .trim_matches('"')
        .split('.')
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase();

    if normalized.starts_with("zh") || normalized.contains("chinese") {
        Some(LauncherLanguage::ZhHans)
    } else if normalized.starts_with("en") {
        Some(LauncherLanguage::En)
    } else if normalized.starts_with("ru") {
        Some(LauncherLanguage::Ru)
    } else if normalized.starts_with("ar") {
        Some(LauncherLanguage::Ar)
    } else if normalized.starts_with("fr") {
        Some(LauncherLanguage::Fr)
    } else if normalized.starts_with("es") {
        Some(LauncherLanguage::Es)
    } else {
        None
    }
}

fn system_locale_tags() -> Vec<String> {
    let mut tags = platform_locale_tags();
    tags.extend(environment_locale_tags());
    tags
}

#[cfg(target_os = "macos")]
fn platform_locale_tags() -> Vec<String> {
    let mut tags = macos_defaults_languages();
    if let Some(locale) = macos_defaults_value("AppleLocale") {
        tags.push(locale);
    }
    tags
}

#[cfg(target_os = "macos")]
fn macos_defaults_languages() -> Vec<String> {
    let Some(output) = macos_defaults_value("AppleLanguages") else {
        return Vec::new();
    };

    output
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .trim_end_matches(',')
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_defaults_value(key: &str) -> Option<String> {
    let output = Command::new("defaults")
        .arg("read")
        .arg("-g")
        .arg(key)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
fn platform_locale_tags() -> Vec<String> {
    let mut buffer = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 1 {
        return Vec::new();
    }
    vec![String::from_utf16_lossy(&buffer[..(len as usize - 1)])]
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_locale_tags() -> Vec<String> {
    Vec::new()
}

fn environment_locale_tags() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
        .collect()
}

pub const PROVIDERS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "deepseek",
        label: "DeepSeek (recommended)",
        family: "anthropic",
        model: "deepseek-v4-pro",
        api_base: "https://api.deepseek.com/anthropic",
        env_var: "DEEPSEEK_API_KEY",
        bearer_auth: false,
    },
    ProviderPreset {
        id: "openai",
        label: "OpenAI",
        family: "openai",
        model: "gpt-5.4",
        api_base: "https://api.openai.com/v1",
        env_var: "OPENAI_API_KEY",
        bearer_auth: false,
    },
    ProviderPreset {
        id: "codex",
        label: "Codex (ChatGPT login)",
        family: "openai",
        model: "gpt-5.5",
        api_base: CODEX_API_BASE,
        env_var: "CODEX_AUTH_JSON",
        bearer_auth: false,
    },
    ProviderPreset {
        id: "anthropic",
        label: "Anthropic",
        family: "anthropic",
        model: "claude-opus-4-7",
        api_base: "https://api.anthropic.com/v1",
        env_var: "ANTHROPIC_API_KEY",
        bearer_auth: false,
    },
    ProviderPreset {
        id: "gemini",
        label: "Gemini",
        family: "gemini",
        model: "gemini-pro-latest",
        api_base: "https://generativelanguage.googleapis.com/v1beta/models",
        env_var: "GEMINI_API_KEY",
        bearer_auth: false,
    },
];

impl LauncherContext {
    pub fn detect() -> LauncherResult<Self> {
        let detected_launcher_exe = env::current_exe()
            .map_err(|err| text().launcher_exe_detect_failed(&err.to_string()))?;
        let launcher_exe = env::var_os(ANDA_LAUNCHER_EXE_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or(detected_launcher_exe);
        let anda_exe = detect_anda_exe(&launcher_exe);
        let home = detect_anda_home()?;
        Ok(Self {
            launcher_exe,
            anda_exe,
            home,
        })
    }

    pub fn config_path(&self) -> PathBuf {
        self.home.join("config.yaml")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("logs")
    }
}

pub fn provider_by_id(id: &str) -> Option<&'static ProviderPreset> {
    PROVIDERS.iter().find(|provider| provider.id == id.trim())
}

pub fn default_provider() -> &'static ProviderPreset {
    &PROVIDERS[0]
}

pub fn provider_ids() -> Vec<&'static str> {
    PROVIDERS.iter().map(|provider| provider.id).collect()
}

pub fn default_model_for_provider(provider_id: &str) -> &'static str {
    provider_by_id(provider_id)
        .unwrap_or_else(default_provider)
        .model
}

pub fn acquire_launcher_instance_lock() -> LauncherResult<Option<LauncherInstanceLock>> {
    #[cfg(unix)]
    {
        acquire_unix_launcher_instance_lock()
    }

    #[cfg(windows)]
    {
        acquire_windows_launcher_instance_lock()
    }

    #[cfg(not(any(unix, windows)))]
    {
        Ok(Some(LauncherInstanceLock { _private: () }))
    }
}

#[cfg(unix)]
fn acquire_unix_launcher_instance_lock() -> LauncherResult<Option<LauncherInstanceLock>> {
    let lock_path = env::temp_dir().join(format!("ai.anda.anda-bot.launcher.{}.lock", unsafe {
        libc::geteuid()
    }));
    #[allow(clippy::suspicious_open_options)]
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)?;

    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(LauncherInstanceLock { file }));
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
    {
        return Ok(None);
    }
    Err(err.into())
}

#[cfg(windows)]
fn acquire_windows_launcher_instance_lock() -> LauncherResult<Option<LauncherInstanceLock>> {
    let name = wide_null_os("Local\\AndaBotLauncher");
    let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error().into());
    }

    let last_error = unsafe { GetLastError() };
    if last_error == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }
        return Ok(None);
    }

    Ok(Some(LauncherInstanceLock { handle }))
}

#[cfg(windows)]
fn wide_null_os(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

pub fn config_needs_setup(ctx: &LauncherContext) -> bool {
    if ensure_config_file_exists(ctx).is_err() {
        return false;
    }

    match fs::read_to_string(ctx.config_path()) {
        Ok(content) => parsed_config_needs_setup(&content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

pub fn write_minimal_config(ctx: &LauncherContext, wizard: &WizardConfig) -> LauncherResult<()> {
    update_model_config(ctx, wizard)
}

pub fn write_initial_minimal_config(
    ctx: &LauncherContext,
    wizard: &WizardConfig,
) -> LauncherResult<()> {
    update_model_config(ctx, wizard)
}

fn update_model_config(ctx: &LauncherContext, wizard: &WizardConfig) -> LauncherResult<()> {
    let provider = provider_by_id(&wizard.provider_id)
        .ok_or_else(|| text().unsupported_provider(&wizard.provider_id))?;
    let model = normalize_non_empty(&wizard.model).unwrap_or_else(|| provider.model.to_string());
    let api_key = normalize_non_empty(&wizard.api_key);
    if provider.requires_api_key() && api_key.is_none() {
        return Err(text().env_required(provider.env_var).into());
    }

    ensure_config_file_exists(ctx)?;
    let config_path = ctx.config_path();
    let content = fs::read_to_string(&config_path)?;
    let updated = apply_model_config_update(
        if content.trim().is_empty() {
            DEFAULT_CONFIG_TEMPLATE
        } else {
            &content
        },
        &ModelConfigUpdate {
            provider,
            model: &model,
            api_key: api_key.as_deref().unwrap_or_default(),
        },
    );

    if content != updated {
        backup_config_file(&config_path)?;
        write_config_atomic(&config_path, &updated)?;
    }
    Ok(())
}

pub fn start_daemon(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["start"])
}

pub fn stop_daemon(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["stop"])
}

pub fn restart_daemon(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["restart"])
}

pub fn reload_models(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["models", "reload"])
}

pub fn reload_models_or_start_daemon(ctx: &LauncherContext) -> CommandResult {
    let reload = reload_models(ctx).unwrap_or_else(command_error_result);
    if reload.success {
        return reload;
    }

    match daemon_status_json(ctx) {
        Ok(status) if status.success => reload,
        _ => start_daemon(ctx).unwrap_or(reload),
    }
}

fn command_error_result(err: Box<dyn std::error::Error + Send + Sync>) -> CommandResult {
    CommandResult {
        success: false,
        message: err.to_string(),
    }
}

pub fn daemon_status(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["status"])
}

pub fn daemon_status_json(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["status", "--json"])
}

pub fn cached_daemon_status() -> LauncherDaemonStatus {
    lock_daemon_status_cache()
        .clone()
        .unwrap_or_else(|| LauncherDaemonStatus {
            summary: text().status_checking,
            pid: None,
            gateway_url: None,
            conversations: None,
            memory_nodes: None,
            memory_links: None,
        })
}

pub fn refresh_daemon_status_cache(ctx: &LauncherContext) -> LauncherDaemonStatus {
    let result = daemon_status_for_launcher(ctx);
    let status = launcher_daemon_status_from_command_result(&result);
    *lock_daemon_status_cache() = Some(status.clone());
    status
}

pub fn daemon_status_poll_interval() -> Duration {
    Duration::from_secs(15)
}

pub fn generate_browser_extension_token(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &browser_extension_token_args())
}

fn browser_extension_token_args() -> [&'static str; 4] {
    ["browser", "token", "--days", BROWSER_EXTENSION_TOKEN_DAYS]
}

pub fn browser_extension_bearer_token(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Bearer token:")
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub fn current_version_tag() -> String {
    normalize_version_tag(env!("CARGO_PKG_VERSION")).unwrap_or_else(|| "v0.0.0".to_string())
}

pub fn check_update_menu_label() -> String {
    let state = lock_update_ui_state().clone();
    let current_tag = state
        .last_state
        .as_ref()
        .map(LauncherAutoUpdateState::current_tag_label)
        .unwrap_or_else(current_version_tag);

    if let Some(checking_since) = state.checking_since {
        return text().checking_update_label(spinner_frame(checking_since), &current_tag);
    }

    if let Some(update_state) = state
        .last_state
        .as_ref()
        .filter(|state| state.downloaded_update_available())
    {
        return text().update_downloaded_restart_message(&update_state.latest_tag_label());
    }

    text().check_update_label(&current_tag)
}

/// Serializes user-triggered menu actions (daemon restart, settings wizard,
/// token generation) so rapid repeated clicks cannot run concurrent `anda`
/// commands or stack result dialogs. Returns `None` while another action is
/// still running.
pub fn try_begin_menu_action() -> Option<std::sync::MutexGuard<'static, ()>> {
    match MENU_ACTION_GATE.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Blocking variant for the startup setup path, which must always run.
pub fn begin_menu_action() -> std::sync::MutexGuard<'static, ()> {
    MENU_ACTION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn begin_update_check() -> bool {
    let mut state = lock_update_ui_state();
    if state.checking_since.is_some() {
        return false;
    }
    state.checking_since = Some(Instant::now());
    true
}

pub fn finish_update_check(state: Option<LauncherAutoUpdateState>) {
    let mut ui_state = lock_update_ui_state();
    ui_state.checking_since = None;
    if let Some(state) = state {
        ui_state.last_state = Some(state);
    }
}

pub fn downloaded_update_state() -> Option<LauncherAutoUpdateState> {
    lock_update_ui_state()
        .last_state
        .as_ref()
        .filter(|state| state.downloaded_update_available())
        .cloned()
}

pub fn begin_update_restart_prompt(state: &LauncherAutoUpdateState) -> bool {
    let mut ui_state = lock_update_ui_state();
    if ui_state.prompting_restart_tag.is_some() {
        return false;
    }
    ui_state.prompting_restart_tag = Some(state.latest_tag_label());
    true
}

pub fn finish_update_restart_prompt(state: &LauncherAutoUpdateState) {
    let tag = state.latest_tag_label();
    let mut ui_state = lock_update_ui_state();
    if ui_state.prompting_restart_tag.as_deref() == Some(tag.as_str()) {
        ui_state.prompting_restart_tag = None;
    }
}

pub fn finish_update_restart_success(state: &LauncherAutoUpdateState) {
    let tag = state.latest_tag_label();
    let mut ui_state = lock_update_ui_state();
    if ui_state.prompting_restart_tag.as_deref() == Some(tag.as_str()) {
        ui_state.prompting_restart_tag = None;
    }
    ui_state.last_state = Some(LauncherAutoUpdateState {
        status: "installed".to_string(),
        current_tag: tag,
        latest_tag: state.latest_tag.clone(),
        downloaded_path: None,
        error: None,
    });
}

pub fn check_update_now(ctx: &LauncherContext) -> LauncherResult<LauncherAutoUpdateState> {
    run_update_check(ctx, true)
}

pub fn check_update_if_due(ctx: &LauncherContext) -> LauncherResult<LauncherAutoUpdateState> {
    run_update_check(ctx, false)
}

pub fn install_update_and_restart(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    let stop = stop_daemon(ctx)?;
    if !stop.success {
        return Ok(stop);
    }

    let update = run_anda(ctx, &["update"])?;
    if !update.success {
        let recovery = start_daemon(ctx).unwrap_or_else(|err| CommandResult {
            success: false,
            message: err.to_string(),
        });
        return Ok(combine_command_results(
            update,
            CommandResult {
                success: false,
                message: text().restart_recovery(&recovery.message),
            },
        ));
    }

    thread::sleep(Duration::from_secs(2));
    let start = start_daemon(ctx)?;
    Ok(combine_command_results(update, start))
}

pub fn auto_update_poll_interval() -> Duration {
    Duration::from_secs(6 * 60 * 60)
}

pub fn run_anda(ctx: &LauncherContext, args: &[&str]) -> LauncherResult<CommandResult> {
    let mut command = Command::new(&ctx.anda_exe);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command.args(args);
    let output = command
        .output()
        .map_err(|err| text().run_anda_failed(&ctx.anda_exe.to_string_lossy(), &err.to_string()))?;
    Ok(command_result(output))
}

fn run_update_check(ctx: &LauncherContext, force: bool) -> LauncherResult<LauncherAutoUpdateState> {
    let args = if force {
        ["update", "--check", "--json"]
    } else {
        ["update", "--check-if-due", "--json"]
    };
    let result = run_anda(ctx, &args)?;
    if !result.success {
        return Err(result.message.into());
    }
    serde_json::from_str::<LauncherAutoUpdateState>(&result.message).map_err(|err| {
        text()
            .command_failed(&format!("invalid update state JSON: {err}"))
            .into()
    })
}

pub fn command_result(output: Output) -> CommandResult {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if !stdout.is_empty() {
        stdout
    } else if !stderr.is_empty() {
        stderr
    } else if output.status.success() {
        text().command_done
    } else {
        text().command_exited(output.status)
    };
    CommandResult {
        success: output.status.success(),
        message,
    }
}

fn combine_command_results(first: CommandResult, second: CommandResult) -> CommandResult {
    let message = match (
        first.message.trim().is_empty(),
        second.message.trim().is_empty(),
    ) {
        (true, true) => text().command_done,
        (false, true) => first.message,
        (true, false) => second.message,
        (false, false) => format!("{}\n{}", first.message, second.message),
    };
    CommandResult {
        success: first.success && second.success,
        message,
    }
}

fn lock_update_ui_state() -> std::sync::MutexGuard<'static, LauncherUpdateUiState> {
    UPDATE_UI_STATE
        .get_or_init(|| Mutex::new(LauncherUpdateUiState::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_daemon_status_cache() -> std::sync::MutexGuard<'static, Option<LauncherDaemonStatus>> {
    DAEMON_STATUS_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn daemon_status_for_launcher(ctx: &LauncherContext) -> CommandResult {
    match daemon_status_json(ctx) {
        Ok(result) if result.success => result,
        Ok(result) => daemon_status(ctx).unwrap_or(result),
        Err(err) => CommandResult {
            success: false,
            message: err.to_string(),
        },
    }
}

fn launcher_daemon_status_from_command_result(result: &CommandResult) -> LauncherDaemonStatus {
    let message = result.message.trim();
    if result.success
        && let Some(status) = launcher_daemon_status_from_json(message)
    {
        return status;
    }

    let summary = message
        .lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty()).then_some(line.to_string())
        })
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| text().status_unavailable);

    if !result.success {
        return LauncherDaemonStatus {
            summary,
            pid: None,
            gateway_url: None,
            conversations: None,
            memory_nodes: None,
            memory_links: None,
        };
    }

    LauncherDaemonStatus {
        summary,
        pid: parse_daemon_status_pid(message),
        gateway_url: parse_daemon_status_gateway_url(message),
        conversations: parse_daemon_status_value(message, "Conversations:"),
        memory_nodes: parse_daemon_status_value(message, "Memory nodes:"),
        memory_links: parse_daemon_status_value(message, "Memory links:"),
    }
}

fn launcher_daemon_status_from_json(message: &str) -> Option<LauncherDaemonStatus> {
    let status = serde_json::from_str::<LauncherDaemonStatusJson>(message).ok()?;
    let summary = status
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| text().status_unavailable);
    let pid = json_status_value(status.pid).or_else(|| {
        status
            .pid_file
            .as_deref()
            .map(str::trim)
            .filter(|pid_file| !pid_file.is_empty())
            .map(ToOwned::to_owned)
    });

    Some(LauncherDaemonStatus {
        summary,
        pid,
        gateway_url: non_empty_json_string(status.gateway_url),
        conversations: json_status_value(status.conversations),
        memory_nodes: json_status_value(status.memory_nodes),
        memory_links: json_status_value(status.memory_links),
    })
}

fn json_status_value(value: Option<serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(value) => non_empty_json_string(Some(value)),
        _ => None,
    }
}

fn non_empty_json_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_daemon_status_pid(message: &str) -> Option<String> {
    for line in message.lines().map(str::trim) {
        if let Some(pid) = line
            .split_once("(pid ")
            .and_then(|(_, rest)| rest.split(')').next())
            .map(str::trim)
            .filter(|pid| pid.chars().all(|ch| ch.is_ascii_digit()))
            .filter(|pid| !pid.is_empty())
        {
            return Some(pid.to_string());
        }

        if let Some(pid_file) = line
            .strip_prefix("PID file:")
            .map(str::trim)
            .filter(|pid| !pid.is_empty())
        {
            return Some(pid_file.to_string());
        }
    }
    None
}

fn parse_daemon_status_gateway_url(message: &str) -> Option<String> {
    parse_daemon_status_value(message, "Gateway URL:")
}

fn parse_daemon_status_value(message: &str, label: &str) -> Option<String> {
    message.lines().find_map(|line| {
        line.trim()
            .strip_prefix(label)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn normalize_version_tag(tag: &str) -> Option<String> {
    let tag = tag.trim();
    if tag.is_empty() {
        None
    } else if tag.starts_with('v') || tag.starts_with('V') {
        Some(format!(
            "v{}",
            tag[1..].trim_start_matches('v').trim_start_matches('V')
        ))
    } else {
        Some(format!("v{tag}"))
    }
}

fn spinner_frame(since: Instant) -> &'static str {
    let frame = (since.elapsed().as_millis() / 200) as usize % UPDATE_SPINNER_FRAMES.len();
    UPDATE_SPINNER_FRAMES[frame]
}

fn detect_anda_exe(launcher_exe: &Path) -> PathBuf {
    if let Some(anda_exe) = env::var_os(ANDA_EXE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return anda_exe;
    }
    detect_anda_exe_from_candidates(launcher_exe, fallback_anda_exe_candidates())
}

fn detect_anda_exe_from_candidates(
    launcher_exe: &Path,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> PathBuf {
    let exe_name = if cfg!(windows) { "anda.exe" } else { "anda" };
    if let Some(parent) = launcher_exe.parent() {
        let sibling = parent.join(exe_name);
        if sibling.exists() {
            return sibling;
        }
    }
    for candidate in candidates {
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(exe_name)
}

fn fallback_anda_exe_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/bin/anda"));
        }
        candidates.push(PathBuf::from("/opt/homebrew/bin/anda"));
        candidates.push(PathBuf::from("/usr/local/bin/anda"));
    }
    candidates
}

fn detect_anda_home() -> LauncherResult<PathBuf> {
    if let Some(home) = home_arg_from_args(env::args_os().skip(1))? {
        return Ok(home);
    }
    if let Some(home) = env::var_os("ANDA_HOME") {
        return Ok(PathBuf::from(home));
    }
    let user_home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .ok_or_else(|| text().detect_home_failed)?;
    Ok(PathBuf::from(user_home).join(".anda"))
}

fn home_arg_from_args<I>(args: I) -> LauncherResult<Option<PathBuf>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == OsStr::new("--home") {
            let Some(home) = args.next() else {
                return Err(text().missing_home_arg().into());
            };
            if home.as_os_str().is_empty() {
                return Err(text().missing_home_arg().into());
            }
            return Ok(Some(PathBuf::from(home)));
        }

        if let Some(home) = arg.to_str().and_then(|value| value.strip_prefix("--home=")) {
            if home.is_empty() {
                return Err(text().missing_home_arg().into());
            }
            return Ok(Some(PathBuf::from(home)));
        }
    }

    Ok(None)
}

fn ensure_config_file_exists(ctx: &LauncherContext) -> LauncherResult<bool> {
    let config_path = ctx.config_path();
    match fs::metadata(&config_path) {
        Ok(_) => Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(&ctx.home)?;
            write_config_atomic(&config_path, DEFAULT_CONFIG_TEMPLATE)?;
            Ok(true)
        }
        Err(err) => Err(err.into()),
    }
}

/// Writes through a sibling temp file and rename so a crash mid-write cannot
/// leave a truncated config.yaml behind (a partial file would either break the
/// daemon or silently suppress the setup wizard).
fn write_config_atomic(path: &Path, content: &str) -> io::Result<()> {
    let mut tmp_name = path
        .file_name()
        .unwrap_or_else(|| OsStr::new("config.yaml"))
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = path.with_file_name(tmp_name);

    let result = fs::File::create(&tmp_path)
        .and_then(|mut file| {
            file.write_all(content.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&tmp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn backup_config_file(config_path: &Path) -> LauncherResult<PathBuf> {
    let backup_path = unique_backup_path(config_path);
    fs::copy(config_path, &backup_path)?;
    Ok(backup_path)
}

fn unique_backup_path(config_path: &Path) -> PathBuf {
    let name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.yaml");
    let backup_path = config_path.with_file_name(format!("{name}.bak"));
    if !backup_path.exists() {
        return backup_path;
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    for attempt in 0..1000 {
        let backup_path = config_path.with_file_name(format!("{name}.{stamp}.{attempt}.bak"));
        if !backup_path.exists() {
            return backup_path;
        }
    }
    config_path.with_file_name(format!("{name}.{stamp}.bak"))
}

fn parsed_config_needs_setup(content: &str) -> bool {
    parse_existing_config(content)
        .map(|existing| existing.needs_setup())
        .unwrap_or(false)
}

struct ModelConfigUpdate<'a> {
    provider: &'static ProviderPreset,
    model: &'a str,
    api_key: &'a str,
}

fn apply_model_config_update(content: &str, update: &ModelConfigUpdate<'_>) -> String {
    let mut lines = content
        .trim_end_matches(['\r', '\n'])
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();

    if lines.is_empty() {
        lines = DEFAULT_CONFIG_TEMPLATE
            .trim_end_matches(['\r', '\n'])
            .split('\n')
            .map(|line| line.to_string())
            .collect();
    }

    apply_model_config_update_to_lines(&mut lines, update);
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn apply_model_config_update_to_lines(lines: &mut Vec<String>, update: &ModelConfigUpdate<'_>) {
    let Some(model_idx) = find_top_level_key(lines, "model") else {
        append_model_section(lines, update);
        return;
    };

    let model_end = find_section_end(lines, model_idx, 0);
    upsert_scalar_line(lines, model_idx + 1, model_end, 2, "active", update.model);

    let model_end = find_section_end(lines, model_idx, 0);
    let Some(providers_idx) = find_key_in_range(lines, model_idx + 1, model_end, 2, "providers")
    else {
        insert_providers_section(lines, model_idx, update);
        return;
    };

    let providers_end = find_section_end(lines, providers_idx, 2);
    if let Some((item_start, item_end)) =
        find_provider_item(lines, providers_idx + 1, providers_end, update)
    {
        update_provider_item(lines, item_start, item_end, update);
    } else {
        insert_provider_item(lines, providers_end, update);
    }
}

fn append_model_section(lines: &mut Vec<String>, update: &ModelConfigUpdate<'_>) {
    if !lines.last().is_none_or(|line| line.trim().is_empty()) {
        lines.push(String::new());
    }
    lines.push("model:".to_string());
    lines.push(format!("  active: {}", yaml_string(update.model)));
    lines.push("  providers:".to_string());
    insert_provider_item(lines, lines.len(), update);
}

fn insert_providers_section(
    lines: &mut Vec<String>,
    model_idx: usize,
    update: &ModelConfigUpdate<'_>,
) {
    let model_end = find_section_end(lines, model_idx, 0);
    lines.insert(model_end, "  providers:".to_string());
    insert_provider_item(lines, model_end + 1, update);
}

fn update_provider_item(
    lines: &mut Vec<String>,
    item_start: usize,
    item_end: usize,
    update: &ModelConfigUpdate<'_>,
) {
    upsert_scalar_line(
        lines,
        item_start,
        item_end,
        6,
        "family",
        update.provider.family,
    );
    let item_end = find_item_end(lines, item_start);
    upsert_scalar_line(lines, item_start, item_end, 6, "model", update.model);
    let item_end = find_item_end(lines, item_start);
    upsert_scalar_line(
        lines,
        item_start,
        item_end,
        6,
        "api_base",
        update.provider.api_base,
    );
    let item_end = find_item_end(lines, item_start);
    upsert_scalar_line(lines, item_start, item_end, 6, "api_key", update.api_key);
    let item_end = find_item_end(lines, item_start);
    upsert_bool_line(lines, item_start, item_end, 6, "disabled", false);
    let item_end = find_item_end(lines, item_start);
    if update.provider.bearer_auth
        || find_key_in_range(lines, item_start, item_end, 6, "bearer_auth").is_some()
    {
        upsert_bool_line(
            lines,
            item_start,
            item_end,
            6,
            "bearer_auth",
            update.provider.bearer_auth,
        );
    }
}

fn insert_provider_item(lines: &mut Vec<String>, index: usize, update: &ModelConfigUpdate<'_>) {
    let mut item = vec![
        format!(
            "    - family: {}",
            yaml_bare_or_string(update.provider.family)
        ),
        format!("      model: {}", yaml_string(update.model)),
        format!("      api_base: {}", yaml_string(update.provider.api_base)),
        format!("      api_key: {}", yaml_string(update.api_key)),
        "      effort: high".to_string(),
        "      context_window: 400000".to_string(),
        "      max_output: 128000".to_string(),
        "      labels: [\"memory\", \"image\", \"audio\", \"video\"]".to_string(),
        "      disabled: false".to_string(),
    ];
    if update.provider.bearer_auth {
        item.push("      bearer_auth: true".to_string());
    }
    lines.splice(index..index, item);
}

fn upsert_scalar_line(
    lines: &mut Vec<String>,
    start: usize,
    end: usize,
    indent: usize,
    key: &str,
    value: &str,
) {
    if let Some(idx) = find_key_in_range(lines, start, end, indent, key) {
        lines[idx] = replace_yaml_value(&lines[idx], indent, key, &yaml_string(value));
    } else {
        lines.insert(
            end,
            format!("{}{}: {}", " ".repeat(indent), key, yaml_string(value)),
        );
    }
}

fn upsert_bool_line(
    lines: &mut Vec<String>,
    start: usize,
    end: usize,
    indent: usize,
    key: &str,
    value: bool,
) {
    let value = if value { "true" } else { "false" };
    if let Some(idx) = find_key_in_range(lines, start, end, indent, key) {
        lines[idx] = replace_yaml_value(&lines[idx], indent, key, value);
    } else {
        lines.insert(end, format!("{}{}: {}", " ".repeat(indent), key, value));
    }
}

fn replace_yaml_value(line: &str, indent: usize, key: &str, value: &str) -> String {
    let prefix = format!("{}{}:", " ".repeat(indent), key);
    let comment = yaml_inline_comment(line).unwrap_or("");
    if comment.is_empty() {
        format!("{prefix} {value}")
    } else {
        format!("{prefix} {value} {comment}")
    }
}

fn yaml_inline_comment(line: &str) -> Option<&str> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return Some(line[idx..].trim()),
            _ => {}
        }
    }
    None
}

fn find_top_level_key(lines: &[String], key: &str) -> Option<usize> {
    find_key_in_range(lines, 0, lines.len(), 0, key)
}

fn find_key_in_range(
    lines: &[String],
    start: usize,
    end: usize,
    indent: usize,
    key: &str,
) -> Option<usize> {
    let prefix = format!("{}{}:", " ".repeat(indent), key);
    (start..end.min(lines.len())).find(|&idx| {
        let line = &lines[idx];
        !line.trim_start().starts_with('#') && line.starts_with(&prefix)
    })
}

fn find_section_end(lines: &[String], section_start: usize, indent: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().skip(section_start + 1) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if leading_spaces(line) <= indent {
            return idx;
        }
    }
    lines.len()
}

fn find_provider_item(
    lines: &[String],
    start: usize,
    end: usize,
    update: &ModelConfigUpdate<'_>,
) -> Option<(usize, usize)> {
    let mut idx = start;
    while idx < end {
        if is_provider_item_start(&lines[idx]) {
            let item_end = find_item_end_bounded(lines, idx, end);
            let item = parse_provider_item(lines, idx, item_end);
            if item.api_base.as_deref() == Some(update.provider.api_base)
                || item.model.as_deref() == Some(update.provider.model)
                || item.model.as_deref() == Some(update.model)
            {
                return Some((idx, item_end));
            }
            idx = item_end;
        } else {
            idx += 1;
        }
    }
    None
}

fn find_item_end(lines: &[String], item_start: usize) -> usize {
    find_item_end_bounded(lines, item_start, lines.len())
}

fn find_item_end_bounded(lines: &[String], item_start: usize, bound: usize) -> usize {
    for (idx, line) in lines.iter().enumerate().take(bound).skip(item_start + 1) {
        if is_provider_item_start(line) || (is_content_line(line) && leading_spaces(line) <= 2) {
            return idx;
        }
    }
    bound
}

fn is_provider_item_start(line: &str) -> bool {
    line.starts_with("    - ")
}

fn is_content_line(line: &str) -> bool {
    !line.trim().is_empty() && !line.trim_start().starts_with('#')
}

#[derive(Default)]
struct ExistingProviderItem {
    model: Option<String>,
    api_base: Option<String>,
}

fn parse_provider_item(lines: &[String], start: usize, end: usize) -> ExistingProviderItem {
    let mut item = ExistingProviderItem::default();
    for line in &lines[start..end] {
        if let Some(value) = parse_yaml_field(line, "model") {
            item.model = Some(value);
        } else if let Some(value) = parse_yaml_field(line, "api_base") {
            item.api_base = Some(value);
        }
    }
    item
}

fn parse_yaml_field(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    let value = trimmed.strip_prefix(&format!("{key}:"))?;
    Some(unquote_yaml_scalar(strip_inline_comment(value).trim()))
}

fn strip_inline_comment(value: &str) -> &str {
    let comment = yaml_inline_comment(value);
    if let Some(comment) = comment {
        &value[..value.len() - comment.len()]
    } else {
        value
    }
}

fn unquote_yaml_scalar(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        value[1..value.len() - 1].replace("''", "'")
    } else {
        value.to_string()
    }
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

#[cfg(test)]
fn render_minimal_config(provider: &ProviderPreset, model: &str, api_key: &str) -> String {
    let mut config = String::new();
    config.push_str("addr: 127.0.0.1:8042\n");
    config.push_str("log_level: warn\n\n");
    config.push_str("model:\n");
    config.push_str(&format!("  active: {}\n", yaml_string(model)));
    config.push_str("  providers:\n");
    config.push_str(&format!("    - family: {}\n", yaml_string(provider.family)));
    config.push_str(&format!("      model: {}\n", yaml_string(model)));
    config.push_str(&format!(
        "      api_base: {}\n",
        yaml_string(provider.api_base)
    ));
    config.push_str(&format!("      api_key: {}\n", yaml_string(api_key)));
    config.push_str("      effort: high\n");
    config.push_str("      context_window: 400000\n");
    config.push_str("      max_output: 128000\n");
    config.push_str("      labels: [\"memory\", \"image\", \"audio\", \"video\"]\n");
    config.push_str("      disabled: false\n");
    if provider.bearer_auth {
        config.push_str("      bearer_auth: true\n");
    }
    config
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn yaml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn yaml_bare_or_string(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        value.to_string()
    } else {
        yaml_string(value)
    }
}

#[derive(Debug, Default, Deserialize)]
struct ExistingConfig {
    #[serde(default)]
    model: ExistingModel,
}

impl ExistingConfig {
    fn needs_setup(&self) -> bool {
        let active = self.model.active.trim();
        if active.is_empty() {
            return true;
        }

        let Some(provider) = self
            .model
            .providers
            .iter()
            .find(|provider| provider.model.trim() == active)
        else {
            return true;
        };

        provider.family.trim().is_empty()
            || provider.model.trim().is_empty()
            || provider.api_base.trim().is_empty()
            || !provider_has_auth(provider)
    }
}

#[derive(Debug, Default, Deserialize)]
struct ExistingModel {
    #[serde(default)]
    active: String,
    #[serde(default)]
    providers: Vec<ExistingProvider>,
}

#[derive(Debug, Default, Deserialize)]
struct ExistingProvider {
    #[serde(default)]
    family: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    api_base: String,
    #[serde(default)]
    api_key: String,
}

fn parse_existing_config(content: &str) -> LauncherResult<ExistingConfig> {
    if content.trim().is_empty() {
        return Ok(ExistingConfig::default());
    }
    Ok(serde_saphyr::from_str::<ExistingConfig>(content)?)
}

fn provider_has_auth(provider: &ExistingProvider) -> bool {
    !provider.api_key.trim().is_empty()
        || codex_auth_file_available(provider.api_base.trim())
        || env_api_key_for_provider(
            provider.family.trim(),
            provider.model.trim(),
            provider.api_base.trim(),
        )
        .is_some()
}

fn codex_auth_file_available(api_base: &str) -> bool {
    if api_base != CODEX_API_BASE {
        return false;
    }

    let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) else {
        return false;
    };
    let auth_path = PathBuf::from(home).join(".codex/auth.json");
    let Ok(content) = fs::read_to_string(auth_path) else {
        return false;
    };
    serde_json::from_str::<ExistingCodexAuth>(&content)
        .is_ok_and(|auth| !auth.tokens.access_token.trim().is_empty())
}

#[derive(Debug, Default, Deserialize)]
struct ExistingCodexAuth {
    #[serde(default)]
    tokens: ExistingOAuthToken,
}

#[derive(Debug, Default, Deserialize)]
struct ExistingOAuthToken {
    #[serde(default)]
    access_token: String,
}

fn env_api_key_for_provider(family: &str, model: &str, api_base: &str) -> Option<String> {
    api_key_env_candidates(family, model, api_base)
        .into_iter()
        .find_map(|name| {
            env::var(name).ok().and_then(|value| {
                let value = value.trim().to_string();
                (!value.is_empty()).then_some(value)
            })
        })
}

fn api_key_env_candidates(family: &str, model: &str, api_base: &str) -> Vec<&'static str> {
    let family = family.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    let api_base = api_base.to_ascii_lowercase();
    let mut candidates = Vec::new();

    if api_base.contains("deepseek") || model.contains("deepseek") {
        push_candidate(&mut candidates, "DEEPSEEK_API_KEY");
    } else if api_base.contains("minimaxi") || model.contains("minimax") {
        push_candidate(&mut candidates, "MINIMAX_API_KEY");
        push_candidate(&mut candidates, "MINIMAXI_API_KEY");
    } else if api_base.contains("xiaomimimo") || model.contains("mimo") {
        push_candidate(&mut candidates, "MIMO_API_KEY");
        push_candidate(&mut candidates, "XIAOMI_MIMO_API_KEY");
    } else if api_base.contains("moonshot") || model.contains("kimi") {
        push_candidate(&mut candidates, "MOONSHOT_API_KEY");
        push_candidate(&mut candidates, "KIMI_API_KEY");
    } else if api_base.contains("bigmodel") || model.contains("glm") {
        push_candidate(&mut candidates, "BIGMODEL_API_KEY");
        push_candidate(&mut candidates, "ZHIPUAI_API_KEY");
        push_candidate(&mut candidates, "GLM_API_KEY");
    } else if api_base.contains("openrouter") {
        push_candidate(&mut candidates, "OPENROUTER_API_KEY");
    } else if api_base.contains("groq") {
        push_candidate(&mut candidates, "GROQ_API_KEY");
    } else if api_base.contains("siliconflow") {
        push_candidate(&mut candidates, "SILICONFLOW_API_KEY");
    } else if api_base.contains("dashscope") || model.contains("qwen") {
        push_candidate(&mut candidates, "DASHSCOPE_API_KEY");
        push_candidate(&mut candidates, "QWEN_API_KEY");
    } else if api_base.contains("anthropic.com") {
        push_candidate(&mut candidates, "ANTHROPIC_API_KEY");
    } else if api_base.contains("openai.com") {
        push_candidate(&mut candidates, "OPENAI_API_KEY");
    } else if api_base.contains("googleapis.com") || model.contains("gemini") {
        push_candidate(&mut candidates, "GEMINI_API_KEY");
        push_candidate(&mut candidates, "GOOGLE_API_KEY");
    }

    if candidates.is_empty() {
        match family.as_str() {
            "anthropic" => push_candidate(&mut candidates, "ANTHROPIC_API_KEY"),
            "openai" => push_candidate(&mut candidates, "OPENAI_API_KEY"),
            "gemini" | "google" => {
                push_candidate(&mut candidates, "GEMINI_API_KEY");
                push_candidate(&mut candidates, "GOOGLE_API_KEY");
            }
            _ => {}
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<&'static str>, name: &'static str) {
    if !candidates.contains(&name) {
        candidates.push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static UPDATE_UI_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_update_ui_state_for_test() {
        *lock_update_ui_state() = LauncherUpdateUiState::default();
    }

    fn downloaded_test_update_state() -> LauncherAutoUpdateState {
        LauncherAutoUpdateState {
            status: "downloaded".to_string(),
            current_tag: "v1.0.0".to_string(),
            latest_tag: Some("v1.2.3".to_string()),
            downloaded_path: Some("/tmp/anda-update".to_string()),
            error: None,
        }
    }

    #[test]
    fn launcher_language_detects_chinese_tags() {
        assert_eq!(
            language_from_tags(["xx-XX", "zh-Hans-CN", "en-US"]),
            LauncherLanguage::ZhHans
        );
        assert_eq!(
            language_from_tags(["zh_CN.UTF-8"]),
            LauncherLanguage::ZhHans
        );
    }

    #[test]
    fn launcher_language_detects_all_supported_tags() {
        assert_eq!(language_from_tags(["ru-RU"]), LauncherLanguage::Ru);
        assert_eq!(language_from_tags(["ar-SA"]), LauncherLanguage::Ar);
        assert_eq!(language_from_tags(["fr-FR"]), LauncherLanguage::Fr);
        assert_eq!(language_from_tags(["es-419"]), LauncherLanguage::Es);
        assert_eq!(
            language_from_tags(["xx-XX", "fr-FR", "en-US"]),
            LauncherLanguage::Fr
        );
    }

    #[test]
    fn launcher_language_falls_back_to_english() {
        assert_eq!(language_from_tags(["de-DE", "xx-XX"]), LauncherLanguage::En);
        assert_eq!(language_from_tags(["en-US"]), LauncherLanguage::En);
    }

    #[test]
    fn persisted_language_roundtrip() {
        let home = tempfile::tempdir().unwrap();

        assert_eq!(load_persisted_language(home.path()), None);
        persist_launcher_language(home.path(), LauncherLanguage::Fr).unwrap();
        assert_eq!(
            load_persisted_language(home.path()),
            Some(LauncherLanguage::Fr)
        );

        let content = fs::read_to_string(ui_settings_path(home.path())).unwrap();
        assert!(content.contains("\"language\": \"fr\""));
    }

    #[test]
    fn persisted_language_ignores_invalid_content() {
        let home = tempfile::tempdir().unwrap();
        let path = ui_settings_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        fs::write(&path, "not json").unwrap();
        assert_eq!(load_persisted_language(home.path()), None);

        fs::write(&path, r#"{"language": "xx"}"#).unwrap();
        assert_eq!(load_persisted_language(home.path()), None);
    }

    #[test]
    fn launcher_text_uses_locale_files() {
        let en = text_for_language(LauncherLanguage::En);
        assert_eq!(en.api_key, "API key");
        assert_eq!(en.settings, "Settings");
        assert_eq!(en.model_settings, "Model settings...");
        assert_eq!(en.status_pid, "PID");
        assert_eq!(en.status_gateway_url, "Gateway URL");
        assert_eq!(en.status_conversations, "Conversations");
        assert_eq!(en.status_memory_nodes, "Memory nodes");
        assert_eq!(en.status_memory_links, "Memory links");
        assert_eq!(
            en.unsupported_provider("custom"),
            "unsupported provider: custom"
        );
        assert_eq!(
            en.update_ready_message("v1.2.3"),
            "Downloaded v1.2.3. Restart to apply."
        );
        assert_eq!(
            en.check_update_label("v1.2.3"),
            "Check for updates (v1.2.3)"
        );
        assert_eq!(
            en.checking_update_label("/", "v1.2.3"),
            "/ Checking for updates (v1.2.3)"
        );
        assert_eq!(
            en.update_downloaded_restart_message("v1.2.3"),
            "Install and restart v1.2.3"
        );
        assert_eq!(en.browser_extension_token_copy_button, "Copy Token");
        assert_eq!(
            en.browser_extension_token_only_copied,
            "Copied Bearer token to the clipboard."
        );

        let zh = text_for_language(LauncherLanguage::ZhHans);
        assert_eq!(zh.api_key, "API 密钥");
        assert_eq!(zh.settings, "设置");
        assert_eq!(zh.model_settings, "大模型配置...");
        assert_eq!(zh.status_pid, "PID");
        assert_eq!(zh.status_gateway_url, "Gateway URL");
        assert_eq!(zh.status_conversations, "对话数量");
        assert_eq!(zh.status_memory_nodes, "记忆节点数量");
        assert_eq!(zh.status_memory_links, "记忆链接数量");
        assert_eq!(
            zh.unsupported_provider("custom"),
            "不支持的模型供应商：custom"
        );
        assert_eq!(
            zh.update_ready_message("v1.2.3"),
            "已下载 v1.2.3，重启生效。"
        );
        assert_eq!(zh.check_update_label("v1.2.3"), "检查更新（v1.2.3）");
        assert_eq!(
            zh.checking_update_label("/", "v1.2.3"),
            "/ 正在检查更新（v1.2.3）"
        );
        assert_eq!(
            zh.update_downloaded_restart_message("v1.2.3"),
            "安装并重启 v1.2.3"
        );
        assert_eq!(zh.browser_extension_token_copy_button, "复制 Token");
        assert_eq!(
            zh.browser_extension_token_only_copied,
            "已将 Bearer token 复制到剪贴板。"
        );
    }

    #[test]
    fn launcher_text_supports_added_locales() {
        assert_eq!(
            text_for_language(LauncherLanguage::Ru).settings,
            "Настройки"
        );
        assert_eq!(text_for_language(LauncherLanguage::Ar).settings, "الإعدادات");
        assert_eq!(
            text_for_language(LauncherLanguage::Fr).settings,
            "Paramètres"
        );
        assert_eq!(
            text_for_language(LauncherLanguage::Es).settings,
            "Configuración"
        );

        for language in LauncherLanguage::ALL {
            let copy = text_for_language(language);
            assert!(!copy.language.is_empty());
            assert_ne!(copy.language, "launcher.language");
            assert!(!language.native_name().is_empty());
            assert_eq!(language_from_tag(language.locale()), Some(language));
        }
    }

    #[test]
    fn current_version_tag_uses_package_version() {
        assert_eq!(
            current_version_tag(),
            format!("v{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn update_check_gate_blocks_concurrent_checks() {
        let _guard = UPDATE_UI_TEST_LOCK.lock().unwrap();
        reset_update_ui_state_for_test();

        assert!(begin_update_check());
        assert!(!begin_update_check());
        finish_update_check(None);
        assert!(begin_update_check());
        finish_update_check(None);

        reset_update_ui_state_for_test();
    }

    #[test]
    fn downloaded_update_state_returns_only_pending_downloads() {
        let _guard = UPDATE_UI_TEST_LOCK.lock().unwrap();
        reset_update_ui_state_for_test();

        assert!(downloaded_update_state().is_none());
        finish_update_check(Some(LauncherAutoUpdateState {
            status: "idle".to_string(),
            current_tag: "v1.0.0".to_string(),
            latest_tag: Some("v1.0.0".to_string()),
            downloaded_path: None,
            error: None,
        }));
        assert!(downloaded_update_state().is_none());

        let downloaded = downloaded_test_update_state();
        finish_update_check(Some(downloaded.clone()));
        assert_eq!(downloaded_update_state(), Some(downloaded));

        reset_update_ui_state_for_test();
    }

    #[test]
    fn update_restart_prompt_gate_blocks_duplicate_prompts() {
        let _guard = UPDATE_UI_TEST_LOCK.lock().unwrap();
        reset_update_ui_state_for_test();
        let state = downloaded_test_update_state();

        assert!(begin_update_restart_prompt(&state));
        assert!(!begin_update_restart_prompt(&state));
        finish_update_restart_prompt(&state);
        assert!(begin_update_restart_prompt(&state));
        finish_update_restart_prompt(&state);

        reset_update_ui_state_for_test();
    }

    #[test]
    fn update_restart_success_clears_pending_download_menu_state() {
        let _guard = UPDATE_UI_TEST_LOCK.lock().unwrap();
        reset_update_ui_state_for_test();
        let state = downloaded_test_update_state();

        finish_update_check(Some(state.clone()));
        assert_eq!(downloaded_update_state(), Some(state.clone()));
        assert_eq!(
            check_update_menu_label(),
            text().update_downloaded_restart_message("v1.2.3")
        );

        assert!(begin_update_restart_prompt(&state));
        finish_update_restart_success(&state);

        assert!(downloaded_update_state().is_none());
        assert_eq!(
            check_update_menu_label(),
            text().check_update_label("v1.2.3")
        );
        assert!(begin_update_restart_prompt(&state));
        finish_update_restart_prompt(&state);

        reset_update_ui_state_for_test();
    }

    #[test]
    fn menu_action_gate_blocks_concurrent_actions() {
        let guard = try_begin_menu_action().expect("gate should be free");
        assert!(try_begin_menu_action().is_none());
        drop(guard);

        let guard = begin_menu_action();
        assert!(try_begin_menu_action().is_none());
        drop(guard);
        assert!(try_begin_menu_action().is_some());
    }

    #[test]
    fn write_config_atomic_replaces_existing_file_and_cleans_temp() {
        let home = tempfile::tempdir().unwrap();
        let config_path = home.path().join("config.yaml");
        fs::write(&config_path, "old").unwrap();

        write_config_atomic(&config_path, "new contents\n").unwrap();

        assert_eq!(fs::read_to_string(&config_path).unwrap(), "new contents\n");
        assert!(!config_path.with_file_name("config.yaml.tmp").exists());
    }

    #[test]
    fn minimal_config_is_setup_complete() {
        let provider = provider_by_id("openai").unwrap();
        let config = render_minimal_config(provider, "gpt-test", "sk-test");
        parse_existing_config(&config).unwrap();

        assert!(!parsed_config_needs_setup(&config));
        assert!(config.contains("model: \"gpt-test\""));
        assert!(config.contains("api_key: \"sk-test\""));
    }

    #[test]
    fn empty_api_key_needs_setup() {
        let config = r#"
model:
  active: custom-model
  providers:
    - family: custom
      model: custom-model
      api_base: https://example.invalid/v1
      api_key: ""
"#;

        assert!(parsed_config_needs_setup(config));
    }

    #[test]
    fn existing_non_empty_config_does_not_trigger_initial_setup() {
        let home = tempfile::tempdir().unwrap();
        let ctx = launcher_context(home.path());
        fs::write(
            ctx.config_path(),
            r#"
model:
  active: deepseek-v4-pro
  providers:
    - family: anthropic
      model: deepseek-v4-pro
      api_base: https://api.deepseek.com/anthropic
      api_key: sk-existing
"#,
        )
        .unwrap();

        assert!(!config_needs_setup(&ctx));
    }

    #[test]
    fn config_update_preserves_comments_and_creates_backup() {
        let home = tempfile::tempdir().unwrap();
        let ctx = launcher_context(home.path());
        let existing = r#"# keep top comment
model:
  # keep model comment
  active: "gpt-5.4" # active comment
  providers:
    - family: openai
      model: "gpt-5.4"
      api_base: "https://api.openai.com/v1"
      api_key: "old-key" # key comment
      labels: ["custom"]

tts:
  # keep tts comment
  enabled: false
"#;
        fs::write(ctx.config_path(), existing).unwrap();

        write_minimal_config(&ctx, &wizard_config()).unwrap();

        let updated = fs::read_to_string(ctx.config_path()).unwrap();
        assert!(updated.contains("# keep top comment"));
        assert!(updated.contains("  active: \"gpt-test\" # active comment"));
        assert!(updated.contains("      api_key: \"sk-test\" # key comment"));
        assert!(updated.contains("      labels: [\"custom\"]"));
        assert!(updated.contains("  # keep tts comment"));

        let backups = fs::read_dir(home.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".bak"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), existing);
    }

    #[test]
    fn initial_config_write_creates_missing_config() {
        let home = tempfile::tempdir().unwrap();
        let ctx = launcher_context(home.path());

        write_initial_minimal_config(&ctx, &wizard_config()).unwrap();

        let config = fs::read_to_string(ctx.config_path()).unwrap();
        assert!(config.contains("## anda_bot runtime configuration"));
        assert!(config.contains("model: \"gpt-test\""));
        assert!(config.contains("api_key: \"sk-test\""));
    }

    #[test]
    fn config_needs_setup_copies_default_config_when_missing() {
        let home = tempfile::tempdir().unwrap();
        let ctx = launcher_context(home.path());

        let _ = config_needs_setup(&ctx);

        assert_eq!(
            fs::read_to_string(ctx.config_path()).unwrap(),
            DEFAULT_CONFIG_TEMPLATE
        );
    }

    #[test]
    fn provider_defaults_are_addressable_by_id() {
        assert_eq!(default_model_for_provider("gemini"), "gemini-pro-latest");
        assert_eq!(
            default_model_for_provider("missing"),
            default_provider().model
        );
        assert!(provider_ids().contains(&"deepseek"));
    }

    #[test]
    fn browser_extension_token_command_defaults_to_one_year() {
        assert_eq!(
            browser_extension_token_args(),
            ["browser", "token", "--days", "365"]
        );
    }

    #[test]
    fn browser_extension_bearer_token_extracts_only_token_value() {
        let output = "\
Gateway URL: http://127.0.0.1:8042
Bearer token: hEOhASegWGqIAng_c3NhaHUtZTR1YXctMzRtNXEtZXpnamEtZW
Extension directory: chrome_extension
";

        assert_eq!(
            browser_extension_bearer_token(output),
            Some("hEOhASegWGqIAng_c3NhaHUtZTR1YXctMzRtNXEtZXpnamEtZW".to_string())
        );
        assert_eq!(
            browser_extension_bearer_token("Gateway URL: http://127.0.0.1:8042"),
            None
        );
    }

    #[test]
    fn launcher_daemon_status_extracts_pid_and_gateway_url() {
        let result = CommandResult {
            success: true,
            message: "\
anda daemon is running (pid 12345)
Gateway URL: http://127.0.0.1:8042
Logs: /tmp/anda.log
Conversations: 7
Memory nodes: 11
Memory links: 13
"
            .to_string(),
        };

        let status = launcher_daemon_status_from_command_result(&result);

        assert_eq!(status.summary, "anda daemon is running (pid 12345)");
        assert_eq!(status.pid.as_deref(), Some("12345"));
        assert_eq!(status.gateway_url.as_deref(), Some("http://127.0.0.1:8042"));
        assert_eq!(status.conversations.as_deref(), Some("7"));
        assert_eq!(status.memory_nodes.as_deref(), Some("11"));
        assert_eq!(status.memory_links.as_deref(), Some("13"));
    }

    #[test]
    fn launcher_daemon_status_extracts_json_status() {
        let result = CommandResult {
            success: true,
            message: r#"{
  "state": "running",
  "summary": "anda daemon is running (pid 12345)",
  "pid": 12345,
  "pid_file": null,
  "gateway_url": "http://127.0.0.1:8042",
  "log_file": "/tmp/anda.log",
  "conversations": 7,
  "memory_nodes": 11,
  "memory_links": 13
}"#
            .to_string(),
        };

        let status = launcher_daemon_status_from_command_result(&result);

        assert_eq!(status.summary, "anda daemon is running (pid 12345)");
        assert_eq!(status.pid.as_deref(), Some("12345"));
        assert_eq!(status.gateway_url.as_deref(), Some("http://127.0.0.1:8042"));
        assert_eq!(status.conversations.as_deref(), Some("7"));
        assert_eq!(status.memory_nodes.as_deref(), Some("11"));
        assert_eq!(status.memory_links.as_deref(), Some("13"));
    }

    #[test]
    fn launcher_daemon_status_uses_json_pid_file_when_pid_is_missing() {
        let result = CommandResult {
            success: true,
            message: r#"{
  "state": "gateway_running",
  "summary": "anda daemon gateway is running",
  "pid": null,
  "pid_file": "missing",
  "gateway_url": "http://127.0.0.1:8042",
  "log_file": null,
  "conversations": 3,
  "memory_nodes": 5,
  "memory_links": 8
}"#
            .to_string(),
        };

        let status = launcher_daemon_status_from_command_result(&result);

        assert_eq!(status.summary, "anda daemon gateway is running");
        assert_eq!(status.pid.as_deref(), Some("missing"));
        assert_eq!(status.gateway_url.as_deref(), Some("http://127.0.0.1:8042"));
        assert_eq!(status.conversations.as_deref(), Some("3"));
        assert_eq!(status.memory_nodes.as_deref(), Some("5"));
        assert_eq!(status.memory_links.as_deref(), Some("8"));
    }

    #[test]
    fn launcher_daemon_status_handles_gateway_without_pid() {
        let result = CommandResult {
            success: true,
            message: "\
anda daemon gateway is running
Gateway URL: http://127.0.0.1:8042
PID file: missing
Conversations: 3
Memory nodes: 5
Memory links: 8
"
            .to_string(),
        };

        let status = launcher_daemon_status_from_command_result(&result);

        assert_eq!(status.summary, "anda daemon gateway is running");
        assert_eq!(status.pid.as_deref(), Some("missing"));
        assert_eq!(status.gateway_url.as_deref(), Some("http://127.0.0.1:8042"));
        assert_eq!(status.conversations.as_deref(), Some("3"));
        assert_eq!(status.memory_nodes.as_deref(), Some("5"));
        assert_eq!(status.memory_links.as_deref(), Some("8"));
    }

    #[test]
    fn launcher_daemon_status_handles_process_without_gateway() {
        let result = CommandResult {
            success: true,
            message: "\
anda daemon process exists but gateway is not responding (pid 12345)
Logs: /tmp/anda.log
"
            .to_string(),
        };

        let status = launcher_daemon_status_from_command_result(&result);

        assert_eq!(
            status.summary,
            "anda daemon process exists but gateway is not responding (pid 12345)"
        );
        assert_eq!(status.pid.as_deref(), Some("12345"));
        assert_eq!(status.gateway_url, None);
        assert_eq!(status.conversations, None);
        assert_eq!(status.memory_nodes, None);
        assert_eq!(status.memory_links, None);
    }

    #[test]
    fn launcher_home_arg_overrides_default_detection() {
        assert_eq!(
            home_arg_from_args([
                OsString::from("--home"),
                OsString::from("C:\\Users\\test\\.anda-custom"),
            ])
            .unwrap(),
            Some(PathBuf::from("C:\\Users\\test\\.anda-custom"))
        );
        assert_eq!(
            home_arg_from_args([OsString::from("--home=C:\\Users\\test\\.anda-inline")]).unwrap(),
            Some(PathBuf::from("C:\\Users\\test\\.anda-inline"))
        );
        assert!(home_arg_from_args([OsString::from("--home")]).is_err());
    }

    #[test]
    fn detect_anda_exe_uses_existing_sibling_first() {
        let home = tempfile::tempdir().unwrap();
        let launcher = home.path().join("anda_launcher");
        let sibling = home
            .path()
            .join(if cfg!(windows) { "anda.exe" } else { "anda" });
        fs::write(&sibling, "").unwrap();
        let fallback = home.path().join("other").join("anda");

        assert_eq!(
            detect_anda_exe_from_candidates(&launcher, [fallback]),
            sibling
        );
    }

    #[test]
    fn detect_anda_exe_uses_existing_fallback_when_sibling_missing() {
        let home = tempfile::tempdir().unwrap();
        let launcher = home.path().join("app/Contents/MacOS/Anda Bot");
        let fallback_dir = home.path().join("bin");
        fs::create_dir_all(&fallback_dir).unwrap();
        let fallback = fallback_dir.join("anda");
        fs::write(&fallback, "").unwrap();

        assert_eq!(
            detect_anda_exe_from_candidates(&launcher, [fallback.clone()]),
            fallback
        );
    }

    fn launcher_context(home: &Path) -> LauncherContext {
        LauncherContext {
            launcher_exe: home.join("anda_launcher"),
            anda_exe: home.join("anda"),
            home: home.to_path_buf(),
        }
    }

    fn wizard_config() -> WizardConfig {
        WizardConfig {
            provider_id: "openai".to_string(),
            model: "gpt-test".to_string(),
            api_key: "sk-test".to_string(),
        }
    }

    #[cfg(unix)]
    fn context_with_fake_anda(home: &Path, succeed: bool) -> LauncherContext {
        use std::os::unix::fs::PermissionsExt;
        let anda_exe = home.join("anda");
        let script = if succeed {
            r#"#!/bin/sh
if [ "$1" = "update" ] && [ "$2" = "--check" ]; then
  echo '{"status":"downloaded","current_tag":"v1.0.0","latest_tag":"v2.0.0","downloaded_path":"/tmp/anda-update"}'
  exit 0
fi
if [ "$1" = "update" ] && [ "$2" = "--check-if-due" ]; then
  echo '{"status":"current","current_tag":"v1.0.0"}'
  exit 0
fi
if [ "$1" = "status" ]; then
  if [ "$2" = "--json" ]; then
    echo '{"summary":"anda daemon is running (pid 111)","pid":111,"gateway_url":"http://127.0.0.1:8042","conversations":1,"memory_nodes":2,"memory_links":3}'
  else
    echo "anda daemon is running (pid 111)"
  fi
  exit 0
fi
echo "ok: $@"
exit 0
"#
        } else {
            r#"#!/bin/sh
echo "boom" 1>&2
exit 1
"#
        };
        fs::write(&anda_exe, script).unwrap();
        fs::set_permissions(&anda_exe, fs::Permissions::from_mode(0o755)).unwrap();
        LauncherContext {
            launcher_exe: home.join("anda_launcher"),
            anda_exe,
            home: home.to_path_buf(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_anda_commands_dispatch_to_fake_binary() {
        let home = tempfile::tempdir().unwrap();
        let ctx = context_with_fake_anda(home.path(), true);

        assert!(start_daemon(&ctx).unwrap().success);
        assert!(stop_daemon(&ctx).unwrap().success);
        assert!(restart_daemon(&ctx).unwrap().success);
        assert!(reload_models(&ctx).unwrap().success);
        assert!(daemon_status(&ctx).unwrap().success);
        assert!(daemon_status_json(&ctx).unwrap().success);
        assert!(generate_browser_extension_token(&ctx).unwrap().success);
        assert!(reload_models_or_start_daemon(&ctx).success);

        let status = refresh_daemon_status_cache(&ctx);
        assert_eq!(status.pid.as_deref(), Some("111"));
        // The cache now returns the refreshed value.
        assert_eq!(cached_daemon_status().pid.as_deref(), Some("111"));
    }

    #[cfg(unix)]
    #[test]
    fn check_update_now_and_if_due_parse_json_state() {
        let home = tempfile::tempdir().unwrap();
        let ctx = context_with_fake_anda(home.path(), true);

        let now = check_update_now(&ctx).unwrap();
        assert_eq!(now.status, "downloaded");
        assert!(now.downloaded_update_available());

        let due = check_update_if_due(&ctx).unwrap();
        assert_eq!(due.status, "current");
    }

    #[cfg(unix)]
    #[test]
    fn run_anda_surfaces_failure_exit_status() {
        let home = tempfile::tempdir().unwrap();
        let ctx = context_with_fake_anda(home.path(), false);
        let result = start_daemon(&ctx).unwrap();
        assert!(!result.success);
        assert_eq!(result.message, "boom");

        // run_update_check returns Err for non-success commands.
        assert!(check_update_now(&ctx).is_err());
    }

    #[test]
    fn run_anda_errors_when_binary_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let ctx = launcher_context(home.path());
        // The configured anda_exe does not exist, so spawning fails.
        assert!(run_anda(&ctx, &["status"]).is_err());
    }

    #[test]
    fn launcher_auto_update_state_status_messages() {
        let downloaded = LauncherAutoUpdateState {
            status: "downloaded".to_string(),
            current_tag: "v1.0.0".to_string(),
            latest_tag: Some("v2.0.0".to_string()),
            downloaded_path: Some("/tmp/anda".to_string()),
            error: None,
        };
        assert!(downloaded.downloaded_update_available());
        assert_eq!(downloaded.current_tag_label(), "v1.0.0");
        assert_eq!(downloaded.latest_tag_label(), "v2.0.0");
        assert!(!downloaded.check_message().is_empty());

        // Same current and latest tag is not an update.
        let current = LauncherAutoUpdateState {
            status: "downloaded".to_string(),
            current_tag: "v2.0.0".to_string(),
            latest_tag: Some("v2.0.0".to_string()),
            downloaded_path: Some("/tmp/anda".to_string()),
            error: None,
        };
        assert!(!current.downloaded_update_available());

        for status in [
            "failed",
            "checking",
            "downloading",
            "idle",
            "current",
            "weird",
        ] {
            let state = LauncherAutoUpdateState {
                status: status.to_string(),
                current_tag: "v1.0.0".to_string(),
                error: Some("oops".to_string()),
                ..Default::default()
            };
            assert!(!state.check_message().is_empty());
        }

        // Missing latest tag uses the localized fallback label.
        let no_latest = LauncherAutoUpdateState::default();
        assert!(!no_latest.latest_tag_label().is_empty());
    }

    #[test]
    fn check_update_menu_label_reflects_ui_state() {
        reset_update_ui_state_for_test();
        // Default (no state) shows the check label.
        assert!(!check_update_menu_label().is_empty());

        // While checking, the label includes the spinner.
        assert!(begin_update_check());
        assert!(!check_update_menu_label().is_empty());
        finish_update_check(Some(downloaded_test_update_state()));

        // A downloaded update surfaces the restart message.
        assert!(!check_update_menu_label().is_empty());
        reset_update_ui_state_for_test();
    }

    #[test]
    fn launcher_text_formatters_render_for_default_locale() {
        let copy = text();
        assert!(!copy.unsupported_provider("x").is_empty());
        assert!(!copy.unsupported_provider_from_wizard("x").is_empty());
        assert!(!copy.env_required("OPENAI_API_KEY").is_empty());
        assert!(!copy.settings_wizard_failed("d").is_empty());
        assert!(!copy.powershell_launch_failed("d").is_empty());
        assert!(!copy.launcher_exe_detect_failed("d").is_empty());
        assert!(!copy.run_anda_failed("/p", "d").is_empty());
        assert!(!copy.command_failed("d").is_empty());
        assert!(!copy.schtasks_failed("d").is_empty());
        assert!(!copy.check_update_label("v1").is_empty());
        assert!(!copy.checking_update_label("|", "v1").is_empty());
        assert!(!copy.update_downloaded_restart_message("v2").is_empty());
        assert!(!copy.update_ready_message("v2").is_empty());
        assert!(!copy.update_restart_confirm("v2").is_empty());
        assert!(!copy.update_current_message("v1").is_empty());
        assert!(!copy.update_check_failed_message("d").is_empty());
        assert!(!copy.update_restart_failed_message("d").is_empty());
        assert!(!copy.restart_recovery("m").is_empty());
        assert!(!copy.missing_home_arg().is_empty());
    }

    #[test]
    fn launcher_text_renders_for_every_language() {
        for language in [
            LauncherLanguage::En,
            LauncherLanguage::ZhHans,
            LauncherLanguage::Ru,
            LauncherLanguage::Ar,
            LauncherLanguage::Fr,
            LauncherLanguage::Es,
        ] {
            let copy = text_for_language(language);
            assert!(!copy.app_title.is_empty());
            assert!(!copy.start_daemon.is_empty());
        }
    }

    #[test]
    fn api_key_env_candidates_route_by_api_base_and_family() {
        assert_eq!(
            api_key_env_candidates("openai", "deepseek-chat", "https://api.deepseek.com/v1"),
            vec!["DEEPSEEK_API_KEY"]
        );
        assert!(
            api_key_env_candidates("x", "kimi-k2", "https://api.moonshot.cn")
                .contains(&"MOONSHOT_API_KEY")
        );
        assert!(
            api_key_env_candidates("x", "m", "https://openrouter.ai/api/v1")
                .contains(&"OPENROUTER_API_KEY")
        );
        assert_eq!(
            api_key_env_candidates("anthropic", "claude", "https://api.anthropic.com"),
            vec!["ANTHROPIC_API_KEY"]
        );
        assert_eq!(
            api_key_env_candidates("openai", "gpt", "https://api.openai.com/v1"),
            vec!["OPENAI_API_KEY"]
        );
        // Family fallback when api_base/model are unrecognized.
        assert_eq!(
            api_key_env_candidates("anthropic", "custom", "https://example.invalid"),
            vec!["ANTHROPIC_API_KEY"]
        );
        assert!(api_key_env_candidates("unknown", "model", "https://example.invalid").is_empty());
    }

    #[test]
    fn codex_auth_file_available_requires_codex_api_base() {
        assert!(!codex_auth_file_available("https://api.openai.com/v1"));
    }

    #[test]
    fn yaml_scalar_helpers_quote_and_unquote() {
        assert_eq!(yaml_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(yaml_bare_or_string("simple-id_1"), "simple-id_1");
        assert_eq!(yaml_bare_or_string("has space"), "\"has space\"");

        assert_eq!(unquote_yaml_scalar("\"hi\\\"there\""), "hi\"there");
        assert_eq!(unquote_yaml_scalar("'it''s'"), "it's");
        assert_eq!(unquote_yaml_scalar("bare"), "bare");

        assert_eq!(leading_spaces("    x"), 4);
        assert_eq!(leading_spaces("y"), 0);
    }

    #[test]
    fn parse_existing_config_detects_setup_completeness() {
        let complete = parse_existing_config(
            r#"
model:
  active: gpt-test
  providers:
    - family: openai
      model: gpt-test
      api_base: https://api.openai.com/v1
      api_key: sk-test
"#,
        )
        .unwrap();
        assert!(!complete.needs_setup());

        let missing_key = parse_existing_config(
            r#"
model:
  active: gpt-test
  providers:
    - family: openai
      model: gpt-test
      api_base: https://api.openai.com/v1
      api_key: ""
"#,
        )
        .unwrap();
        assert!(missing_key.needs_setup());

        // Empty content needs setup.
        assert!(parse_existing_config("").unwrap().needs_setup());
    }

    #[test]
    fn spinner_frame_returns_a_frame() {
        let frame = spinner_frame(std::time::Instant::now());
        assert!(!frame.is_empty());
    }

    #[test]
    fn locale_tag_collectors_return_lists() {
        // These just read the environment/platform; they must not panic.
        let _ = system_locale_tags();
        let _ = environment_locale_tags();
    }
}
