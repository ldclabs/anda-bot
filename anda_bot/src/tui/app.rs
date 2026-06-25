use std::path::PathBuf;

use anda_core::BoxError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::oneshot;

use crate::{
    auto_update::AutoUpdateState,
    config::Config,
    daemon::{Daemon, LaunchState, process_exists},
    gateway,
};

use super::{
    input::{InputCursorDirection, input_newline_key, move_cursor_vertically},
    text::{compact_cjk_spacing_with_cursor, normalize_newlines},
};

#[derive(Default)]
pub(super) struct SetupState {
    pub(super) template_created: bool,
    pub(super) issues: Vec<String>,
}

impl SetupState {
    pub(super) fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }
}

pub(super) struct App {
    pub(super) home: PathBuf,
    pub(super) client: gateway::Client,
    pub(super) should_quit: bool,
    pub(super) notice: String,
    pub(super) pid: Option<u32>,
    pub(super) daemon_running: bool,
    pub(super) runtime_cfg: Config,
    pub(super) setup: SetupState,
    pub(super) chat: gateway::ChatSession,
    pub(super) input_buf: String,
    pub(super) input_cursor: usize,
    pub(super) input_preferred_col: Option<u16>,
    pub(super) animation_tick: u64,
    pub(super) static_panel_flushed: bool,
    pub(super) flushed_message_count: usize,
    pub(super) pending_scrollback_purge: bool,
    pub(super) input_focused: bool,
    pub(super) pending_update_check: Option<oneshot::Receiver<Result<AutoUpdateState, String>>>,
}

impl App {
    pub(super) fn new(home: PathBuf, cfg: Config, client: gateway::Client) -> Self {
        Self {
            home,
            client: client.clone(),
            should_quit: false,
            notice: String::new(),
            pid: None,
            daemon_running: false,
            runtime_cfg: cfg,
            setup: SetupState::default(),
            chat: gateway::ChatSession::new(client),
            input_buf: String::new(),
            input_cursor: 0,
            input_preferred_col: None,
            animation_tick: 0,
            static_panel_flushed: false,
            flushed_message_count: 0,
            pending_scrollback_purge: false,
            input_focused: true,
            pending_update_check: None,
        }
    }

    pub(super) fn runtime_daemon(&self) -> Daemon {
        Daemon::new(self.home.clone(), self.runtime_cfg.clone())
    }

    pub(super) fn config_file_path(&self) -> PathBuf {
        self.home.join("config.yaml")
    }

    pub(super) fn log_file_path(&self) -> PathBuf {
        crate::logger::current_daily_log_file_path(
            self.home.join("logs"),
            crate::logger::DAEMON_LOG_FILE_PREFIX,
        )
    }

    pub(super) fn setup_required(&self) -> bool {
        !self.setup.is_ready()
    }

    pub(super) fn chat_enabled(&self) -> bool {
        self.setup.is_ready() && self.daemon_running
    }

    pub(super) fn rebind_client(&mut self) {
        let client = self.client.rebased(self.runtime_cfg.base_url());
        self.client = client.clone();
        self.chat = gateway::ChatSession::new(client);
        self.input_buf.clear();
        self.input_cursor = 0;
        self.reset_message_view();
    }

    pub(super) fn reset_message_view(&mut self) {
        self.flushed_message_count = 0;
        self.input_focused = true;
    }

    pub(super) fn clear_message_view(&mut self) {
        self.reset_message_view();
        self.static_panel_flushed = false;
        self.pending_scrollback_purge = true;
    }

