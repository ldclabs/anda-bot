use anda_core::BoxError;
use anda_engine::memory::ConversationStatus;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    daemon::{Daemon, DaemonArgs, process_exists},
    gateway,
};

mod theme;
mod widgets;

use self::widgets::InfoPanel;

// ── Config form constants ──────────────────────────────────────────────
const FIELD_COUNT: usize = 7;
const ACTION_SAVE: usize = FIELD_COUNT;
const ACTION_APPLY: usize = FIELD_COUNT + 1;
const ACTION_STOP: usize = FIELD_COUNT + 2;
const ACTION_RELOAD: usize = FIELD_COUNT + 3;
const ACTION_CHAT: usize = FIELD_COUNT + 4;
const ACTION_QUIT: usize = FIELD_COUNT + 5;
const TOTAL_CONFIG_ITEMS: usize = FIELD_COUNT + 6;

const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

pub async fn run(daemon: Daemon, client: gateway::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client);
    app.bootstrap().await;

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let run_result = run_app(&mut terminal, &mut app).await;

    let raw_mode_result = disable_raw_mode();
    let mut stdout = io::stdout();
    let screen_result = stdout.execute(LeaveAlternateScreen);

    raw_mode_result?;
    screen_result?;
    run_result
}

// ── View mode ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Config,
    Chat,
}

// ── Application state ──────────────────────────────────────────────────
struct App {
    // common
    home: PathBuf,
    client: gateway::Client,
    mode: Mode,
    should_quit: bool,
    notice: String,
    pid: Option<u32>,
    daemon_running: bool,

    // ── config mode ────────────────────────────────────────────────────
    runtime_cfg: DaemonArgs,
    persisted_cfg: DaemonArgs,
    draft_cfg: DaemonArgs,
    selected: usize,

    // ── chat mode ──────────────────────────────────────────────────────
    chat: gateway::ChatSession,
    input_buf: String,
    input_cursor: usize,
    scroll_offset: usize,
}

impl App {
    fn new(home: PathBuf, cfg: DaemonArgs, client: gateway::Client) -> Self {
        Self {
            home,
            client: client.clone(),
            mode: Mode::Config,
            should_quit: false,
            notice: String::new(),
            pid: None,
            daemon_running: false,

            runtime_cfg: cfg.clone(),
            persisted_cfg: cfg.clone(),
            draft_cfg: cfg,
            selected: 0,

            input_buf: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            chat: gateway::ChatSession::new(client.clone()),
        }
    }

    // ── bootstrap ──────────────────────────────────────────────────────
    async fn bootstrap(&mut self) {
        match self
            .client
            .ensure_daemon_running(&self.runtime_daemon())
            .await
        {
            Ok(gateway::LaunchState::AlreadyRunning) => {
                self.notice = format!("Connected to daemon at {}.", self.runtime_cfg.base_url());
            }
            Ok(gateway::LaunchState::Started(child)) => {
                self.notice = format!(
                    "Started daemon (pid {}). Logs: {}",
                    child.pid,
                    child.log_path.display()
                );
            }
            Err(err) => {
                self.notice = format!("Auto-start failed: {err}");
            }
        }
        if let Err(err) = self.refresh_status().await {
            self.notice = format!("Status refresh failed: {err}");
        }
    }

    fn runtime_daemon(&self) -> Daemon {
        Daemon::new(self.home.clone(), self.runtime_cfg.clone())
    }

    fn draft_daemon(&self) -> Daemon {
        Daemon::new(self.home.clone(), self.draft_cfg.clone())
    }

    fn is_dirty(&self) -> bool {
        self.draft_cfg != self.persisted_cfg
    }

    // ── Config navigation helpers ──────────────────────────────────────
    fn next_item(&mut self) {
        self.selected = (self.selected + 1) % TOTAL_CONFIG_ITEMS;
    }

    fn prev_item(&mut self) {
        self.selected = if self.selected == 0 {
            TOTAL_CONFIG_ITEMS - 1
        } else {
            self.selected - 1
        };
    }

