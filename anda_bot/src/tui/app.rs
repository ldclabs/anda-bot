use std::{cell::RefCell, path::PathBuf};

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
        ACTION_RESPONSE_TIMEOUT, ActionApiOutput, TuiAction, TuiActionAnswer, TuiActionChoice,
        TuiActionChoiceDraft, TuiActionResponseRequest, TuiActionState, action_footer_line,
        action_response_notice, action_state_snapshot, active_pending_action,
        apply_action_response_to_message_value, apply_action_response_to_messages,
    },
    input::{
        InputCursorDirection, InputLayouts, cached_input_layout, cursor_byte_index,
        input_newline_key, next_cursor, previous_cursor,
    },
    text::normalize_newlines,
};

type ActionResponseResult = Result<ActionApiOutput, String>;
type StatusResult = Result<(Option<u32>, bool), String>;

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
    pub(super) pending_memory: Option<oneshot::Receiver<Result<String, String>>>,
    pub(super) pending_memory_inbox:
        Option<oneshot::Receiver<Result<crate::brain::AttentionPage, String>>>,
    pub(super) memory_inbox: Option<crate::brain::AttentionPage>,
    pub(super) pending_action_response: Option<oneshot::Receiver<ActionResponseResult>>,
    pub(super) choice_input: Option<TuiActionChoiceDraft>,
    pub(super) full_access: bool,
    pub(super) input_layouts: RefCell<InputLayouts>,
    pending_bootstrap: Option<oneshot::Receiver<Box<App>>>,
    pending_status: Option<oneshot::Receiver<StatusResult>>,
    actions_key: (u64, usize),
    action_states: Vec<TuiActionState>,
    active_action: Option<TuiAction>,
}

impl App {
    pub(super) fn new(
        home: PathBuf,
        cfg: Config,
        client: gateway::Client,
        full_access: bool,
    ) -> Self {
        Self {
            home,
            client: client.clone(),
            should_quit: false,
            notice: String::new(),
            pid: None,
            daemon_running: false,
            runtime_cfg: cfg,
            setup: SetupState::default(),
            chat: gateway::ChatSession::new(client).with_full_access(full_access),
            input_buf: String::new(),
            input_cursor: 0,
            input_preferred_col: None,
            animation_tick: 0,
            static_panel_flushed: false,
            flushed_message_count: 0,
            pending_scrollback_purge: false,
            input_focused: true,
            pending_update_check: None,
            pending_memory: None,
            pending_memory_inbox: None,
            memory_inbox: None,
            pending_action_response: None,
            choice_input: None,
            full_access,
            input_layouts: RefCell::default(),
            pending_bootstrap: None,
            pending_status: None,
            actions_key: (0, 0),
            action_states: Vec::new(),
            active_action: None,
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
        self.setup.is_ready() && self.daemon_running && self.pending_bootstrap.is_none()
    }

    pub(super) fn rebind_client(&mut self) {
        let client = self.client.rebased(self.runtime_cfg.base_url());
        self.client = client.clone();
        self.chat = gateway::ChatSession::new(client).with_full_access(self.full_access);
        self.clear_input();
        self.choice_input = None;
        self.pending_action_response = None;
        // An initial bind has no transcript to replace. In particular it must
        // not purge the shell's scrollback before this TUI has written anything.
        if self.flushed_message_count > 0 {
            self.clear_message_view();
        }
        self.action_states.clear();
        self.active_action = None;
        self.actions_key = (self.chat.revision(), 0);
    }

    pub(super) fn clear_message_view(&mut self) {
        self.flushed_message_count = 0;
        self.static_panel_flushed = false;
        self.pending_scrollback_purge = true;
        self.input_focused = true;
        self.action_states.clear();
        self.active_action = None;
        self.actions_key = (u64::MAX, usize::MAX);
    }

    pub(super) fn clear_input(&mut self) {
        self.input_buf.clear();
        self.input_cursor = 0;
        self.input_preferred_col = None;
    }

    pub(super) fn insert_input_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let byte = cursor_byte_index(&self.input_buf, self.input_cursor);
        self.input_buf.insert_str(byte, text);
        self.input_cursor += text.chars().count();
        // Inserting a combining mark/ZWJ can join the following grapheme.
        if self.input_cursor > 0 {
            self.input_cursor = next_cursor(&self.input_buf, self.input_cursor - 1);
        }
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
            self.submit_choice_input();
            return Ok(());
        }

