use anda_core::BoxError;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span, Text},
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

use self::widgets::{Banner, InfoPanel};

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

#[derive(Default)]
struct SetupState {
    template_created: bool,
    missing_fields: Vec<&'static str>,
}

impl SetupState {
    fn is_ready(&self) -> bool {
        self.missing_fields.is_empty()
    }
}

struct App {
    home: PathBuf,
    client: gateway::Client,
    should_quit: bool,
    notice: String,
    pid: Option<u32>,
    daemon_running: bool,
    runtime_cfg: DaemonArgs,
    setup: SetupState,
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
            should_quit: false,
            notice: String::new(),
            pid: None,
            daemon_running: false,
            runtime_cfg: cfg,
            setup: SetupState::default(),
            chat: gateway::ChatSession::new(client),
            input_buf: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
        }
    }

    fn runtime_daemon(&self) -> Daemon {
        Daemon::new(self.home.clone(), self.runtime_cfg.clone())
    }

    fn env_file_path(&self) -> PathBuf {
        self.home.join(".env")
    }

    fn log_file_path(&self) -> PathBuf {
        self.home.join("logs").join("anda-daemon.log")
    }

    fn setup_required(&self) -> bool {
        !self.setup.is_ready()
    }

    fn chat_enabled(&self) -> bool {
        self.setup.is_ready() && self.daemon_running
    }

    fn rebind_client(&mut self) {
        let client = self.client.rebased(self.runtime_cfg.base_url());
        self.client = client.clone();
        self.chat = gateway::ChatSession::new(client);
        self.input_buf.clear();
        self.input_cursor = 0;
        self.scroll_offset = 0;
    }

    async fn bootstrap(&mut self) {
        self.notice.clear();
        self.pid = None;
        self.daemon_running = false;
        self.setup = SetupState::default();

        let daemon = self.runtime_daemon();
        self.setup.template_created = match daemon.ensure_env_file_exists().await {
            Ok(created) => created,
            Err(err) => {
                self.notice = format!(
                    "Failed to prepare {}: {err}",
                    daemon.env_file_path().display()
                );
                return;
            }
        };

        let env_path = daemon.env_file_path();
        self.runtime_cfg = match DaemonArgs::from_env_file(&env_path).await {
            Ok(cfg) => cfg,
            Err(err) => {
                self.notice = format!("Failed to read {}: {err}", env_path.display());
                return;
            }
        };
        self.setup.missing_fields = self.runtime_cfg.missing_required_fields();
        self.rebind_client();

        if self.setup_required() {
            let missing = self.setup.missing_fields.join(", ");
            self.notice = if self.setup.template_created {
                format!(
                    "Created {}. Fill in {} and press Ctrl+R.",
                    env_path.display(),
                    missing
                )
            } else {
                format!(
                    "Edit {} and fill in {}. Press Ctrl+R after saving.",
                    env_path.display(),
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
                self.notice = format!("Daemon unavailable: {err}. Press Ctrl+R to retry.");
            }
        }

        if let Err(err) = self.refresh_status().await {
            self.notice = format!("Status refresh failed: {err}");
        }
    }

    async fn refresh_status(&mut self) -> Result<(), BoxError> {
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

    fn new_conversation(&mut self) {
        self.chat.reset();
        self.scroll_offset = 0;
        self.input_buf.clear();
        self.input_cursor = 0;
        self.notice = "New conversation.".to_string();
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<(), BoxError> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    self.bootstrap().await;
                    return Ok(());
                }
                KeyCode::Char('n') if self.chat_enabled() => {
                    self.new_conversation();
                    return Ok(());
                }
                KeyCode::Char('u') if self.chat_enabled() => {
                    self.input_buf.clear();
                    self.input_cursor = 0;
                    return Ok(());
                }
                KeyCode::Char('a') if self.chat_enabled() => {
                    self.input_cursor = 0;
                    return Ok(());
                }
                KeyCode::Char('e') if self.chat_enabled() => {
                    self.input_cursor = self.input_buf.chars().count();
                    return Ok(());
                }
                _ => {}
            }
        }

        if !self.chat_enabled() {
            if key.code == KeyCode::Enter {
                self.notice = if self.setup_required() {
                    format!(
                        "Edit {} and fill in {}. Press Ctrl+R when ready.",
                        self.env_file_path().display(),
                        self.setup.missing_fields.join(", ")
                    )
                } else {
                    format!(
                        "Daemon not ready. Check {} and press Ctrl+R to reconnect.",
                        self.log_file_path().display()
                    )
                };
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.notice.clear();
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
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
            {
                let chars: Vec<char> = self.input_buf.chars().collect();
                let mut new = String::with_capacity(self.input_buf.len() + ch.len_utf8());
                new.extend(&chars[..self.input_cursor]);
                new.push(ch);
                new.extend(&chars[self.input_cursor..]);
                self.input_buf = new;
                self.input_cursor += 1;
            }
            _ => {}
        }
        Ok(())
    }
}

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

        if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
            let was_running = app.daemon_running;
            let _ = app.refresh_status().await;
            if app.setup.is_ready() && was_running && !app.daemon_running && app.notice.is_empty() {
                app.notice = "Daemon connection lost. Press Ctrl+R to reload .env and reconnect."
                    .to_string();
            }
            last_status_refresh = Instant::now();
        }

        if app.chat_enabled() {
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
            if let Err(err) = app.handle_key(key).await {
                app.notice = err.to_string();
            }
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    let body = if area.width >= 110 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(60), Constraint::Length(34)])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(12), Constraint::Length(9)])
            .split(chunks[1])
    };

    render_title(frame, app, chunks[0]);
    render_main_panel(frame, app, body[0]);
    render_sidebar(frame, app, body[1]);
    render_input(frame, app, chunks[2]);
    render_status_line(frame, app, chunks[3]);
}

