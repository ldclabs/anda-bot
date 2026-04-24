use anda_core::BoxError;
use crossterm::{
    ExecutableCommand,
    cursor::MoveToNextLine,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    config::Config,
    daemon::{Daemon, LaunchState, process_exists},
    gateway,
};

mod theme;
mod widgets;

use self::widgets::Banner;

const STATUS_MAX_WIDTH: u16 = 92;
const MIN_INTERACTION_HEIGHT: u16 = 4;
const MAX_INPUT_LINES: u16 = 4;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

pub async fn run(daemon: Daemon, client: gateway::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client);
    app.bootstrap().await;

    enable_raw_mode()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(inline_viewport_height()),
        },
    )?;
    let run_result = run_app(&mut terminal, &mut app).await;

    let raw_mode_result = disable_raw_mode();
    let mut stdout = io::stdout();

    raw_mode_result?;
    let cursor_result = stdout.execute(MoveToNextLine(1));
    cursor_result?;
    run_result
}

#[derive(Default)]
struct SetupState {
    template_created: bool,
    issues: Vec<String>,
}

impl SetupState {
    fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }
}

struct App {
    home: PathBuf,
    client: gateway::Client,
    should_quit: bool,
    notice: String,
    pid: Option<u32>,
    daemon_running: bool,
    runtime_cfg: Config,
    setup: SetupState,
    chat: gateway::ChatSession,
    input_buf: String,
    input_cursor: usize,
    scroll_offset: usize,
}

impl App {
    fn new(home: PathBuf, cfg: Config, client: gateway::Client) -> Self {
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

    fn config_file_path(&self) -> PathBuf {
        self.home.join("config.yaml")
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
                KeyCode::Char('c') => {
                    self.should_quit = true;
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
                self.bootstrap().await;
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
                        if text == "/new" {
                            self.new_conversation();
                            return Ok(());
                        }

                        if text == "/reload" {
                            self.bootstrap().await;
                            return Ok(());
                        }

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
        terminal.autoresize()?;
        terminal.draw(|frame| render(frame, app))?;

        if app.should_quit {
            break;
        }

        if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
            let was_running = app.daemon_running;
            let _ = app.refresh_status().await;
            if app.setup.is_ready() && was_running && !app.daemon_running && app.notice.is_empty() {
                app.notice =
                    "Daemon connection lost. Press Enter to reload config.yaml and reconnect."
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
    let (status_height, interaction_height, input_height) = layout_heights(app, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(status_height),
            Constraint::Length(interaction_height),
            Constraint::Length(input_height),
        ])
        .split(area);

    render_status_panel(frame, app, chunks[0]);
    render_interaction_output(frame, app, chunks[1]);
    render_input(frame, app, chunks[2]);
}

fn render_status_panel(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let area = centered_area(area, STATUS_MAX_WIDTH);

    let header = panel_header_line(app);
    let lines = panel_lines(app);
    let (headline, subtitle) = panel_banner(app);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(Banner::height().min(area.height.saturating_sub(1))),
            Constraint::Min(0),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(header).alignment(ratatui::layout::Alignment::Center),
        sections[0],
    );
    frame.render_widget(
        Banner {
            headline: headline.as_str(),
            subtitle: subtitle.as_str(),
        },
        sections[1],
    );

    if sections[2].height == 0 || lines.is_empty() {
        return;
    }

    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel_glow_style())
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: false }),
        sections[2],
    );
}

fn render_interaction_output(frame: &mut Frame, app: &mut App, area: Rect) {
    let width = area.width as usize;
    if width == 0 || area.height == 0 {
        return;
    }

    let rendered_lines = interaction_lines(app, width);
    if rendered_lines.is_empty() {
        return;
    }

    let visible_height = area.height as usize;
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

    frame.render_widget(
        Paragraph::new(visible)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        area,
    );

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
    if area.width == 0 || area.height == 0 {
        return;
    }

    let placeholder = if app.setup_required() {
        "Edit config.yaml, save, then press Enter to enable chat."
    } else if !app.daemon_running {
        "Waiting for a healthy local daemon. Press Enter to retry."
    } else if app.chat.sending {
        ""
    } else if app.chat.is_active() {
        "Type a follow-up message..."
    } else {
        "Type a message..."
    };

    let prompt_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let prompt_lines = build_prompt_lines(app, placeholder, prompt_area.width as usize);
    frame.render_widget(
        Paragraph::new(prompt_lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        prompt_area,
    );

    if !app.chat_enabled() || app.chat.sending || prompt_area.width <= 2 || prompt_area.height == 0
    {
        return;
    }

    let content_width = prompt_area.width.saturating_sub(5);
    let (cursor_col, cursor_row) =
        wrapped_cursor_position(&app.input_buf, app.input_cursor, content_width);
    if cursor_row < prompt_area.height {
        frame.set_cursor_position((
            prompt_area.x + 5 + cursor_col.min(content_width.saturating_sub(1)),
            prompt_area.y + cursor_row,
        ));
    }
}

fn inline_viewport_height() -> u16 {
    Banner::height() + MIN_INTERACTION_HEIGHT + MAX_INPUT_LINES + 3
}

fn layout_heights(app: &App, area: Rect) -> (u16, u16, u16) {
    if area.width == 0 || area.height == 0 {
        return (0, 0, 0);
    }

    let input = input_height(app, area).min(area.height);
    let remaining = area.height.saturating_sub(input);
    let interaction_floor = MIN_INTERACTION_HEIGHT.min(remaining);
    let status_limit = remaining.saturating_sub(interaction_floor);
    let status = status_panel_height(
        app,
        Rect {
            height: status_limit,
            ..centered_area(area, STATUS_MAX_WIDTH)
        },
    );
    let interaction = area.height.saturating_sub(status + input);

    (status, interaction, input)
}

fn status_panel_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let lines = panel_lines(app).len() as u16;
    let desired = 1 + Banner::height() + lines;
    desired.min(area.height)
}