        let text = self.input_buf.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }

        // A stray keystroke puts text in the composer, which routes the
        // single-key answers to the composer instead. Accept the same answer
        // typed out so a pending action never becomes unanswerable and has to
        // be waited out until it expires.
        if let Some(action) = self.active_pending_action()
            && let Some(answer) = action.answer_from_text(&text)
        {
            self.clear_input();
            self.answer_action(&action, answer);
            return Ok(());
        }

        if text == "/reload" {
            self.start_bootstrap();
            return Ok(());
        }

        if text == "/memory inbox" || text == "/memory next" {
            let cursor = if text == "/memory next" {
                self.memory_inbox
                    .as_ref()
                    .and_then(|page| page.next_cursor.clone())
            } else {
                None
            };
            if text == "/memory next" && cursor.is_none() {
                self.notice = "No next inbox page / 没有下一页".into();
                return Ok(());
            }
            if self.pending_memory_inbox.is_none() {
                let client = self.client.clone();
                let (tx, rx) = oneshot::channel();
                tokio::spawn(async move {
                    let result = client
                        .brain()
                        .attention(&crate::brain::AttentionQuery {
                            cursor,
                            limit: Some(20),
                        })
                        .await
                        .map_err(|error| error.to_string());
                    let _ = tx.send(result);
                });
                self.pending_memory_inbox = Some(rx);
                self.notice = "Reading inbox / 正在读取待办".into();
            }
            self.clear_input();
            return Ok(());
        }
        if let Some(answer) = text
            .strip_prefix("/memory answer ")
            .or_else(|| text.strip_prefix("/memory retry "))
        {
            let retry = text.starts_with("/memory retry ");
            let Some((number, answer)) = (if retry {
                Some((answer, ""))
            } else {
                answer.split_once(' ')
            }) else {
                self.notice = "/memory answer <number> <text>".into();
                return Ok(());
            };
            let item = number
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| {
                    self.memory_inbox
                        .as_ref()
                        .and_then(|page| page.items.get(n))
                })
                .cloned();
            let Some(item) = item else {
                self.notice =
                    "Open /memory inbox and select a visible item number / 请先查看待办编号".into();
                return Ok(());
            };
            if self.pending_memory.is_none() {
                let client = self.client.clone();
                let home = self.home.clone();
                let text = answer.to_string();
                let (tx, rx) = oneshot::channel();
                tokio::spawn(async move {
                    let result = if retry {
                        crate::brain::outbox::retry(&client, &home, &item).await
                    } else {
                        crate::brain::outbox::reply(&client, &home, &item, text).await
                    }
                    .map(|receipt| format!("{}\n/memory inbox", receipt.status))
                    .map_err(|error| error.to_string());
                    let _ = tx.send(result);
                });
                self.pending_memory = Some(rx);
                self.notice = "Sending answer / 正在提交回答".into();
            }
            self.clear_input();
            return Ok(());
        }

        if text == "/brain" || text == "/memory" || text.starts_with("/memory ") {
            self.clear_input();
            if text == "/memory help" || text == "/memory guide" {
                self.append_memory_output(crate::brain::product::MEMORY_GUIDE.into());
            } else if text == "/brain"
                || text == "/memory"
                || text == "/memory status"
                || text == "/memory activity"
            {
                if self.pending_memory.is_none() {
                    let client = self.client.clone();
                    let (tx, rx) = oneshot::channel();
                    let conversation = self.chat.conversation.as_ref().map(|c| c._id.to_string());
                    tokio::spawn(async move {
                        if text == "/memory activity" {
                            let result = match conversation {
                                Some(conversation) => client
                                    .memory_activity(&crate::brain::activity::ActivityQuery {
                                        conversation: Some(conversation),
                                        cursor: None,
                                        limit: Some(20),
                                    })
                                    .await
                                    .map(|p| p.render())
                                    .map_err(|e| e.to_string()),
                                None => Ok("No current conversation / 当前还没有对话记录".into()),
                            };
                            let _ = tx.send(result);
                            return;
                        }
                        let _ = tx.send(
                            client
                                .memory_overview()
                                .await
                                .map(|v| v.render())
                                .map_err(|e| e.to_string()),
                        );
                    });
                    self.pending_memory = Some(rx);
                    self.notice = "Checking memory… / 正在检查记忆…".into();
                }
            } else {
                self.notice = "Use /memory, /memory status or /memory help".into();
            }
            return Ok(());
        }

        if text.starts_with("/brain ") {
            if self.pending_memory.is_some() {
                self.notice = "A memory request is already running.".into();
                return Ok(());
            }
            let client = self.client.clone();
            let (tx, rx) = oneshot::channel();
            tokio::spawn(async move {
                let result = tokio::time::timeout(
                    ACTION_RESPONSE_TIMEOUT,
                    Self::brain_command(client, &text),
                )
                .await
                .map_err(|_| "Brain request timed out.".to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
                let _ = tx.send(result);
            });
            self.pending_memory = Some(rx);
            self.clear_input();
            self.notice = "Reading Brain…".into();
            return Ok(());
        }

        let resets_display = gateway::is_new_conversation_command(&text);

        self.clear_input();
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

    async fn brain_command(client: gateway::Client, text: &str) -> Result<String, BoxError> {
        let brain = client.brain();
        let command = text.strip_prefix("/brain").unwrap_or_default().trim();
        let result = if command == "status" {
            serde_json::to_value(brain.runtime_status().await?)?
        } else if let Some(id) = command.strip_prefix("formation ") {
            serde_json::to_value(brain.formation_conversation(id.parse()?).await?)?
        } else if command.is_empty() || command == "inbox" || command.starts_with("next ") {
            serde_json::to_value(
                brain
                    .attention(&crate::brain::AttentionQuery {
                        cursor: command.strip_prefix("next ").map(|s| s.trim().to_string()),
                        limit: Some(20),
                    })
                    .await?,
            )?
        } else if command.starts_with("answer ") || command.starts_with("statement ") {
            let mut parts = command.splitn(4, ' ');
            let kind = parts.next().unwrap_or_default();
            let id = parts.next().ok_or("missing attention id")?;
            let event_key = parts.next().ok_or("missing stable event key")?.to_string();
            let answer = parts
                .next()
                .filter(|s| !s.trim().is_empty())
                .ok_or("missing response text")?
                .to_string();
            let response = if kind == "answer" {
                crate::brain::AttentionResponse::Clarification { event_key, answer }
            } else {
                crate::brain::AttentionResponse::AgentStatement {
                    event_key,
                    statement: answer,
                }
            };
            serde_json::to_value(brain.respond(id, &response).await?)?
        } else {
            return Ok("/brain inbox · /brain status · /brain next <cursor>\n/brain answer <id> <event_key> <answer>\n/brain statement <id> <event_key> <statement>\nRetry with the same event key and text. Answers do not grant execution authority. / 重试须保留相同事件键和正文；回答不授予执行权限。".into());
        };
        Ok(format!(
            "Brain\n```json\n{}\n```",
            serde_json::to_string_pretty(&result)?
        ))
    }

    fn append_memory_output(&mut self, content: String) {
        self.chat.messages.push(anda_core::Message {
            role: "system".into(),
            content: vec![content.into()],
            ..Default::default()
        });
        self.notice.clear();
    }

    pub(super) fn finish_pending_memory(&mut self) -> bool {
        let Some(rx) = self.pending_memory.as_mut() else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.pending_memory = None;
                match result {
                    Ok(content) => self.append_memory_output(content),
                    Err(error) => self.notice = error,
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_memory = None;
                self.notice = "Memory status request ended / 记忆状态查询已结束".into();
                true
            }
        }
    }

    pub(super) fn finish_pending_memory_inbox(&mut self) -> bool {
        let Some(receiver) = self.pending_memory_inbox.as_mut() else {
            return false;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.pending_memory_inbox = None;
                match result {
                    Ok(page) => {
                        self.append_memory_output(crate::brain::outbox::render(&page));
                        self.memory_inbox = Some(page);
                    }
                    Err(error) => {
                        self.memory_inbox = None;
                        self.notice = error;
                    }
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_memory_inbox = None;
                false
            }
        }
    }

    fn submit_choice_input(&mut self) {
        if self.action_response_pending() {
            return;
        }

        let Some(draft) = self.choice_input.clone() else {
            return;
        };

        let text = self.input_buf.trim().to_string();
        if draft.required && text.is_empty() {
            self.notice = "Choice text is required.".to_string();
            return;
        }

        self.start_action_response(TuiActionResponseRequest::choice(
            draft.action_id,
            draft.choice_id,
            (!text.is_empty()).then_some(text),
        ));
    }

    pub(super) fn start_bootstrap(&mut self) {
        if self.pending_bootstrap.is_some() {
            return;
        }
        let mut connecting = Box::new(Self::new(
            self.home.clone(),
            self.runtime_cfg.clone(),
            self.client.clone(),
            self.full_access,
        ));
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            connecting.bootstrap().await;
            let _ = tx.send(connecting);
        });
        self.pending_status = None;
        self.pending_update_check = None;
        self.pending_memory = None;
        self.pending_memory_inbox = None;
        self.memory_inbox = None;
        self.pending_action_response = None;
        self.choice_input = None;
        self.clear_input();
        self.pending_bootstrap = Some(rx);
        self.notice = "Connecting to daemon… Ctrl+C quits.".into();
    }

    pub(super) fn finish_pending_bootstrap(&mut self) -> bool {
        let Some(rx) = self.pending_bootstrap.as_mut() else {
            return false;
        };
        match rx.try_recv() {
            Ok(connected) => {
                self.pending_bootstrap = None;
                if self.flushed_message_count > 0 {
                    self.clear_message_view();
                }
                self.runtime_cfg = connected.runtime_cfg;
                self.client = connected.client;
                self.setup = connected.setup;
                self.chat = connected.chat;
                self.pid = connected.pid;
                self.daemon_running = connected.daemon_running;
                self.notice = connected.notice;
                self.pending_update_check = connected.pending_update_check;
                self.actions_key = (u64::MAX, usize::MAX);
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_bootstrap = None;
                self.notice = "Connection task ended. Press Enter to retry.".into();
                self.daemon_running = false;
                true
            }
        }
    }

    pub(super) fn start_status_refresh(&mut self) {
        if self.pending_status.is_some() || self.pending_bootstrap.is_some() {
            return;
        }
        let daemon = self.runtime_daemon();
        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(
                Self::fetch_status(daemon, client)
                    .await
                    .map_err(|error| error.to_string()),
            );
        });
        self.pending_status = Some(rx);
    }

    pub(super) fn finish_pending_status(&mut self) -> bool {
        let Some(rx) = self.pending_status.as_mut() else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.pending_status = None;
                match result {
                    Ok((pid, running)) => {
                        let changed = self.pid != pid || self.daemon_running != running;
                        if self.daemon_running && !running {
                            self.notice =
                                "Daemon connection lost. Press Enter to reconnect.".into();
                        }
                        self.pid = pid;
                        self.daemon_running = running;
                        changed
                    }
                    Err(error) => {
                        self.notice = format!("Status refresh failed: {error}");
                        true
                    }
                }
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_status = None;
                false
            }
        }
    }

    pub(super) async fn bootstrap(&mut self) {
        self.notice.clear();
        self.pid = None;
        self.daemon_running = false;
        self.setup = SetupState::default();
        self.pending_update_check = None;
        self.pending_memory = None;
        self.pending_memory_inbox = None;
        self.memory_inbox = None;
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

        let connected = match self
            .client
            .ensure_daemon_running(&self.runtime_daemon())
            .await
        {
            Ok(LaunchState::AlreadyRunning) => {
                self.notice = format!("Connected to daemon at {}.", self.runtime_cfg.base_url());
                true
            }
            Ok(LaunchState::Started(child)) => {
                self.notice = format!(
                    "Started daemon (pid {}). Logs: {}",
                    child.pid,
                    child.log_path.display()
                );
                true
            }
            Err(err) => {
                self.notice = format!("Daemon unavailable: {err}. Press Enter to retry.");
                false
            }
        };

        if connected {
            let registration = match std::env::current_dir() {
                Ok(workspace) => self.client.register_cli_workspace(&workspace).await,
                Err(err) => Err(err.into()),
            };
            if let Err(err) = registration {
                self.notice =
                    format!("Cannot register CLI workspace: {err}. Press Enter to retry.");
                return;
            }
        }

        if let Err(err) = self.refresh_status().await {
            self.notice = format!("Status refresh failed: {err}");
        }

        if self.chat_enabled() {
            self.start_auto_update_check();
            match self.chat.restore_source_conversation().await {
                // clear (not reset): the restore refetches the full history,
                // which must replace the scrollback instead of piling on top.
                Ok(true) if self.flushed_message_count > 0 => self.clear_message_view(),
                Ok(true) => {}
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

    pub(super) fn finish_pending_action_response(&mut self) -> bool {
        let Some(rx) = self.pending_action_response.as_mut() else {
            return false;
        };

        match rx.try_recv() {
            Ok(result) => {
                self.pending_action_response = None;
                self.apply_action_response_result(result);
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

    fn apply_action_response_result(&mut self, result: ActionResponseResult) {
        match result {
            Ok(output) => {
                apply_action_response_to_messages(&mut self.chat.messages, &output);
                self.chat.mark_changed();
                if self
                    .choice_input
                    .as_ref()
                    .is_some_and(|draft| draft.action_id == output.action_id)
                {
                    self.choice_input = None;
                    self.clear_input();
                }
                if let Some(conversation) = self.chat.conversation.as_mut() {
                    for message in &mut conversation.messages {
                        apply_action_response_to_message_value(message, &output);
                    }
                }
                self.notice = action_response_notice(&output);
                if output.conversation > 0 {
                    self.chat.start_poll(Some(output.conversation));
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
        let (pid, running) = Self::fetch_status(self.runtime_daemon(), self.client.clone()).await?;
        self.pid = pid;
        self.daemon_running = running;
        Ok(())
    }

    async fn fetch_status(
        daemon: Daemon,
        client: gateway::Client,
    ) -> Result<(Option<u32>, bool), BoxError> {
        let mut pid = daemon.read_pid_file().await?;
        if let Some(value) = pid
            && !process_exists(value)
        {
            let _ = tokio::fs::remove_file(daemon.pid_file_path()).await;
            pid = None;
        }
        Ok((pid, client.status().await.is_ok()))
    }

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
                KeyCode::Char('u') if self.chat_enabled() && !self.action_response_pending() => {
                    self.clear_input();
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
                self.start_bootstrap();
            }
            return Ok(());
        }

        if self.choice_input.is_some()
            && !self.action_response_pending()
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

        if self.choice_input.is_none() && self.input_buf.is_empty() && self.handle_action_key(key) {
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
                let previous = previous_cursor(&self.input_buf, self.input_cursor);
                let range = cursor_byte_index(&self.input_buf, previous)
                    ..cursor_byte_index(&self.input_buf, self.input_cursor);
                self.input_buf.replace_range(range, "");
                self.input_cursor = previous;
                self.input_preferred_col = None;
            }
            KeyCode::Delete => {
                let next = next_cursor(&self.input_buf, self.input_cursor);
                let range = cursor_byte_index(&self.input_buf, self.input_cursor)
                    ..cursor_byte_index(&self.input_buf, next);
                self.input_buf.replace_range(range, "");
                self.input_preferred_col = None;
            }
            KeyCode::Left => {
                self.input_cursor = previous_cursor(&self.input_buf, self.input_cursor);
                self.input_preferred_col = None;
            }
            KeyCode::Right => {
                self.input_cursor = next_cursor(&self.input_buf, self.input_cursor);
                self.input_preferred_col = None;
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

    fn handle_action_key(&mut self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            return false;
        }

        let Some(action) = self.active_pending_action() else {
            return false;
        };

        match key.code {
            KeyCode::Char('y' | 'Y') if action.is_approval() => {
                self.answer_action(&action, TuiActionAnswer::Approve(true));
                true
            }
            KeyCode::Char('n' | 'N') if action.is_approval() => {
                self.answer_action(&action, TuiActionAnswer::Approve(false));
                true
            }
            KeyCode::Char(ch) => {
                let Some(choice) = action.choice_for_key(ch).cloned() else {
                    // The keystroke falls through into the composer, which
                    // deactivates the shortcuts. Say how to answer from there,
                    // otherwise the card looks stuck until it expires.
                    if let Some(notice) = action.unanswered_notice() {
                        self.notice = notice;
                    }
                    return false;
                };
                self.answer_action(&action, TuiActionAnswer::Choice(choice));
                true
            }
            _ => false,
        }
    }

    fn answer_action(&mut self, action: &TuiAction, answer: TuiActionAnswer) {
        match answer {
            TuiActionAnswer::Approve(approve) => {
                self.start_action_response(TuiActionResponseRequest::approve(
                    action.id.clone(),
                    approve,
                ));
            }
            TuiActionAnswer::Choice(choice) => self.answer_choice(action, &choice),
        }
    }

    fn answer_choice(&mut self, action: &TuiAction, choice: &TuiActionChoice) {
        if choice.input.is_some() {
            self.choice_input = Some(action.choice_draft(choice));
            self.clear_input();
            self.input_focused = true;
            return;
        }

        self.start_action_response(TuiActionResponseRequest::choice(
            action.id.clone(),
            choice.id.clone(),
            None,
        ));
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
        self.clear_input();
        self.notice.clear();
    }

    pub(super) fn action_response_pending(&self) -> bool {
        self.pending_action_response.is_some()
    }

    pub(super) fn active_pending_action(&self) -> Option<TuiAction> {
        if self.actions_key == (self.chat.revision(), self.chat.messages.len()) {
            self.active_action.clone()
        } else {
            active_pending_action(&self.chat.messages)
        }
    }

    /// Reconcile only after a chat mutation, never on animation or typing ticks.
    /// Scrollback is append-only, so resolved cards get one durable receipt.
    pub(super) fn refresh_actions(&mut self) -> bool {
        let key = (self.chat.revision(), self.chat.messages.len());
        if self.actions_key == key {
            return false;
        }
        let states = action_state_snapshot(&self.chat.messages);
        let receipts: Vec<_> = states
            .iter()
            .filter(|state| {
                state.status != "pending"
                    && self
                        .action_states
                        .iter()
                        .any(|old| old.id == state.id && old != *state)
            })
            .map(|state| format!("Action {} {}.", state.id, state.status))
            .collect();
        self.active_action = active_pending_action(&self.chat.messages);
        self.action_states = states;
        for receipt in receipts {
            self.chat.messages.push(anda_core::Message {
                role: "system".into(),
                content: vec![receipt.into()],
                ..Default::default()
            });
        }
        self.actions_key = (self.chat.revision(), self.chat.messages.len());
        true
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
        if self.actions_key == (self.chat.revision(), self.chat.messages.len()) {
            return self
                .active_action
                .as_ref()
                .and_then(|action| action_footer_line(action, width, self.input_buf.is_empty()));
        }
        let action = self.active_pending_action()?;
        action_footer_line(&action, width, self.input_buf.is_empty())
    }

    pub(super) fn move_input_cursor_vertically(
        &mut self,
        direction: InputCursorDirection,
        width: u16,
    ) {
        let (cursor, preferred_col) = cached_input_layout(self, width.max(1)).move_cursor(
            self.input_cursor,
            direction,
            self.input_preferred_col,
        );
        self.input_cursor = cursor;
        self.input_preferred_col = Some(preferred_col);
    }
}
