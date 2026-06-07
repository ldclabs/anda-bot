use rust_i18n::t;
use serde::Deserialize;
use std::{
    env,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(windows)]
use std::{os::windows::ffi::OsStrExt, ptr};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    Globalization::GetUserDefaultLocaleName,
    System::Threading::{CreateMutexW, ReleaseMutex},
};

pub type LauncherResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../../assets/config.yaml");
const CODEX_API_BASE: &str = "https://chatgpt.com/backend-api/codex";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherLanguage {
    En,
    ZhHans,
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
    pub fn downloaded_update_available(&self) -> bool {
        self.status == "downloaded"
            && self
                .latest_tag
                .as_deref()
                .is_some_and(|latest| latest != self.current_tag)
            && self
                .downloaded_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
    }

    pub fn latest_tag_label(&self) -> String {
        self.latest_tag
            .as_deref()
            .filter(|tag| !tag.trim().is_empty())
            .map(ToOwned::to_owned)
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
            _ => copy.update_current_message(&self.current_tag),
        }
    }
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
    pub status: String,
    pub start_daemon: String,
    pub stop_daemon: String,
    pub restart_daemon: String,
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

static LAUNCHER_LANGUAGE: std::sync::OnceLock<LauncherLanguage> = std::sync::OnceLock::new();

impl LauncherLanguage {
    fn locale(self) -> &'static str {
        match self {
            LauncherLanguage::En => "en",
            LauncherLanguage::ZhHans => "zh-Hans",
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
        status: t!("launcher.status", locale = locale).into_owned(),
        start_daemon: t!("launcher.start_daemon", locale = locale).into_owned(),
        stop_daemon: t!("launcher.stop_daemon", locale = locale).into_owned(),
        restart_daemon: t!("launcher.restart_daemon", locale = locale).into_owned(),
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
    *LAUNCHER_LANGUAGE.get_or_init(detect_launcher_language)
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
    ProviderPreset {
        id: "minimax",
        label: "MiniMax",
        family: "anthropic",
        model: "MiniMax-M3",
        api_base: "https://api.minimaxi.com/anthropic/v1",
        env_var: "MINIMAX_API_KEY",
        bearer_auth: false,
    },
    ProviderPreset {
        id: "mimo",
        label: "MiMo",
        family: "anthropic",
        model: "mimo-v2.5-pro",
        api_base: "https://api.xiaomimimo.com/anthropic/v1",
        env_var: "MIMO_API_KEY",
        bearer_auth: true,
    },
];

impl LauncherContext {
    pub fn detect() -> LauncherResult<Self> {
        let launcher_exe = env::current_exe()
            .map_err(|err| text().launcher_exe_detect_failed(&err.to_string()))?;
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
        fs::write(config_path, updated)?;
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

pub fn daemon_status(ctx: &LauncherContext) -> LauncherResult<CommandResult> {
    run_anda(ctx, &["status"])
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
    command.arg("--home").arg(&ctx.home);
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

fn detect_anda_exe(launcher_exe: &Path) -> PathBuf {
    let exe_name = if cfg!(windows) { "anda.exe" } else { "anda" };
    if let Some(parent) = launcher_exe.parent() {
        let sibling = parent.join(exe_name);
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from(exe_name)
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
            fs::write(config_path, DEFAULT_CONFIG_TEMPLATE)?;
            Ok(true)
        }
        Err(err) => Err(err.into()),
    }
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

    #[test]
    fn launcher_language_detects_chinese_tags() {
        assert_eq!(
            language_from_tags(["fr-FR", "zh-Hans-CN", "en-US"]),
            LauncherLanguage::ZhHans
        );
        assert_eq!(
            language_from_tags(["zh_CN.UTF-8"]),
            LauncherLanguage::ZhHans
        );
    }

    #[test]
    fn launcher_language_falls_back_to_english() {
        assert_eq!(language_from_tags(["fr-FR", "es-ES"]), LauncherLanguage::En);
        assert_eq!(language_from_tags(["en-US"]), LauncherLanguage::En);
    }

    #[test]
    fn launcher_text_uses_locale_files() {
        let en = text_for_language(LauncherLanguage::En);
        assert_eq!(en.api_key, "API key");
        assert_eq!(
            en.unsupported_provider("custom"),
            "unsupported provider: custom"
        );
        assert_eq!(
            en.update_ready_message("v1.2.3"),
            "Anda v1.2.3 has been downloaded. Install and restart to use it."
        );

        let zh = text_for_language(LauncherLanguage::ZhHans);
        assert_eq!(zh.api_key, "API 密钥");
        assert_eq!(
            zh.unsupported_provider("custom"),
            "不支持的模型供应商：custom"
        );
        assert_eq!(
            zh.update_ready_message("v1.2.3"),
            "已下载 Anda v1.2.3。安装并重启后生效。"
        );
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
}