    fn field_label(index: usize) -> &'static str {
        match index {
            0 => "addr",
            1 => "sandbox",
            2 => "model_family",
            3 => "model_name",
            4 => "model_api_key",
            5 => "model_api_base",
            6 => "https_proxy",
            _ => "",
        }
    }

    fn field_value(&self, index: usize) -> String {
        match index {
            0 => display_value(&self.draft_cfg.addr),
            1 => {
                if self.draft_cfg.sandbox {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            2 => display_value(&self.draft_cfg.model_family),
            3 => display_value(&self.draft_cfg.model_name),
            4 => mask_secret(&self.draft_cfg.model_api_key),
            5 => display_value(&self.draft_cfg.model_api_base),
            6 => display_optional(self.draft_cfg.https_proxy.as_deref()),
            _ => String::new(),
        }
    }

    fn field_is_empty(&self, index: usize) -> bool {
        match index {
            0 => self.draft_cfg.addr.is_empty(),
            1 => false,
            2 => self.draft_cfg.model_family.is_empty(),
            3 => self.draft_cfg.model_name.is_empty(),
            4 => self.draft_cfg.model_api_key.is_empty(),
            5 => self.draft_cfg.model_api_base.is_empty(),
            6 => self
                .draft_cfg
                .https_proxy
                .as_deref()
                .is_none_or(|v| v.is_empty()),
            _ => true,
        }
    }

    fn action_label(&self, index: usize) -> &'static str {
        match index {
            ACTION_SAVE => "[ Save ]",
            ACTION_APPLY if self.daemon_running || self.pid.is_some() => "[ Apply+Restart ]",
            ACTION_APPLY => "[ Apply+Start ]",
            ACTION_STOP => "[ Stop ]",
            ACTION_RELOAD => "[ Reload ]",
            ACTION_CHAT => "[ Chat ]",
            ACTION_QUIT => "[ Quit ]",
            _ => "",
        }
    }

    fn toggle_selected_bool(&mut self) {
        if self.selected == 1 {
            self.draft_cfg.sandbox = !self.draft_cfg.sandbox;
        }
    }

    fn push_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        match self.selected {
            0 => self.draft_cfg.addr.push(ch),
            2 => self.draft_cfg.model_family.push(ch),
            3 => self.draft_cfg.model_name.push(ch),
            4 => self.draft_cfg.model_api_key.push(ch),
            5 => self.draft_cfg.model_api_base.push(ch),
            6 => self
                .draft_cfg
                .https_proxy
                .get_or_insert_with(String::new)
                .push(ch),
            _ => {}
        }
    }

    fn pop_char(&mut self) {
        match self.selected {
            0 => {
                self.draft_cfg.addr.pop();
            }
            2 => {
                self.draft_cfg.model_family.pop();
            }
            3 => {
                self.draft_cfg.model_name.pop();
            }
            4 => {
                self.draft_cfg.model_api_key.pop();
            }
            5 => {
                self.draft_cfg.model_api_base.pop();
            }
            6 => {
                if let Some(proxy) = self.draft_cfg.https_proxy.as_mut() {
                    proxy.pop();
                    if proxy.is_empty() {
                        self.draft_cfg.https_proxy = None;
                    }
                }
            }
            _ => {}
        }
    }

    fn clear_selected_field(&mut self) {
        match self.selected {
            0 => self.draft_cfg.addr.clear(),
            1 => self.draft_cfg.sandbox = false,
            2 => self.draft_cfg.model_family.clear(),
            3 => self.draft_cfg.model_name.clear(),
            4 => self.draft_cfg.model_api_key.clear(),
            5 => self.draft_cfg.model_api_base.clear(),
            6 => self.draft_cfg.https_proxy = None,
            _ => {}
        }
    }

    // ── daemon operations ──────────────────────────────────────────────
    async fn refresh_status(&mut self) -> Result<(), BoxError> {
        let daemon = self.runtime_daemon();
        self.pid = daemon.read_pid_file().await?;
        if let Some(pid) = self.pid
            && !process_exists(pid)
        {
            let _ = tokio::fs::remove_file(daemon.pid_file_path()).await;
            self.pid = None;
        }
        match self.client.status().await {
            Ok(_) => {
                self.daemon_running = true;
            }
            Err(_) => {
                self.daemon_running = false;
            }
        }
        Ok(())
    }

    async fn save_only(&mut self) -> Result<(), BoxError> {
        let daemon = self.draft_daemon();
        daemon.persist_config().await?;
        self.persisted_cfg = self.draft_cfg.clone();
        self.notice = format!("Saved to {}.", daemon.env_file_path().display());
        Ok(())
    }

    async fn apply_and_restart(&mut self) -> Result<(), BoxError> {
        let daemon = self.draft_daemon();
        daemon.persist_config().await?;
        self.persisted_cfg = self.draft_cfg.clone();

        self.runtime_daemon()
            .stop_background(Duration::from_secs(10))
            .await?;

        let child = daemon.spawn_background()?;
        if let Err(err) = self
            .client
            .wait_for_daemon_ready(Duration::from_secs(20))
            .await
        {
            self.notice = format!("Daemon not ready. Logs: {}", child.log_path.display());
            let _ = self.refresh_status().await;
            return Err(format!("{err}; logs: {}", child.log_path.display()).into());
        }

        self.runtime_cfg = self.draft_cfg.clone();
        self.notice = format!("Daemon (pid {}) on {}.", child.pid, daemon.base_url());
        self.refresh_status().await?;
        Ok(())
    }

    async fn stop_daemon(&mut self) -> Result<(), BoxError> {
        let stopped = self
            .runtime_daemon()
            .stop_background(Duration::from_secs(10))
            .await?;
        self.notice = if stopped {
            "Daemon stopped.".to_string()
        } else {
            "Daemon was not running.".to_string()
        };
        self.refresh_status().await?;
        Ok(())
    }

    fn reload_saved(&mut self) {
        self.draft_cfg = self.persisted_cfg.clone();
        self.notice = "Reloaded saved config.".to_string();
    }

    // ── config mode key handler ────────────────────────────────────────
    async fn handle_config_key(&mut self, key: KeyEvent) -> Result<(), BoxError> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('s') => return self.save_only().await,
                KeyCode::Char('r') => return self.apply_and_restart().await,
                KeyCode::Char('x') => return self.stop_daemon().await,
                KeyCode::Char('l') => {
                    self.reload_saved();
                    return Ok(());
                }
                KeyCode::Char('u') => {
                    self.clear_selected_field();
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') if self.selected >= FIELD_COUNT => self.should_quit = true,
            KeyCode::Up | KeyCode::BackTab => self.prev_item(),
            KeyCode::Down | KeyCode::Tab => self.next_item(),
            KeyCode::Enter | KeyCode::Char(' ') if self.selected >= ACTION_SAVE => {
                match self.selected {
                    ACTION_SAVE => self.save_only().await?,
                    ACTION_APPLY => self.apply_and_restart().await?,
                    ACTION_STOP => self.stop_daemon().await?,
                    ACTION_RELOAD => self.reload_saved(),
                    ACTION_CHAT => {
                        if !self.daemon_running {
                            self.notice = "Daemon not running. Start it first.".to_string();
                        } else {
                            self.mode = Mode::Chat;
                            self.notice.clear();
                        }
                    }
                    ACTION_QUIT => self.should_quit = true,
                    _ => {}
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') if self.selected == 1 => {
                self.toggle_selected_bool();
            }
            KeyCode::Backspace if self.selected < FIELD_COUNT => self.pop_char(),
            KeyCode::Delete if self.selected < FIELD_COUNT => self.clear_selected_field(),
            KeyCode::Char(ch) if self.selected < FIELD_COUNT => {
                if self.selected == 1 {
                    match ch {
                        't' | 'T' | 'y' | 'Y' | '1' => self.draft_cfg.sandbox = true,
                        'f' | 'F' | 'n' | 'N' | '0' => self.draft_cfg.sandbox = false,
                        _ => {}
                    }
                } else if !key.modifiers.intersects(KeyModifiers::ALT) {
                    self.push_char(ch);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── chat operations ────────────────────────────────────────────────
    fn new_conversation(&mut self) {
        self.chat.reset();
        self.scroll_offset = 0;
        self.input_buf.clear();
        self.input_cursor = 0;
        self.notice = "New conversation.".to_string();
    }

    // ── chat mode key handler ──────────────────────────────────────────
    async fn handle_chat_key(&mut self, key: KeyEvent) -> Result<(), BoxError> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('n') => {
                    self.new_conversation();
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    self.mode = Mode::Config;
                    return Ok(());
                }
                KeyCode::Char('u') => {
                    self.input_buf.clear();
                    self.input_cursor = 0;
                    return Ok(());
                }
                KeyCode::Char('a') => {
                    self.input_cursor = 0;
                    return Ok(());
                }
                KeyCode::Char('e') => {
                    self.input_cursor = self.input_buf.chars().count();
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Config;
            }
            KeyCode::Enter => {
                if !self.chat.sending {
                    let text = self.input_buf.trim().to_string();
                    if !text.is_empty() {
                        self.input_buf.clear();
                        self.input_cursor = 0;
                        self.scroll_offset = usize::MAX;
                        if let Some(err) = self.chat.send(text).await {
                            self.notice = err;
                        } else {
                            self.notice.clear();
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    let chars: Vec<char> = self.input_buf.chars().collect();
                    let pos = self.input_cursor - 1;
                    self.input_buf = chars[..pos].iter().chain(chars[pos + 1..].iter()).collect();
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Delete => {
                let chars: Vec<char> = self.input_buf.chars().collect();
                if self.input_cursor < chars.len() {
                    self.input_buf = chars[..self.input_cursor]
                        .iter()
                        .chain(chars[self.input_cursor + 1..].iter())
                        .collect();
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let len = self.input_buf.chars().count();
                if self.input_cursor < len {
                    self.input_cursor += 1;
                }
            }
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input_buf.chars().count(),
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
            }
            KeyCode::Char(ch) => {
                if !key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
                {
                    let chars: Vec<char> = self.input_buf.chars().collect();
                    let mut new = String::with_capacity(self.input_buf.len() + ch.len_utf8());
                    new.extend(&chars[..self.input_cursor]);
                    new.push(ch);
                    new.extend(&chars[self.input_cursor..]);
                    self.input_buf = new;
                    self.input_cursor += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── unified key handler ────────────────────────────────────────────
    async fn handle_key(&mut self, key: KeyEvent) {
        let result = match self.mode {
            Mode::Config => self.handle_config_key(key).await,
            Mode::Chat => self.handle_chat_key(key).await,
        };
        if let Err(err) = result {
            self.notice = err.to_string();
        }
    }
}

// ── Event loop ─────────────────────────────────────────────────────────

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let mut last_status_refresh = Instant::now();
    loop {
        terminal.draw(|frame| render(frame, app))?;

        if app.should_quit {
            break;
        }

        // Periodic background work
        if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL && app.mode == Mode::Config {
            let _ = app.refresh_status().await;
            last_status_refresh = Instant::now();
        }

        if app.mode == Mode::Chat {
            if app.chat.poll().await {
                app.scroll_offset = usize::MAX;
            }
            if let Some(reason) = app.chat.failed_reason.take() {
                app.notice = format!("Failed: {reason}");
            }
        }

        if !event::poll(Duration::from_millis(150))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            app.handle_key(key).await;
        }
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
//  Rendering
// ════════════════════════════════════════════════════════════════════════

fn render(frame: &mut Frame, app: &mut App) {
    match app.mode {
        Mode::Config => render_config(frame, app),
        Mode::Chat => render_chat(frame, app),
    }
}

// ── Config view ────────────────────────────────────────────────────────
fn render_config(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(4),    // form
            Constraint::Length(2), // notice
        ])
        .split(area);

    // Title bar
    let status_label = if app.daemon_running {
        Span::styled(" RUNNING ", theme::success_style())
    } else if app.pid.is_some() {
        Span::styled(" UNHEALTHY ", theme::warn_style())
    } else {
        Span::styled(" STOPPED ", theme::danger_style())
    };
    let dirty_label = if app.is_dirty() {
        Span::styled(" *modified", theme::warn_style())
    } else {
        Span::raw("")
    };
    let title_line = Line::from(vec![
        Span::styled(" ANDA ", theme::title_style()),
        Span::styled("Config", theme::heading_style()),
        Span::raw(" │ "),
        status_label,
        Span::raw(" "),
        Span::styled(app.runtime_cfg.base_url(), theme::dim_style()),
        dirty_label,
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    // Form
    let mut lines = Vec::new();
    for i in 0..FIELD_COUNT {
        let sel = app.selected == i;
        let lbl_style = if sel {
            theme::selected_style()
        } else {
            theme::body_style().add_modifier(Modifier::BOLD)
        };
        let val_style = if sel {
            theme::selected_style()
        } else if app.field_is_empty(i) {
            theme::dim_style()
        } else {
            theme::input_style()
        };
        lines.push(Line::from(vec![
            Span::styled(if sel { "▸ " } else { "  " }, lbl_style),
            Span::styled(format!("{:<16}", App::field_label(i)), lbl_style),
            Span::styled(app.field_value(i), val_style),
        ]));
    }
    lines.push(Line::from(""));
    for i in ACTION_SAVE..TOTAL_CONFIG_ITEMS {
        let sel = app.selected == i;
        let style = if sel {
            theme::selected_style()
        } else {
            match i {
                ACTION_APPLY => theme::accent_style(),
                ACTION_STOP => theme::danger_style(),
                ACTION_CHAT => theme::heading_style(),
                _ => theme::body_style(),
            }
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", if sel { "▸ " } else { "  " }, app.action_label(i)),
            style,
        )));
    }

    frame.render_widget(InfoPanel { title: "", lines }, chunks[1]);

    // Notice / shortcuts
    render_config_footer(frame, app, chunks[2]);
}

fn render_config_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if !app.notice.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" notice: ", theme::dim_style()),
            Span::styled(&app.notice, theme::body_style()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " ↑↓ nav │ Enter activate │ ^S save │ ^R apply │ ^X stop │ q quit",
        theme::dim_style(),
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

// ── Chat view ──────────────────────────────────────────────────────────
fn render_chat(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(3),    // message area
            Constraint::Length(3), // input box
            Constraint::Length(1), // status line
        ])
        .split(area);

    // Title bar
    let conv_label = if let Some(id) = app.chat.conversation_id {
        format!("#{id}")
    } else {
        "new".to_string()
    };
    let status_style = match &app.chat.conv_status {
        Some(ConversationStatus::Working) | Some(ConversationStatus::Submitted) => {
            theme::warn_style()
        }
        Some(ConversationStatus::Completed) => theme::success_style(),
        Some(ConversationStatus::Failed) | Some(ConversationStatus::Cancelled) => {
            theme::danger_style()
        }
        None => theme::dim_style(),
    };
    let title_line = Line::from(vec![
        Span::styled(" ANDA ", theme::title_style()),
        Span::styled("Chat", theme::heading_style()),
        Span::raw(" │ "),
        Span::styled(&conv_label, theme::body_style()),
        Span::raw(" "),
        Span::styled(app.chat.status_label(), status_style),
        Span::raw("  "),
        Span::styled("Esc:config ^N:new ^Q:quit", theme::dim_style()),
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    // Messages
    render_messages(frame, app, chunks[1]);

    // Input
    render_input(frame, app, chunks[2]);

    // Status line
    let status_line = if !app.notice.is_empty() {
        Line::from(Span::styled(
            format!(" {}", &app.notice),
            theme::warn_style(),
        ))
    } else {
        Line::from(Span::styled(
            " /steer, /stop, /cancel │ ↑↓ scroll │ Enter send",
            theme::dim_style(),
        ))
    };
    frame.render_widget(Paragraph::new(status_line), chunks[3]);
}

fn render_messages(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.chat.messages.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Type a message and press Enter to start.",
                theme::dim_style(),
            )),
            inner,
        );
        return;
    }

    let width = inner.width as usize;
    let mut rendered_lines: Vec<Line> = Vec::new();

    for msg in &app.chat.messages {
        let (prefix, style) = match msg.role.as_str() {
            "user" => ("▶ You: ", theme::accent_style()),
            "assistant" => ("◀ Bot: ", theme::success_style()),
            "system" => ("● Sys: ", theme::warn_style()),
            _ => ("  ", theme::dim_style()),
        };

        let wrap_w = width.saturating_sub(2).max(1);
        let mut first = true;

        for text_line in msg.text.lines() {
            if text_line.is_empty() {
                rendered_lines.push(if first {
                    first = false;
                    Line::from(Span::styled(prefix.to_string(), style))
                } else {
                    Line::from("")
                });
                continue;
            }

            let mut remaining = text_line;
            while !remaining.is_empty() {
                let take = if remaining.len() > wrap_w {
                    let mut end = wrap_w;
                    while end > 0 && !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    if end == 0 {
                        wrap_w.min(remaining.len())
                    } else {
                        end
                    }
                } else {
                    remaining.len()
                };
                let chunk = &remaining[..take];
                remaining = &remaining[take..];

                if first {
                    first = false;
                    rendered_lines.push(Line::from(vec![
                        Span::styled(prefix.to_string(), style),
                        Span::styled(chunk.to_string(), theme::body_style()),
                    ]));
                } else {
                    rendered_lines.push(Line::from(Span::styled(
                        format!("  {chunk}"),
                        theme::body_style(),
                    )));
                }
            }
        }

        // Add a blank separator between messages
        rendered_lines.push(Line::from(""));
    }

    let visible_h = inner.height as usize;
    let total = rendered_lines.len();

    // Clamp scroll offset
    if total <= visible_h {
        app.scroll_offset = 0;
    } else if app.scroll_offset >= total.saturating_sub(visible_h) {
        app.scroll_offset = total.saturating_sub(visible_h);
    }

    let visible: Vec<Line> = rendered_lines
        .into_iter()
        .skip(app.scroll_offset)
        .take(visible_h)
        .collect();

    frame.render_widget(Paragraph::new(visible), inner);

    // Scrollbar
    if total > visible_h {
        let mut scrollbar_state =
            ScrollbarState::new(total.saturating_sub(visible_h)).position(app.scroll_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar_state,
        );
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = if app.chat.sending {
        " Sending… "
    } else if app.chat.is_active() {
        " ▸ follow-up "
    } else {
        " ▸ "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if app.chat.sending {
            theme::dim_style()
        } else {
            theme::accent_style()
        })
        .title(Span::styled(prompt, theme::heading_style()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(app.input_buf.as_str())
        .style(theme::body_style())
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);

    // Cursor position: calculate display width of chars before cursor
    let display_col: u16 = app
        .input_buf
        .chars()
        .take(app.input_cursor)
        .map(|c| c.width().unwrap_or(0) as u16)
        .sum();
    let cursor_x = inner.x + display_col;
    if cursor_x < inner.x + inner.width {
        frame.set_cursor_position((cursor_x, inner.y));
    }
}

// ════════════════════════════════════════════════════════════════════════
//  Helpers
// ════════════════════════════════════════════════════════════════════════

fn display_value(value: &str) -> String {
    if value.trim().is_empty() {
        "(empty)".to_string()
    } else {
        value.to_string()
    }
}

fn display_optional(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => "(unset)".to_string(),
    }
}

fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return "(empty)".to_string();
    }
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{}{}", "*".repeat(chars.len() - 4), tail)
}

#[cfg(unix)]
fn reopen_stdin_from_tty() -> Result<(), BoxError> {
    if io::stdin().is_terminal() {
        return Ok(());
    }

    use std::{fs::File, os::unix::io::IntoRawFd};

    let tty = File::open("/dev/tty")?;
    let fd = tty.into_raw_fd();
    unsafe {
        if libc::dup2(fd, 0) == -1 {
            libc::close(fd);
            return Err(io::Error::last_os_error().into());
        }
        libc::close(fd);
    }
    Ok(())
}