fn render_title(frame: &mut Frame, app: &App, area: Rect) {
    let status_label = if app.setup_required() {
        Span::styled(" SETUP REQUIRED ", theme::danger_style())
    } else if app.daemon_running {
        Span::styled(" READY ", theme::success_style())
    } else if app.pid.is_some() {
        Span::styled(" UNHEALTHY ", theme::warn_style())
    } else {
        Span::styled(" OFFLINE ", theme::danger_style())
    };

    let title_line = Line::from(vec![
        Span::styled(" ANDA ", theme::title_style()),
        Span::styled("Chat", theme::heading_style()),
        Span::raw(" │ "),
        status_label,
        Span::raw(" "),
        Span::styled(app.runtime_cfg.base_url(), theme::dim_style()),
        Span::raw(" │ "),
        Span::styled(
            format!(".env {}", app.env_file_path().display()),
            theme::dim_style(),
        ),
    ]);
    frame.render_widget(Paragraph::new(title_line), area);
}

fn render_main_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.setup_required() {
        render_state_panel(
            frame,
            area,
            "Setup",
            "Chat needs a complete .env file",
            "Fill the provider settings, save, then press Ctrl+R.",
            setup_lines(app),
        );
        return;
    }

    if !app.daemon_running {
        render_state_panel(
            frame,
            area,
            "Connection",
            "The local daemon is not ready",
            "Check the log file or reload the .env settings.",
            daemon_unavailable_lines(app),
        );
        return;
    }

    if app.chat.messages.is_empty() {
        render_state_panel(
            frame,
            area,
            "Conversation",
            "Local AI chat workspace",
            "Type a message below to start a new conversation.",
            empty_chat_lines(app),
        );
        return;
    }

    render_messages(frame, app, area);
}

fn render_state_panel(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    headline: &str,
    subtitle: &str,
    lines: Vec<Line<'static>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(format!(" {title} "), theme::heading_style()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(4)])
        .split(inner);

    frame.render_widget(Banner { headline, subtitle }, chunks[0]);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let (title, lines) = if app.setup_required() {
        ("Setup", sidebar_setup_lines(app))
    } else if !app.daemon_running {
        ("Connection", sidebar_connection_lines(app))
    } else {
        ("Session", sidebar_session_lines(app))
    };

    frame.render_widget(InfoPanel { title, lines }, area);
}

