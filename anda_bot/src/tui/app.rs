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
    action::{
        ACTION_RESPONSE_TIMEOUT, TuiAction, TuiActionApiOutput, TuiActionChoiceDraft,
        TuiActionResponseRequest, action_footer_line, action_response_notice,
        active_pending_action, apply_action_response_to_message_value,
        apply_action_response_to_messages,
    },
    input::{InputCursorDirection, input_newline_key, move_cursor_vertically},
    text::{compact_cjk_spacing_with_cursor, normalize_newlines},
};

type ActionResponseResult = Result<TuiActionApiOutput, String>;

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
    pub(super) pending_action_response: Option<oneshot::Receiver<ActionResponseResult>>,
    pub(super) choice_input: Option<TuiActionChoiceDraft>,
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
            pending_action_response: None,
            choice_input: None,
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
        self.choice_input = None;
        self.pending_action_response = None;
        self.reset_message_view();
    }

    pub(super) fn reset_message_view(&mut self) {
        self.flushed_message_count = 0;
        self.input_focused = true;
    }

    pub(super) fn clear_message_view(&mut self) {
        self.reset_message_view();
        self.refresh_message_view();
    }

    pub(super) fn refresh_message_view(&mut self) {
        self.flushed_message_count = 0;
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
        if !self.chat_enabled() || self.chat.sending || self.action_response_pending() {
            return;
        }

        self.insert_input_text(&normalize_newlines(&text));
    }

    pub(super) async fn submit_input(&mut self) -> Result<(), BoxError> {
        if self.chat.sending {
            return Ok(());
        }

        if self.choice_input.is_some() {
            return self.submit_choice_input().await;
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

    async fn submit_choice_input(&mut self) -> Result<(), BoxError> {
        if self.action_response_pending() {
            return Ok(());
        }

        let Some(draft) = self.choice_input.clone() else {
            return Ok(());
        };

        let text = self.input_buf.trim().to_string();
        if draft.required && text.is_empty() {
            self.notice = "Choice text is required.".to_string();
            return Ok(());
        }

        self.input_buf.clear();
        self.input_cursor = 0;
        self.input_preferred_col = None;
        self.choice_input = None;
        self.start_action_response(TuiActionResponseRequest::choice(
            draft.action_id,
            draft.choice_id,
            (!text.is_empty()).then_some(text),
        ));

        Ok(())
    }

    pub(super) async fn bootstrap(&mut self) {
        self.notice.clear();
        self.pid = None;
        self.daemon_running = false;
        self.setup = SetupState::default();
        self.pending_update_check = None;
        self.pending_action_response = None;
        self.choice_input = None;

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

    pub(super) async fn finish_pending_action_response(&mut self) -> bool {
        let Some(rx) = self.pending_action_response.as_mut() else {
            return false;
        };

        match rx.try_recv() {
            Ok(result) => {
                self.pending_action_response = None;
                self.apply_action_response_result(result).await;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_action_response = None;
                self.notice = "Action response task cancelled.".to_string();
                true
            }
        }
    }

    async fn apply_action_response_result(&mut self, result: ActionResponseResult) {
        match result {
            Ok(output) => {
                let mut updated =
                    apply_action_response_to_messages(&mut self.chat.messages, &output);
                if let Some(conversation) = self.chat.conversation.as_mut() {
                    for message in &mut conversation.messages {
                        updated |= apply_action_response_to_message_value(message, &output);
                    }
                }
                if updated {
                    self.refresh_message_view();
                }
                self.notice = action_response_notice(&output);
                if output.conversation > 0 {
                    let _ = self.chat.poll(Some(output.conversation)).await;
                }
            }
            Err(err) => {
                self.notice = format!("Action response failed: {err}");
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

        if self.choice_input.is_some()
            && key.code == KeyCode::Esc
            && !key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            self.cancel_choice_input();
            return Ok(());
        }

        if self.chat.sending {
            return Ok(());
        }

        if self.action_response_pending() {
            return Ok(());
        }

        if self.choice_input.is_none()
            && self.input_buf.is_empty()
            && self.handle_action_key(key).await?
        {
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

    async fn handle_action_key(&mut self, key: KeyEvent) -> Result<bool, BoxError> {
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            return Ok(false);
        }

        let Some(action) = self.active_pending_action() else {
            return Ok(false);
        };

        match key.code {
            KeyCode::Char('y' | 'Y') if action.is_approval() => {
                self.start_action_response(TuiActionResponseRequest::approve(action.id, true));
                Ok(true)
            }
            KeyCode::Char('n' | 'N') if action.is_approval() => {
                self.start_action_response(TuiActionResponseRequest::approve(action.id, false));
                Ok(true)
            }
            KeyCode::Char(ch) => {
                let Some(choice) = action.choice_for_key(ch).cloned() else {
                    return Ok(false);
                };
                if choice.input.is_some() {
                    self.choice_input = Some(action.choice_draft(&choice));
                    self.input_buf.clear();
                    self.input_cursor = 0;
                    self.input_preferred_col = None;
                    self.input_focused = true;
                } else {
                    self.start_action_response(TuiActionResponseRequest::choice(
                        action.id, choice.id, None,
                    ));
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn start_action_response(&mut self, request: TuiActionResponseRequest) {
        if self.pending_action_response.is_some() {
            return;
        }

        let input = request.tool_input();
        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let result = client
                .tool_call_with_timeout(&input, ACTION_RESPONSE_TIMEOUT)
                .await
                .map(|output| output.output)
                .map_err(|err| err.to_string());
            let _ = tx.send(result);
        });

        self.pending_action_response = Some(rx);
        self.notice = "Responding to action...".to_string();
    }

    fn cancel_choice_input(&mut self) {
        self.choice_input = None;
        self.input_buf.clear();
        self.input_cursor = 0;
        self.input_preferred_col = None;
        self.notice.clear();
    }

    pub(super) fn action_response_pending(&self) -> bool {
        self.pending_action_response.is_some()
    }

    pub(super) fn active_pending_action(&self) -> Option<TuiAction> {
        active_pending_action(&self.chat.messages)
    }

    pub(super) fn action_footer_line(&self, width: usize) -> Option<ratatui::text::Line<'static>> {
        if self.action_response_pending() {
            return Some(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled("ACTION ", super::theme::accent_style()),
                ratatui::text::Span::styled("responding...", super::theme::subtle_style()),
            ]));
        }
        if let Some(draft) = &self.choice_input {
            let text = format!(
                "{} · Enter submit · Esc cancel",
                draft.placeholder.as_deref().unwrap_or(&draft.label)
            );
            return Some(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled("ACTION ", super::theme::accent_style()),
                ratatui::text::Span::styled(
                    super::text::truncate_visual(&text, width.saturating_sub(7)),
                    super::theme::subtle_style(),
                ),
            ]));
        }
        let action = self.active_pending_action()?;
        action_footer_line(&action, width)
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
