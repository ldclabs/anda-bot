use std::{cell::RefCell, path::PathBuf};

use anda_core::BoxError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::oneshot;

use crate::{
    auto_update::AutoUpdateState,
    brain::AttentionPage,
    config::Config,
    daemon::{Daemon, LaunchState, process_exists},
    gateway,
};

use super::{
    action::{
        ACTION_RESPONSE_TIMEOUT, ActionApiOutput, TuiAction, TuiActionAnswer, TuiActionChoice,
        TuiActionChoiceDraft, TuiActionResponseRequest, TuiActionState, action_footer_line,
        action_line, action_response_notice, action_state_snapshot, active_pending_action,
        apply_action_response_to_message_value, apply_action_response_to_messages,
    },
    input::{
        InputCursorDirection, InputLayouts, cached_input_layout, cursor_byte_index,
        input_newline_key, next_cursor, previous_cursor,
    },
    text::normalize_newlines,
    transcript::NOTICE_ROLE,
};

type ActionResponseResult = Result<ActionApiOutput, String>;
type StatusResult = Result<(Option<u32>, bool), String>;
type MemoryResult = Result<MemoryReply, String>;

const DAEMON_LOST_NOTICE: &str = "Daemon connection lost. Press Enter to reconnect.";

/// Output of a `/memory` or `/brain` command.
pub(super) enum MemoryReply {
    Text(String),
    /// An inbox page; its item numbers address `/memory answer`.
    Inbox(AttentionPage),
}

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
    pub(super) pending_memory: Option<oneshot::Receiver<MemoryResult>>,
    pub(super) memory_inbox: Option<AttentionPage>,
    pub(super) pending_action_response: Option<oneshot::Receiver<ActionResponseResult>>,
    pub(super) choice_input: Option<TuiActionChoiceDraft>,
    pub(super) full_access: bool,
    pub(super) input_layouts: RefCell<InputLayouts>,
    pending_bootstrap: Option<oneshot::Receiver<Box<App>>>,
    pub(super) pending_status: Option<oneshot::Receiver<StatusResult>>,
    pending_chatgpt: Option<oneshot::Receiver<Result<(), String>>>,
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
            memory_inbox: None,
            pending_action_response: None,
            choice_input: None,
            full_access,
            input_layouts: RefCell::default(),
            pending_bootstrap: None,
            pending_status: None,
            pending_chatgpt: None,
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

        self.input_focused = true;
        self.insert_input_text(&normalize_newlines(&text));
    }

    pub(super) fn submit_input(&mut self) {
        if self.chat.sending {
            return;
        }

        if self.choice_input.is_some() {
            self.submit_choice_input();
            return;
        }

        let text = self.input_buf.trim().to_string();
        if text.is_empty() {
            return;
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
            return;
        }

        if text == "/reload" {
            self.clear_input();
            self.start_bootstrap();
            return;
        }

        if self.submit_memory_command(&text) {
            return;
        }

        let resets_display = gateway::is_new_conversation_command(&text);

        self.clear_input();
        self.chat.start_send(text);
        self.notice.clear();
        if resets_display {
            self.clear_message_view();
        }
    }

    /// Runs `/memory` and `/brain` commands locally; false for other text.
    /// The input stays when nothing starts, so it can be fixed or resent.
    fn submit_memory_command(&mut self, text: &str) -> bool {
        let is_command = |name: &str| {
            text.strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        };
        if !is_command("/memory") && !is_command("/brain") {
            return false;
        }

        if matches!(text, "/memory help" | "/memory guide") {
            self.clear_input();
            self.append_notice_message(crate::brain::product::MEMORY_GUIDE.into());
            return true;
        }
        if self.pending_memory.is_some() {
            self.notice = "A memory request is already running. / 记忆请求仍在进行".into();
            return true;
        }
        match self.start_memory_request(text) {
            Ok(notice) => {
                self.clear_input();
                self.notice = notice.into();
            }
            Err(hint) => self.notice = hint.into(),
        }
        true
    }

    /// Starts the request behind a memory command. Returns its progress
    /// notice, or a hint when the command cannot run.
    fn start_memory_request(&mut self, text: &str) -> Result<&'static str, &'static str> {
        let client = self.client.clone();
        let (notice, task) = if text.starts_with("/brain ") {
            let text = text.to_string();
            let task = spawn_task(async move {
                tokio::time::timeout(ACTION_RESPONSE_TIMEOUT, Self::brain_command(client, &text))
                    .await
                    .map_err(|_| "Brain request timed out.".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()))
                    .map(MemoryReply::Text)
            });
            ("Reading Brain…", task)
        } else if matches!(text, "/memory inbox" | "/memory next") {
            let cursor = if text == "/memory next" {
                let page = self.memory_inbox.as_ref();
                Some(
                    page.and_then(|page| page.next_cursor.clone())
                        .ok_or("No next inbox page / 没有下一页")?,
                )
            } else {
                None
            };
            // The numbers of a page that failed to load must not be answered.
            self.memory_inbox = None;
            let task = spawn_task(async move {
                client
                    .brain()
                    .attention(&crate::brain::AttentionQuery {
                        cursor,
                        limit: Some(20),
                    })
                    .await
                    .map(MemoryReply::Inbox)
                    .map_err(|error| error.to_string())
            });
            ("Reading inbox / 正在读取待办", task)
        } else if matches!(text, "/brain" | "/memory" | "/memory status") {
            let task = spawn_task(async move {
                client
                    .memory_overview()
                    .await
                    .map(|overview| MemoryReply::Text(overview.render()))
                    .map_err(|error| error.to_string())
            });
            ("Checking memory… / 正在检查记忆…", task)
        } else if text == "/memory activity" {
            let conversation = self
                .chat
                .conversation
                .as_ref()
                .map(|conversation| conversation._id.to_string())
                .ok_or("No current conversation / 当前还没有对话记录")?;
            let task = spawn_task(async move {
                client
                    .memory_activity(&crate::brain::activity::ActivityQuery {
                        conversation: Some(conversation),
                        cursor: None,
                        limit: Some(20),
                    })
                    .await
                    .map(|page| MemoryReply::Text(page.render()))
                    .map_err(|error| error.to_string())
            });
            ("Checking memory… / 正在检查记忆…", task)
        } else if let Some((retry, args)) = text
            .strip_prefix("/memory answer ")
            .map(|args| (false, args))
            .or_else(|| text.strip_prefix("/memory retry ").map(|args| (true, args)))
        {
            let (number, answer) = if retry {
                (args, "")
            } else {
                args.split_once(' ')
                    .ok_or("/memory answer <number> <text>")?
            };
            let item = number
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| self.memory_inbox.as_ref()?.items.get(n))
                .cloned()
                .ok_or("Open /memory inbox and select a visible item number / 请先查看待办编号")?;
            let home = self.home.clone();
            let answer = answer.to_string();
            let task = spawn_task(async move {
                if retry {
                    crate::brain::outbox::retry(&client, &home, &item).await
                } else {
                    crate::brain::outbox::reply(&client, &home, &item, answer).await
                }
                .map(|receipt| MemoryReply::Text(format!("{}\n/memory inbox", receipt.status)))
                .map_err(|error| error.to_string())
            });
            ("Sending answer / 正在提交回答", task)
        } else {
            return Err("Use /memory, /memory status or /memory help");
        };
        self.pending_memory = Some(task);
        Ok(notice)
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

    fn append_notice_message(&mut self, content: String) {
        self.chat.messages.push(notice_message(content));
        self.notice.clear();
    }

    pub(super) fn finish_pending_memory(&mut self) -> bool {
        let Some(result) = take_result(&mut self.pending_memory) else {
            return false;
        };
        match result {
            Some(Ok(MemoryReply::Text(content))) => self.append_notice_message(content),
            Some(Ok(MemoryReply::Inbox(page))) => {
                self.append_notice_message(crate::brain::outbox::render(&page));
                self.memory_inbox = Some(page);
            }
            Some(Err(error)) => self.notice = error,
            None => self.notice = "Memory request ended / 记忆请求已结束".into(),
        }
        true
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
        self.pending_bootstrap = Some(spawn_task(async move {
            connecting.bootstrap().await;
            connecting
        }));
        self.pending_status = None;
        self.pending_update_check = None;
        self.pending_memory = None;
        self.memory_inbox = None;
        self.pending_action_response = None;
        // A choice draft answers an action of the session being replaced;
        // an ordinary draft survives the reconnect.
        if self.choice_input.take().is_some() {
            self.clear_input();
        }
        self.notice = "Connecting to daemon… Ctrl+C quits.".into();
    }

    pub(super) fn finish_pending_bootstrap(&mut self) -> bool {
        let Some(result) = take_result(&mut self.pending_bootstrap) else {
            return false;
        };
        match result {
            Some(connected) => self.install_connection(*connected),
            None => {
                self.notice = "Connection task ended. Press Enter to retry.".into();
                self.daemon_running = false;
            }
        }
        true
    }

    /// Adopts the session a finished `bootstrap` built. Only a restored
    /// conversation replaces the written transcript; without one, new
    /// messages continue below it.
    pub(super) fn install_connection(&mut self, connected: App) {
        if !connected.chat.messages.is_empty() && self.flushed_message_count > 0 {
            self.clear_message_view();
        }
        self.flushed_message_count = 0;
        self.action_states.clear();
        self.active_action = None;
        self.actions_key = (u64::MAX, usize::MAX);
        self.runtime_cfg = connected.runtime_cfg;
        self.client = connected.client;
        self.setup = connected.setup;
        self.chat = connected.chat;
        self.pid = connected.pid;
        self.daemon_running = connected.daemon_running;
        self.notice = connected.notice;
        self.pending_update_check = connected.pending_update_check;
    }

    pub(super) fn start_chatgpt_login(&mut self) {
        if self.pending_chatgpt.is_some() {
            return;
        }
        let client = self.client.clone();
        let daemon = self.runtime_daemon();
        let (tx, rx) = oneshot::channel();
        self.notice="Continue with ChatGPT in your browser. Press Ctrl+C to leave; the pending sign-in will be cancelled.".into();
        self.pending_chatgpt = Some(rx);
        tokio::spawn(async move {
            use crate::chatgpt::{LoginView, api::Request};
            let result: Result<(), BoxError> = async {
                client.ensure_daemon_running(&daemon).await?;
                let flow: LoginView = serde_json::from_value(
                    client
                        .chatgpt(&Request::LoginStart {
                            profile_id: None,
                            port: 0,
                            consent: false,
                        })
                        .await?,
                )?;
                if let Err(error) = crate::cli::auth::open_browser(
                    flow.authorization_url
                        .as_deref()
                        .ok_or("missing sign-in URL")?,
                ) {
                    let _ = client
                        .chatgpt(&Request::LoginCancel {
                            flow_id: flow.flow_id.clone(),
                        })
                        .await;
                    return Err(format!(
                        "{error}. Use `anda auth login chatgpt --no-browser` in a terminal."
                    )
                    .into());
                }
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if tx.is_closed() {
                        let _ = client
                            .chatgpt(&Request::LoginCancel {
                                flow_id: flow.flow_id.clone(),
                            })
                            .await;
                        return Err("sign-in cancelled".into());
                    }
                    let status: LoginView = serde_json::from_value(
                        client
                            .chatgpt(&Request::LoginStatus {
                                flow_id: flow.flow_id.clone(),
                            })
                            .await?,
                    )?;
                    if status.status == "completed" {
                        let profile = status.account_id.ok_or("missing connected account")?;
                        let models = client
                            .chatgpt(&Request::Models {
                                profile_id: profile.clone(),
                            })
                            .await?;
                        let model = models
                            .pointer("/models/0/slug")
                            .and_then(|v| v.as_str())
                            .ok_or("No eligible models; use ChatGPT account settings")?;
                        client
                            .chatgpt(&Request::ModelSelect {
                                profile_id: profile,
                                model: model.into(),
                            })
                            .await?;
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        return Ok(());
                    }
                    if !matches!(status.status.as_str(), "pending" | "exchanging") {
                        return Err(status.error.unwrap_or(status.status).into());
                    }
                }
            }
            .await;
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
    }
    pub(super) fn finish_pending_chatgpt(&mut self) -> bool {
        let Some(result) = take_result(&mut self.pending_chatgpt) else {
            return false;
        };
        match result {
            Some(Ok(())) => self.start_bootstrap(),
            Some(Err(error)) => self.notice = error,
            None => self.notice = "ChatGPT sign-in ended; press Ctrl+G to retry.".into(),
        }
        true
    }

    pub(super) fn start_status_refresh(&mut self) {
        if self.pending_status.is_some() || self.pending_bootstrap.is_some() {
            return;
        }
        let daemon = self.runtime_daemon();
        let client = self.client.clone();
        self.pending_status = Some(spawn_task(async move {
            Self::fetch_status(daemon, client)
                .await
                .map_err(|error| error.to_string())
        }));
    }

    pub(super) fn finish_pending_status(&mut self) -> bool {
        let Some(Some(result)) = take_result(&mut self.pending_status) else {
            return false;
        };
        match result {
            Ok((pid, running)) => {
                let changed = self.pid != pid || self.daemon_running != running;
                if self.daemon_running && !running {
                    self.notice = DAEMON_LOST_NOTICE.into();
                } else if running && self.notice == DAEMON_LOST_NOTICE {
                    self.notice.clear();
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

    /// Connects a fresh `App` (see `start_bootstrap`): loads the config,
    /// starts or reaches the daemon and restores the active conversation.
    pub(super) async fn bootstrap(&mut self) {
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
        self.client = self.client.rebased(self.runtime_cfg.base_url());
        self.chat =
            gateway::ChatSession::new(self.client.clone()).with_full_access(self.full_access);

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
            if let Err(err) = self.chat.restore_source_conversation().await {
                log::warn!("Failed to restore source conversation: {err}");
                if self.notice.is_empty() {
                    self.notice = format!("Conversation restore failed: {err}");
                }
            }
        }
    }

    pub(super) fn start_auto_update_check(&mut self) {
        if self.pending_update_check.is_some() {
            return;
        }
        let client = self.client.clone();
        self.pending_update_check = Some(spawn_task(async move {
            client
                .auto_update_check()
                .await
                .map_err(|err| err.to_string())
        }));
    }

    pub(super) fn finish_pending_update_check(&mut self) -> bool {
        match take_result(&mut self.pending_update_check) {
            Some(Some(Ok(state))) => self.apply_update_state(state),
            Some(Some(Err(err))) => {
                log::warn!("auto update check failed: {err}");
                false
            }
            _ => false,
        }
    }

    pub(super) fn finish_pending_action_response(&mut self) -> bool {
        let Some(result) = take_result(&mut self.pending_action_response) else {
            return false;
        };
        match result {
            Some(result) => self.apply_action_response_result(result),
            None => self.notice = "Action response task cancelled.".to_string(),
        }
        true
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

    pub(super) fn handle_key(&mut self, key: KeyEvent, input_content_width: u16) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('g') if !self.chat.sending => {
                    self.start_chatgpt_login();
                    return;
                }
                KeyCode::Char('c') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('u') if self.chat_enabled() && !self.action_response_pending() => {
                    self.clear_input();
                    return;
                }
                KeyCode::Char('a') if self.chat_enabled() => {
                    self.input_cursor = 0;
                    self.input_preferred_col = None;
                    return;
                }
                KeyCode::Char('e') if self.chat_enabled() => {
                    self.input_cursor = self.input_buf.chars().count();
                    self.input_preferred_col = None;
                    return;
                }
                _ => {}
            }
        }

        if !self.chat_enabled() {
            if key.code == KeyCode::Enter {
                self.start_bootstrap();
            }
            return;
        }

        if self.choice_input.is_some()
            && !self.action_response_pending()
            && key.code == KeyCode::Esc
            && !key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            self.cancel_choice_input();
            return;
        }

        if self.chat.sending {
            return;
        }

        if self.action_response_pending() {
            return;
        }

        if self.choice_input.is_none() && self.input_buf.is_empty() && self.handle_action_key(key) {
            return;
        }

        if !self.input_focused {
            if key.code == KeyCode::Esc {
                self.notice.clear();
                return;
            }
            self.input_focused = true;
            if key.code == KeyCode::Enter {
                return;
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
            KeyCode::Enter => self.submit_input(),
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
        self.pending_action_response = Some(spawn_task(async move {
            client
                .tool_call_with_timeout(&input, ACTION_RESPONSE_TIMEOUT)
                .await
                .map(|output| output.output)
                .map_err(|err| err.to_string())
        }));
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
        // A choice draft for an action resolved elsewhere (expired, picked by
        // default, answered from another client) can no longer be sent.
        if let Some(draft) = &self.choice_input
            && !self.action_response_pending()
            && !states
                .iter()
                .any(|state| state.id == draft.action_id && state.status == "pending")
        {
            self.choice_input = None;
            self.clear_input();
        }
        self.active_action = active_pending_action(&self.chat.messages);
        self.action_states = states;
        self.chat
            .messages
            .extend(receipts.into_iter().map(notice_message));
        self.actions_key = (self.chat.revision(), self.chat.messages.len());
        true
    }

    pub(super) fn action_footer_line(&self, width: usize) -> Option<ratatui::text::Line<'static>> {
        if self.action_response_pending() {
            return Some(action_line("responding...", width));
        }
        if let Some(draft) = &self.choice_input {
            let text = format!(
                "{} · Enter submit · Esc cancel",
                draft.placeholder.as_deref().unwrap_or(&draft.label)
            );
            return Some(action_line(&text, width));
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

fn notice_message(text: String) -> anda_core::Message {
    anda_core::Message {
        role: NOTICE_ROLE.into(),
        content: vec![text.into()],
        ..Default::default()
    }
}

/// Runs `task` in the background; collect its output with `take_result`.
fn spawn_task<T: Send + 'static>(
    task: impl Future<Output = T> + Send + 'static,
) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(task.await);
    });
    rx
}

/// `None` while the task runs. Once it ends, empties `slot` and returns its
/// output, or `Some(None)` when the task died without one.
fn take_result<T>(slot: &mut Option<oneshot::Receiver<T>>) -> Option<Option<T>> {
    let output = match slot.as_mut()?.try_recv() {
        Ok(output) => Some(output),
        Err(oneshot::error::TryRecvError::Empty) => return None,
        Err(oneshot::error::TryRecvError::Closed) => None,
    };
    *slot = None;
    Some(output)
}