fn render_messages(frame: &mut Frame, app: &mut App, area: Rect) {
    let conv_label = app
        .chat
        .conversation_id
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "new".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(
            format!(" Conversation {conv_label} "),
            theme::heading_style(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    if width == 0 || inner.height == 0 {
        return;
    }

    let mut rendered_lines: Vec<Line> = Vec::new();

    for msg in &app.chat.messages {
        let (prefix, style) = match msg.role.as_str() {
            "user" => ("▶ You: ", theme::accent_style()),
            "assistant" => ("◀ Bot: ", theme::success_style()),
            "system" => ("● Sys: ", theme::warn_style()),
            _ => ("  ", theme::dim_style()),
        };

        let prefix_width = display_width(prefix);
        let continuation_prefix = " ".repeat(prefix_width);
        let content_width = width.saturating_sub(prefix_width).max(1);
        let mut first = true;

        for text_line in msg.text.lines() {
            if text_line.is_empty() {
                let marker = if first {
                    prefix.to_string()
                } else {
                    continuation_prefix.clone()
                };
                rendered_lines.push(Line::from(vec![Span::styled(marker, style)]));
                first = false;
                continue;
            }

            for chunk in wrap_visual(text_line, content_width) {
                if first {
                    rendered_lines.push(Line::from(vec![
                        Span::styled(prefix.to_string(), style),
                        Span::styled(chunk, theme::body_style()),
                    ]));
                    first = false;
                } else {
                    rendered_lines.push(Line::from(vec![
                        Span::styled(continuation_prefix.clone(), theme::dim_style()),
                        Span::styled(chunk, theme::body_style()),
                    ]));
                }
            }
        }

        rendered_lines.push(Line::from(""));
    }

    let visible_height = inner.height as usize;
    let total = rendered_lines.len();

    if total <= visible_height {
        app.scroll_offset = 0;
    } else if app.scroll_offset >= total.saturating_sub(visible_height) {
        app.scroll_offset = total.saturating_sub(visible_height);
    }

    let visible: Vec<Line> = rendered_lines
        .into_iter()
        .skip(app.scroll_offset)
        .take(visible_height)
        .collect();

    frame.render_widget(Paragraph::new(visible), inner);

    if total > visible_height {
        let mut scrollbar_state =
            ScrollbarState::new(total.saturating_sub(visible_height)).position(app.scroll_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar_state,
        );
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let (title, border_style, placeholder) = if app.setup_required() {
        (
            " Setup required ",
            theme::warn_style(),
            "Edit .env, save the file, then press Ctrl+R to enable chat.",
        )
    } else if !app.daemon_running {
        (
            " Daemon unavailable ",
            theme::danger_style(),
            "Waiting for a healthy local daemon. Press Ctrl+R to retry.",
        )
    } else if app.chat.sending {
        (" Sending... ", theme::dim_style(), "")
    } else if app.chat.is_active() {
        (
            " Follow-up ",
            theme::accent_style(),
            "Type a follow-up message...",
        )
    } else {
        (" New prompt ", theme::accent_style(), "Type a message...")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(title, theme::heading_style()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if app.chat_enabled() && !app.input_buf.is_empty() {
        Text::from(app.input_buf.clone())
    } else {
        Text::from(Line::from(Span::styled(placeholder, theme::dim_style())))
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        inner,
    );

    if !app.chat_enabled() || app.chat.sending || inner.width == 0 || inner.height == 0 {
        return;
    }

    let (cursor_col, cursor_row) =
        wrapped_cursor_position(&app.input_buf, app.input_cursor, inner.width);
    if cursor_row < inner.height {
        frame.set_cursor_position((
            inner.x + cursor_col.min(inner.width - 1),
            inner.y + cursor_row,
        ));
    }
}

fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let line = if !app.notice.is_empty() {
        Line::from(Span::styled(
            format!(" {}", app.notice),
            theme::warn_style(),
        ))
    } else if app.chat_enabled() {
        Line::from(Span::styled(
            " Ctrl+N new conversation │ Ctrl+R reload .env │ /steer /stop /cancel │ Ctrl+Q quit",
            theme::dim_style(),
        ))
    } else {
        Line::from(Span::styled(
            " Edit .env, save, then press Ctrl+R to retry │ Ctrl+Q quit",
            theme::dim_style(),
        ))
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn setup_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("This TUI only handles chat now; configuration lives in the .env file."),
        Line::from(""),
        Line::from(Span::styled("Required keys", theme::heading_style())),
    ];

    for key in &app.setup.missing_fields {
        lines.push(Line::from(format!("  {key}=...")));
    }

    lines.extend([
        Line::from(""),
        Line::from(format!(
            "Environment file: {}",
            app.env_file_path().display()
        )),
        Line::from("Optional keys: GATEWAY_ADDR, SANDBOX, HTTPS_PROXY"),
        Line::from("After saving the file, press Ctrl+R to reload and auto-start the daemon."),
    ]);

    lines
}

fn daemon_unavailable_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        Line::from("The .env file looks complete, but the local daemon is not responding."),
        Line::from(""),
        Line::from(format!("Gateway: {}", app.runtime_cfg.base_url())),
        Line::from(format!("Log file: {}", app.log_file_path().display())),
        Line::from(""),
        Line::from("Press Ctrl+R after fixing the .env file or once the daemon becomes healthy."),
    ]
}

fn empty_chat_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("Daemon: {}", app.runtime_cfg.base_url())),
        Line::from(""),
        Line::from(Span::styled("Useful controls", theme::heading_style())),
        Line::from("  Enter send message"),
        Line::from("  Ctrl+N start a fresh conversation"),
        Line::from("  Up/Down/PageUp/PageDown scroll transcript"),
        Line::from(""),
        Line::from(Span::styled("Agent commands", theme::heading_style())),
        Line::from("  /steer    guide the current run"),
        Line::from("  /stop     request a graceful stop"),
        Line::from("  /cancel   cancel the current conversation"),
    ]
}