fn input_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let inner_width = area.width.saturating_sub(5) as usize;
    wrapped_line_count(&input_display_text(app), inner_width)
        .clamp(1, MAX_INPUT_LINES)
        .min(area.height)
}

fn panel_banner(app: &App) -> (String, String) {
    if app.setup_required() {
        return (
            "Setup required".to_string(),
            app.config_file_path().display().to_string(),
        );
    }

    if !app.daemon_running {
        let headline = if app.pid.is_some() {
            "Daemon unhealthy"
        } else {
            "Daemon offline"
        };
        return (headline.to_string(), app.runtime_cfg.base_url());
    }

    ("Daemon ready".to_string(), app.runtime_cfg.base_url())
}

fn panel_lines(app: &App) -> Vec<Line<'static>> {
    if app.setup_required() {
        let mut lines = vec![
            Line::from(format!("Config: {}", app.config_file_path().display())),
            Line::from(Span::styled("Missing or invalid", theme::heading_style())),
        ];

        for issue in &app.setup.issues {
            lines.push(Line::from(format!("- {issue}")));
        }

        if app.setup.template_created {
            lines.push(Line::from("Template files were created automatically."));
        }

        lines.push(Line::from("Press Enter after saving to reload."));

        return lines;
    }

    if !app.daemon_running {
        return vec![
            Line::from(format!("Gateway: {}", app.runtime_cfg.base_url())),
            Line::from(format!("Logs: {}", app.log_file_path().display())),
            Line::from("Press Enter after the daemon becomes healthy."),
        ];
    }

    let conversation = app
        .chat
        .conversation_id
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "new".to_string());

    vec![
        Line::from(format!(
            "conversation {conversation}   •   state {}",
            app.chat.status_label()
        )),
        Line::from(app.runtime_cfg.base_url()),
        Line::from("/new clear transcript   •   /reload config   •   Ctrl+C quit"),
        Line::from("/steer guide   •   /stop stop   •   /cancel cancel"),
    ]
}

fn panel_header_line(app: &App) -> Line<'static> {
    let status = if app.setup_required() {
        Span::styled(" SETUP ", theme::warn_style())
    } else if !app.daemon_running {
        Span::styled(" OFFLINE ", theme::danger_style())
    } else {
        Span::styled(" READY ", theme::badge_style())
    };

    Line::from(vec![
        Span::styled("ANDA Chat", theme::title_style()),
        Span::styled("  panda-born inline copilot  ", theme::subtle_style()),
        status,
    ])
}

fn interaction_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut rendered_lines: Vec<Line> = Vec::new();

    if !app.notice.is_empty() {
        rendered_lines.push(Line::from(Span::styled(
            format!("! {}", app.notice),
            theme::warn_style(),
        )));
        rendered_lines.push(Line::from(""));
    }

    if app.chat.messages.is_empty() {
        if app.chat_enabled() {
            rendered_lines.push(Line::from(vec![
                Span::styled("🤖❯ ", theme::success_style()),
                Span::styled("Ready when you are.", theme::subtle_style()),
            ]));
            rendered_lines.push(Line::from(Span::styled(
                "      Send a message below to start the inline chat.",
                theme::subtle_style(),
            )));
        }
        return rendered_lines;
    }

    for msg in &app.chat.messages {
        let (prefix, style) = match msg.role.as_str() {
            "user" => ("❯ ", theme::accent_style()),
            "assistant" => ("🤖 ❯ ", theme::success_style()),
            "system" => ("⛑︎ ❯ ", theme::warn_style()),
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

    rendered_lines
}

fn input_display_text(app: &App) -> String {
    if app.chat_enabled() && !app.input_buf.is_empty() {
        return app.input_buf.clone();
    }

    if app.setup_required() {
        "Edit config.yaml, save, then press Enter to enable chat.".to_string()
    } else if !app.daemon_running {
        "Waiting for a healthy local daemon. Press Enter to retry.".to_string()
    } else if app.chat.sending {
        String::new()
    } else if app.chat.is_active() {
        "Type a follow-up message...".to_string()
    } else {
        "Type a message...".to_string()
    }
}

fn build_prompt_lines(app: &App, placeholder: &str, width: usize) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(5).max(1);

    if !app.chat_enabled() || app.input_buf.is_empty() {
        return vec![Line::from(vec![
            Span::styled("❯ ", theme::accent_style()),
            Span::styled(placeholder.to_string(), theme::dim_style()),
        ])];
    }

    let mut lines = Vec::new();
    let mut first = true;

    for text_line in app.input_buf.lines() {
        if text_line.is_empty() {
            let prefix = if first { "❯ " } else { "  " };
            lines.push(Line::from(vec![Span::styled(
                prefix.to_string(),
                theme::accent_style(),
            )]));
            first = false;
            continue;
        }

        for chunk in wrap_visual(text_line, content_width) {
            let prefix = if first { "❯ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), theme::accent_style()),
                Span::styled(chunk, theme::body_style()),
            ]));
            first = false;
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("❯ ", theme::accent_style()),
            Span::styled(placeholder.to_string(), theme::dim_style()),
        ]));
    }

    lines
}

fn wrapped_line_count(text: &str, width: usize) -> u16 {
    let width = width.max(1);
    let mut lines = 0u16;

    for line in text.split('\n') {
        lines = lines.saturating_add(wrap_visual(line, width).len() as u16);
    }

    lines.max(1)
}

fn centered_area(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width);
    let offset = area.width.saturating_sub(width) / 2;

    Rect {
        x: area.x + offset,
        y: area.y,
        width,
        height: area.height,
    }
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