    pub(super) fn insert_input_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let chars: Vec<char> = self.input_buf.chars().collect();
        let mut new = String::with_capacity(self.input_buf.len() + text.len());
        new.extend(&chars[..self.input_cursor]);
        new.push_str(text);
        new.extend(&chars[self.input_cursor..]);
        let cursor = self.input_cursor + text.chars().count();
        let (normalized, cursor) = compact_cjk_spacing_with_cursor(&new, cursor);
        self.input_buf = normalized;
        self.input_cursor = cursor;
        self.input_preferred_col = None;
    }

    pub(super) fn handle_paste(&mut self, text: String) {
        if !self.chat_enabled() || self.chat.sending {
            return;
        }

        self.insert_input_text(&normalize_newlines(&text));
    }

    pub(super) async fn submit_input(&mut self) -> Result<(), BoxError> {
        if self.chat.sending {
            return Ok(());
        }

        let text = self.input_buf.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }

        if text == "/reload" {
            self.bootstrap().await;
            return Ok(());
        }

        let resets_display = gateway::is_new_conversation_command(&text);

        self.input_buf.clear();
        self.input_cursor = 0;
        self.input_preferred_col = None;
        if let Some(err) = self.chat.start_send(text) {
            self.notice = err;
        } else {
            self.notice.clear();
        }
        if resets_display {
            self.clear_message_view();
        }

        Ok(())
    }

    pub(super) async fn bootstrap(&mut self) {
        self.notice.clear();
        self.pid = None;
        self.daemon_running = false;
        self.setup = SetupState::default();
        self.pending_update_check = None;

        let daemon = self.runtime_daemon();
        let config_created = match daemon.ensure_config_file_exists().await {
            Ok(created) => created,
            Err(err) => {
                self.notice = format!(
                    "Failed to prepare {}: {err}",
                    daemon.config_file_path().display()
                );
                return;
            }
        };
        self.setup.template_created = config_created;

        self.runtime_cfg = match daemon.load_config_from_disk().await {
            Ok(cfg) => cfg,
            Err(err) => {
                self.notice = format!(
                    "Failed to read {}: {err}",
                    self.config_file_path().display()
                );
                return;
            }
        };
        self.setup.issues = self.runtime_cfg.setup_issues();
        self.rebind_client();

        if self.setup_required() {
            let missing = self.setup.issues.join(", ");
            self.notice = if self.setup.template_created {
                format!(
                    "Created {}. Fill in {} and press Enter to reload.",
                    self.config_file_path().display(),
                    missing
                )
            } else {
                format!(
                    "Edit {} and fill in {}. Press Enter after saving.",
                    self.config_file_path().display(),
                    missing
                )
            };
            let _ = self.refresh_status().await;
            return;
        }

        match self
            .client
            .ensure_daemon_running(&self.runtime_daemon())
            .await
        {
            Ok(LaunchState::AlreadyRunning) => {
                self.notice = format!("Connected to daemon at {}.", self.runtime_cfg.base_url());
            }
            Ok(LaunchState::Started(child)) => {
                self.notice = format!(
                    "Started daemon (pid {}). Logs: {}",
                    child.pid,
                    child.log_path.display()
                );
            }
            Err(err) => {
                self.notice = format!("Daemon unavailable: {err}. Press Enter to retry.");
            }
        }

        if let Err(err) = self.refresh_status().await {
            self.notice = format!("Status refresh failed: {err}");
        }

        if self.chat_enabled() {
            self.start_auto_update_check();
            match self.chat.restore_source_conversation().await {
                Ok(true) => self.reset_message_view(),
                Ok(false) => {}
                Err(err) => {
                    log::warn!("Failed to restore source conversation: {err}");
                    if self.notice.is_empty() {
                        self.notice = format!("Conversation restore failed: {err}");
                    }
                }
            }
        }
    }

    pub(super) fn start_auto_update_check(&mut self) {
        if self.pending_update_check.is_some() {
            return;
        }
        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(
                client
                    .auto_update_check()
                    .await
                    .map_err(|err| err.to_string()),
            );
        });
        self.pending_update_check = Some(rx);
    }

    pub(super) fn finish_pending_update_check(&mut self) -> bool {
        let Some(rx) = self.pending_update_check.as_mut() else {
            return false;
        };

        match rx.try_recv() {
            Ok(Ok(state)) => {
                self.pending_update_check = None;
                self.apply_update_state(state)
            }
            Ok(Err(err)) => {
                self.pending_update_check = None;
                log::warn!("auto update check failed: {err}");
                false
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_update_check = None;
                false
            }
        }
    }

    pub(super) fn apply_update_state(&mut self, state: AutoUpdateState) -> bool {
        let Some(notice) = state.cli_notice() else {
            return false;
        };
        if self.notice == notice {
            return false;
        }
        self.notice = notice;
        true
    }

    pub(super) async fn refresh_status(&mut self) -> Result<(), BoxError> {
        let daemon = self.runtime_daemon();
        self.pid = daemon.read_pid_file().await?;
        if let Some(pid) = self.pid
            && !process_exists(pid)
        {
            let _ = tokio::fs::remove_file(daemon.pid_file_path()).await;
            self.pid = None;
        }

        self.daemon_running = self.client.status().await.is_ok();
        Ok(())
    }

    // fn new_conversation(&mut self) {
    //     self.chat.reset();
    //     self.input_buf.clear();
    //     self.input_cursor = 0;
    //     self.reset_message_view();
    //     self.notice = "New conversation.".to_string();
    // }

    pub(super) async fn handle_key(
        &mut self,
        key: KeyEvent,
        input_content_width: u16,
    ) -> Result<(), BoxError> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('u') if self.chat_enabled() => {
                    self.input_buf.clear();
                    self.input_cursor = 0;
                    self.input_preferred_col = None;
                    return Ok(());
                }
                KeyCode::Char('a') if self.chat_enabled() => {
                    self.input_cursor = 0;
                    self.input_preferred_col = None;
                    return Ok(());
                }
                KeyCode::Char('e') if self.chat_enabled() => {
                    self.input_cursor = self.input_buf.chars().count();
                    self.input_preferred_col = None;
                    return Ok(());
                }
                _ => {}
            }
        }

        if !self.chat_enabled() {
            if key.code == KeyCode::Enter {
                self.bootstrap().await;
            }
            return Ok(());
        }

        if self.chat.sending {
            return Ok(());
        }

        if !self.input_focused {
            if key.code == KeyCode::Esc {
                self.notice.clear();
                return Ok(());
            }
            self.input_focused = true;
            if key.code == KeyCode::Enter {
                return Ok(());
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.notice.clear();
                self.input_focused = false;
            }
            _ if input_newline_key(key) => {
                self.insert_input_text("\n");
            }
            KeyCode::Enter => {
                self.submit_input().await?;
            }
            KeyCode::Backspace if self.input_cursor > 0 => {
                let chars: Vec<char> = self.input_buf.chars().collect();
                let pos = self.input_cursor - 1;
                self.input_buf = chars[..pos].iter().chain(chars[pos + 1..].iter()).collect();
                self.input_cursor -= 1;
                self.input_preferred_col = None;
            }
            KeyCode::Delete => {
                let chars: Vec<char> = self.input_buf.chars().collect();
                if self.input_cursor < chars.len() {
                    self.input_buf = chars[..self.input_cursor]
                        .iter()
                        .chain(chars[self.input_cursor + 1..].iter())
                        .collect();
                    self.input_preferred_col = None;
                }
            }
            KeyCode::Left if self.input_cursor > 0 => {
                self.input_cursor -= 1;
                self.input_preferred_col = None;
            }
            KeyCode::Right => {
                let len = self.input_buf.chars().count();
                if self.input_cursor < len {
                    self.input_cursor += 1;
                    self.input_preferred_col = None;
                }
            }
            KeyCode::Up => {
                self.move_input_cursor_vertically(InputCursorDirection::Up, input_content_width);
            }
            KeyCode::Down => {
                self.move_input_cursor_vertically(InputCursorDirection::Down, input_content_width);
            }
            KeyCode::Home => {
                self.input_cursor = 0;
                self.input_preferred_col = None;
            }
            KeyCode::End => {
                self.input_cursor = self.input_buf.chars().count();
                self.input_preferred_col = None;
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
            {
                let mut text = String::with_capacity(ch.len_utf8());
                text.push(ch);
                self.insert_input_text(&text);
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn move_input_cursor_vertically(
        &mut self,
        direction: InputCursorDirection,
        width: u16,
    ) {
        let (cursor, preferred_col) = move_cursor_vertically(
            &self.input_buf,
            self.input_cursor,
            width,
            direction,
            self.input_preferred_col,
        );
        self.input_cursor = cursor;
        self.input_preferred_col = Some(preferred_col);
    }
}