fn sidebar_setup_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled("Environment file", theme::heading_style())),
        Line::from(app.env_file_path().display().to_string()),
        Line::from(""),
        Line::from(Span::styled("Missing keys", theme::heading_style())),
    ];

    for key in &app.setup.missing_fields {
        lines.push(Line::from(format!("- {key}")));
    }

    if app.setup.template_created {
        lines.extend([
            Line::from(""),
            Line::from("A default template was created for you."),
        ]);
    }

    lines.extend([
        Line::from(""),
        Line::from(Span::styled("Workflow", theme::heading_style())),
        Line::from("1. Edit .env"),
        Line::from("2. Save the file"),
        Line::from("3. Press Ctrl+R"),
        Line::from("4. Start chatting"),
    ]);
    lines
}

fn sidebar_connection_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled("Daemon", theme::heading_style())),
        Line::from("not reachable"),
        Line::from(""),
        Line::from(Span::styled("Address", theme::heading_style())),
        Line::from(app.runtime_cfg.base_url()),
        Line::from(""),
        Line::from(Span::styled("Logs", theme::heading_style())),
        Line::from(app.log_file_path().display().to_string()),
        Line::from(""),
        Line::from("Ctrl+R reload .env and reconnect"),
        Line::from("Ctrl+Q quit"),
    ]
}

fn sidebar_session_lines(app: &App) -> Vec<Line<'static>> {
    let conversation = app
        .chat
        .conversation_id
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "new".to_string());

    vec![
        Line::from(Span::styled("Daemon", theme::heading_style())),
        Line::from("running"),
        Line::from(""),
        Line::from(Span::styled("Conversation", theme::heading_style())),
        Line::from(conversation),
        Line::from(format!("status: {}", app.chat.status_label())),
        Line::from(""),
        Line::from(Span::styled("Controls", theme::heading_style())),
        Line::from("Enter send"),
        Line::from("Ctrl+N new chat"),
        Line::from("Ctrl+R reload .env"),
        Line::from("Ctrl+Q quit"),
    ]
}

fn char_display_width(ch: char) -> usize {
    ch.width().unwrap_or(0).max(1)
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn wrap_visual(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        if current_width + ch_width > width && !current.is_empty() {
            wrapped.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || wrapped.is_empty() {
        wrapped.push(current);
    }

    wrapped
}

fn wrapped_cursor_position(text: &str, cursor_chars: usize, width: u16) -> (u16, u16) {
    if width == 0 {
        return (0, 0);
    }

    let line_width = width as usize;
    let mut col = 0usize;
    let mut row = 0usize;

    for ch in text.chars().take(cursor_chars) {
        if ch == '\n' {
            row += 1;
            col = 0;
            continue;
        }

        let ch_width = char_display_width(ch);
        if col + ch_width > line_width && col != 0 {
            row += 1;
            col = 0;
        }
        col += ch_width;
        if col >= line_width {
            row += col / line_width;
            col %= line_width;
        }
    }

    (col as u16, row as u16)
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
