use anda_core::BoxError;
use crossterm::{
    ExecutableCommand,
    cursor::MoveToNextLine,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    terminal::{disable_raw_mode, enable_raw_mode, size},
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
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
const MAX_INPUT_LINES: u16 = 4;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const INPUT_PROMPT_PREFIX: &str = "❯ ";
const INPUT_CONTINUATION_PREFIX: &str = "  ";
const INPUT_DIVIDER_LABEL: &str = "compose";
const THINKING_FRAMES: [&str; 4] = ["thinking", "thinking.", "thinking..", "thinking..."];

pub async fn run(daemon: Daemon, client: gateway::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client);
    app.bootstrap().await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnableBracketedPaste)?;

    // Size the inline viewport to the initial content, not the full terminal,
    // so the TUI expands from the current cursor row instead of reserving the
    // whole screen (which would push prior history up and anchor us at the
    // bottom).
    let (term_w, term_h) = size()?;
    let initial_height = desired_viewport_height(&app, term_w, term_h.max(1));
    let run_result = run_app(create_terminal_with_height(initial_height)?, &mut app).await;

    let paste_mode_result = stdout.execute(DisableBracketedPaste);
    let raw_mode_result = disable_raw_mode();

    paste_mode_result?;
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
    animation_tick: u64,
    flushed_message_lines: usize,
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
            animation_tick: 0,
            flushed_message_lines: 0,
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
        self.reset_transcript_view();
    }

    fn reset_transcript_view(&mut self) {
        self.flushed_message_lines = 0;
    }

    fn insert_input_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let chars: Vec<char> = self.input_buf.chars().collect();
        let mut new = String::with_capacity(self.input_buf.len() + text.len());
        new.extend(&chars[..self.input_cursor]);
        new.push_str(text);
        new.extend(&chars[self.input_cursor..]);
        self.input_buf = new;
        self.input_cursor += text.chars().count();
    }

    fn handle_paste(&mut self, text: String) {
        if !self.chat_enabled() {
            return;
        }

        self.insert_input_text(&normalize_newlines(&text));
    }

    async fn submit_input(&mut self) -> Result<(), BoxError> {
        if self.chat.sending {
            return Ok(());
        }

        let text = self.input_buf.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }

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
        if let Some(err) = self.chat.send(text).await {
            self.notice = err;
        } else {
            self.notice.clear();
        }

        Ok(())
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
        self.input_buf.clear();
        self.input_cursor = 0;
        self.reset_transcript_view();
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
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_input_text("\n");
            }
            KeyCode::Enter => {
                self.submit_input().await?;
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
}

async fn run_app(
    mut terminal: Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let mut last_status_refresh = Instant::now();
    let (mut term_w, mut term_h) = size()?;
    term_h = term_h.max(1);

    loop {
        app.animation_tick = app.animation_tick.wrapping_add(1);

        // Only recreate the terminal when the outer terminal is actually
        // resized. Recreating on pure content changes causes the inline
        // viewport to drift downward because each new Inline(n) emits `n`
        // newlines from the current cursor row.
        let (w, h) = size()?;
        let h = h.max(1);
        if w != term_w || h != term_h {
            let desired = desired_viewport_height(app, w, h);
            drop(terminal);
            terminal = create_terminal_with_height(desired)?;
            term_w = w;
            term_h = h;
        }

        terminal.autoresize()?;
        flush_transcript_scrollback(&mut terminal, app)?;
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
            let _ = app.chat.poll(None).await;
        }

        if !event::poll(Duration::from_millis(150))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Err(err) = app.handle_key(key).await {
                    app.notice = err.to_string();
                }
            }
            Event::Paste(text) => app.handle_paste(text),
            _ => {}
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

    let rendered_lines = visible_interaction_lines(app, width, area.height as usize);
    if rendered_lines.is_empty() {
        return;
    }

    frame.render_widget(
        Paragraph::new(rendered_lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let placeholder = input_placeholder(app);
    let (separator_area, prompt_area) = split_input_area(area);

    if let Some(separator_area) = separator_area {
        frame.render_widget(
            Paragraph::new(vec![input_separator_line(separator_area.width as usize)])
                .wrap(Wrap { trim: false }),
            separator_area,
        );
    }

    if prompt_area.height == 0 {
        return;
    }

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

    let prompt_prefix_width = input_prompt_prefix_width();
    let content_width = prompt_area.width.saturating_sub(prompt_prefix_width);
    let (cursor_col, cursor_row) =
        wrapped_cursor_position(&app.input_buf, app.input_cursor, content_width);
    if cursor_row < prompt_area.height {
        frame.set_cursor_position((
            prompt_area.x + prompt_prefix_width + cursor_col.min(content_width.saturating_sub(1)),
            prompt_area.y + cursor_row,
        ));
    }
}

fn layout_heights(app: &App, area: Rect) -> (u16, u16, u16) {
    if area.width == 0 || area.height == 0 {
        return (0, 0, 0);
    }

    let input = input_height(app, area).min(area.height);
    let remaining = area.height.saturating_sub(input);
    let status = status_panel_height(
        app,
        Rect {
            height: remaining,
            ..centered_area(area, STATUS_MAX_WIDTH)
        },
    );
    let interaction = remaining.saturating_sub(status);

    (status, interaction, input)
}

/// Compute the ideal inline-viewport height for the current app state so the
/// TUI sits naturally in the terminal rather than pinning the input to the
/// bottom. Any overflow still flows into scrollback via `insert_before`.
fn desired_viewport_height(app: &App, term_w: u16, term_h: u16) -> u16 {
    let term_h = term_h.max(1);
    if term_w == 0 {
        return term_h;
    }

    let full = Rect {
        x: 0,
        y: 0,
        width: term_w,
        height: term_h,
    };
    let input = input_height(app, full).min(term_h);
    let after_input = term_h.saturating_sub(input);
    let status = status_panel_height(
        app,
        Rect {
            height: after_input,
            ..centered_area(full, STATUS_MAX_WIDTH)
        },
    );
    let interaction_cap = after_input.saturating_sub(status);
    let interaction_needed = interaction_content_lines(app, term_w as usize) as u16;
    let interaction = interaction_needed.min(interaction_cap);

    (status + interaction + input).clamp(1, term_h)
}

/// How many lines of interaction content we would like to display right now
/// (notices + messages or empty state + loading indicator).
fn interaction_content_lines(app: &App, width: usize) -> usize {
    let notice = notice_lines(app).len();
    let thinking = thinking_lines(app).len();
    let body = if app.chat.messages.is_empty() {
        empty_state_lines(app).len()
    } else {
        // Subtract any lines already flushed into scrollback so the viewport
        // only reserves space for what is still visible.
        let total = chat_message_lines(app, width).len();
        total.saturating_sub(app.flushed_message_lines)
    };
    notice + body + thinking
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

    let inner_width = area.width.saturating_sub(input_prompt_prefix_width()) as usize;
    wrapped_line_count(&input_display_text(app), inner_width)
        .clamp(1, MAX_INPUT_LINES)
        .saturating_add(1)
        .min(area.height)
}

fn create_terminal_with_height(
    viewport_height: u16,
) -> Result<Terminal<CrosstermBackend<io::Stdout>>, BoxError> {
    Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(viewport_height.max(1)),
        },
    )
    .map_err(Into::into)
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
        .conversation
        .as_ref()
        .map(|c| format!("#{}", c._id))
        .unwrap_or_else(|| "new".to_string());

    vec![
        Line::from(format!(
            "conversation {conversation}   •   state {}",
            app.chat.status_label()
        )),
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
        Span::styled("ANDA Bot", theme::title_style()),
        Span::styled("  Born of panda. Awakened as Anda  ", theme::subtle_style()),
        status,
    ])
}

fn visible_interaction_lines(app: &App, width: usize, height: usize) -> Vec<Line<'static>> {
    let mut rendered_lines = notice_lines(app);
    let thinking = thinking_lines(app);
    let message_capacity = height.saturating_sub(rendered_lines.len() + thinking.len());
    let message_lines = chat_message_lines(app, width);

    if message_lines.is_empty() {
        let empty_state = empty_state_lines(app);
        let start = empty_state.len().saturating_sub(message_capacity);
        rendered_lines.extend(empty_state.into_iter().skip(start));
    } else {
        // Skip lines we have already pushed into scrollback, then keep only
        // the tail that fits — older overflow is preserved in scrollback.
        let flushed = app.flushed_message_lines.min(message_lines.len());
        let remaining = &message_lines[flushed..];
        let start = remaining.len().saturating_sub(message_capacity);
        rendered_lines.extend(remaining.iter().skip(start).cloned());
    }

    rendered_lines.extend(thinking);
    rendered_lines
}

fn notice_lines(app: &App) -> Vec<Line<'static>> {
    let mut rendered_lines: Vec<Line> = Vec::new();

    if !app.notice.is_empty() {
        rendered_lines.push(Line::from(Span::styled(
            format!("! {}", app.notice),
            theme::warn_style(),
        )));
        rendered_lines.push(Line::from(""));
    }

    rendered_lines
}

fn empty_state_lines(app: &App) -> Vec<Line<'static>> {
    let mut rendered_lines: Vec<Line> = Vec::new();

    if app.chat.messages.is_empty() && app.chat_enabled() {
        rendered_lines.push(Line::from(vec![
            Span::styled("🐼 ❯ ", theme::success_style()),
            Span::styled("Ready when you are.", theme::subtle_style()),
        ]));
        rendered_lines.push(Line::from(Span::styled(
            "     Send a message below to start the inline chat.",
            theme::subtle_style(),
        )));
    }

    rendered_lines
}

fn chat_message_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut rendered_lines: Vec<Line> = Vec::new();

    if app.chat.messages.is_empty() {
        return rendered_lines;
    }

    for msg in &app.chat.messages {
        let (prefix, style) = match msg.role.as_str() {
            "user" => ("❯ ", theme::accent_style()),
            "assistant" => ("🐼 ❯ ", theme::success_style()),
            "system" => ("⚠️ ❯ ", theme::warn_style()),
            _ => ("  ", theme::dim_style()),
        };

        let prefix_width = display_width(prefix);
        let continuation_prefix = " ".repeat(prefix_width);
        let content_width = width.saturating_sub(prefix_width).max(1);
        let mut first = true;

        for text_line in msg.text.clone().unwrap_or_default().lines() {
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

        if let Some(thoughts) = collapsed_single_line(msg.thoughts.as_deref().unwrap_or_default()) {
            let marker = if first {
                prefix.to_string()
            } else {
                continuation_prefix.clone()
            };
            rendered_lines.push(Line::from(vec![
                Span::styled(marker, theme::dim_style()),
                Span::styled(
                    truncate_visual(&thoughts, content_width),
                    theme::dim_style(),
                ),
            ]));
            first = false;
        }

        push_wrapped_block(
            &mut rendered_lines,
            msg.error.as_deref().unwrap_or_default(),
            &mut first,
            prefix,
            &continuation_prefix,
            theme::danger_style(),
            theme::dim_style(),
            theme::danger_style(),
            content_width,
        );

        rendered_lines.push(Line::from(""));
    }

    rendered_lines
}

fn thinking_lines(app: &App) -> Vec<Line<'static>> {
    if !app.chat.is_thinking() {
        return Vec::new();
    }

    let frame = THINKING_FRAMES[(app.animation_tick as usize / 2) % THINKING_FRAMES.len()];
    vec![Line::from(vec![
        Span::styled("🐼 ❯ ", theme::success_style()),
        Span::styled(frame.to_string(), theme::subtle_style()),
    ])]
}

fn flush_transcript_scrollback(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let size = terminal.size()?;
    if size.width == 0 || size.height == 0 || app.chat.messages.is_empty() {
        return Ok(());
    }

    let area = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };

    let (_, interaction_height, _) = layout_heights(app, area);
    let reserved_lines = notice_lines(app).len() + thinking_lines(app).len();
    let message_capacity = interaction_height as usize;
    let message_capacity = message_capacity.saturating_sub(reserved_lines);
    let message_lines = chat_message_lines(app, area.width as usize);
    let overflow = message_lines.len().saturating_sub(message_capacity);

    if overflow <= app.flushed_message_lines {
        return Ok(());
    }

    let new_lines = message_lines[app.flushed_message_lines..overflow].to_vec();
    if new_lines.is_empty() {
        app.flushed_message_lines = overflow;
        return Ok(());
    }

    terminal.insert_before(new_lines.len() as u16, |buf| {
        Paragraph::new(new_lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false })
            .render(buf.area, buf);
    })?;
    app.flushed_message_lines = overflow;

    Ok(())
}

fn input_display_text(app: &App) -> String {
    if app.chat_enabled() && !app.input_buf.is_empty() {
        return app.input_buf.clone();
    }
    input_placeholder(app).to_string()
}

fn input_placeholder(app: &App) -> &'static str {
    if app.setup_required() {
        "Edit config.yaml, save, then press Enter to enable chat."
    } else if !app.daemon_running {
        "Waiting for a healthy local daemon. Press Enter to retry."
    } else if app.chat.sending {
        ""
    } else if app.chat.is_active() {
        "Type a follow-up message..."
    } else {
        "Type a message..."
    }
}

fn build_prompt_lines(app: &App, placeholder: &str, width: usize) -> Vec<Line<'static>> {
    let content_width = width
        .saturating_sub(input_prompt_prefix_width() as usize)
        .max(1);

    if !app.chat_enabled() || app.input_buf.is_empty() {
        return vec![Line::from(vec![
            Span::styled(INPUT_PROMPT_PREFIX.to_string(), theme::accent_style()),
            Span::styled(placeholder.to_string(), theme::dim_style()),
        ])];
    }

    let mut lines = Vec::new();
    let mut first = true;

    for text_line in app.input_buf.split('\n') {
        if text_line.is_empty() {
            let prefix = if first {
                INPUT_PROMPT_PREFIX
            } else {
                INPUT_CONTINUATION_PREFIX
            };
            lines.push(Line::from(vec![Span::styled(
                prefix.to_string(),
                theme::accent_style(),
            )]));
            first = false;
            continue;
        }

        for chunk in wrap_visual(text_line, content_width) {
            let prefix = if first {
                INPUT_PROMPT_PREFIX
            } else {
                INPUT_CONTINUATION_PREFIX
            };
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), theme::accent_style()),
                Span::styled(chunk, theme::body_style()),
            ]));
            first = false;
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(INPUT_PROMPT_PREFIX.to_string(), theme::accent_style()),
            Span::styled(placeholder.to_string(), theme::dim_style()),
        ]));
    }

    lines
}

fn input_prompt_prefix_width() -> u16 {
    display_width(INPUT_PROMPT_PREFIX) as u16
}

fn split_input_area(area: Rect) -> (Option<Rect>, Rect) {
    if area.height <= 1 {
        return (None, area);
    }

    (
        Some(Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        }),
        Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height - 1,
        },
    )
}

fn input_separator_line(width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }

    let prefix = "── ";
    let title = INPUT_DIVIDER_LABEL;
    let compact = format!("{prefix}{title}");
    let compact_width = display_width(&compact);

    if width <= compact_width {
        return Line::from(Span::styled(
            truncate_visual(&compact, width),
            theme::dim_style(),
        ));
    }

    let filler_width = width.saturating_sub(compact_width + 1);
    Line::from(vec![
        Span::styled(prefix.to_string(), theme::dim_style()),
        Span::styled(title.to_string(), theme::accent_style()),
        Span::styled(format!(" {}", "─".repeat(filler_width)), theme::dim_style()),
    ])
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
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

fn collapsed_single_line(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn truncate_visual(text: &str, width: usize) -> String {
    let width = width.max(1);
    if display_width(text) <= width {
        return text.to_string();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let target_width = width - 3;
    let mut truncated = String::new();
    let mut current_width = 0;

    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        if current_width + ch_width > target_width {
            break;
        }
        truncated.push(ch);
        current_width += ch_width;
    }

    truncated.push_str("...");
    truncated
}

#[allow(clippy::too_many_arguments)]
fn push_wrapped_block(
    rendered_lines: &mut Vec<Line<'static>>,
    text: &str,
    first: &mut bool,
    prefix: &str,
    continuation_prefix: &str,
    first_prefix_style: Style,
    continuation_prefix_style: Style,
    body_style: Style,
    content_width: usize,
) {
    for text_line in text.lines() {
        if text_line.is_empty() {
            let marker = if *first {
                prefix.to_string()
            } else {
                continuation_prefix.to_string()
            };
            let marker_style = if *first {
                first_prefix_style
            } else {
                continuation_prefix_style
            };
            rendered_lines.push(Line::from(vec![Span::styled(marker, marker_style)]));
            *first = false;
            continue;
        }

        for chunk in wrap_visual(text_line, content_width) {
            let marker = if *first {
                prefix.to_string()
            } else {
                continuation_prefix.to_string()
            };
            let marker_style = if *first {
                first_prefix_style
            } else {
                continuation_prefix_style
            };
            rendered_lines.push(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(chunk, body_style),
            ]));
            *first = false;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::memory::{Conversation, ConversationStatus};

    fn test_client() -> gateway::Client {
        gateway::Client::new("http://127.0.0.1:8042".to_string(), String::new())
    }

    fn ready_app() -> App {
        let mut app = App::new(PathBuf::from("."), Config::default(), test_client());
        app.daemon_running = true;
        app
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn normalize_newlines_converts_crlf_and_cr() {
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn insert_input_text_respects_cursor_position() {
        let mut app = ready_app();
        app.input_buf = "abef".to_string();
        app.input_cursor = 2;

        app.insert_input_text("cd");

        assert_eq!(app.input_buf, "abcdef");
        assert_eq!(app.input_cursor, 4);
    }

    #[test]
    fn handle_paste_inserts_normalized_multiline_text() {
        let mut app = ready_app();

        app.handle_paste("first\r\nsecond\rthird".to_string());

        assert_eq!(app.input_buf, "first\nsecond\nthird");
        assert_eq!(app.input_cursor, "first\nsecond\nthird".chars().count());
    }

    #[test]
    fn build_prompt_lines_uses_continuation_prefix_for_multiline_input() {
        let mut app = ready_app();
        app.input_buf = "alpha\nbeta".to_string();
        app.input_cursor = app.input_buf.chars().count();

        let lines = build_prompt_lines(&app, "", 24);

        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "❯ alpha");
        assert_eq!(line_text(&lines[1]), "  beta");
    }

    #[test]
    fn input_height_reserves_space_for_separator() {
        let app = ready_app();

        assert_eq!(
            input_height(
                &app,
                Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 8,
                }
            ),
            2
        );
    }

    #[test]
    fn split_input_area_preserves_prompt_when_only_one_line_fits() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };

        let (separator, prompt) = split_input_area(area);

        assert!(separator.is_none());
        assert_eq!(prompt, area);
    }

    #[test]
    fn input_separator_line_labels_compose_section() {
        let line = input_separator_line(24);

        assert!(line_text(&line).starts_with("── compose "));
    }

    #[test]
    fn chat_message_lines_render_thoughts_as_single_dim_excerpt() {
        let mut app = ready_app();
        app.chat.messages.push(gateway::ChatMessage {
            role: "assistant".to_string(),
            text: Some("Answer ready".to_string()),
            thoughts: Some(
                "first line of reasoning\nsecond line that is long enough to truncate".to_string(),
            ),
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 28);

        assert_eq!(line_text(&lines[0]), "🐼 ❯ Answer ready");
        assert_eq!(lines[1].spans[0].style, theme::dim_style());
        assert_eq!(lines[1].spans[1].style, theme::dim_style());
        assert!(line_text(&lines[1]).contains("first line"));
        assert!(line_text(&lines[1]).ends_with("..."));
        assert_eq!(line_text(&lines[2]), "");
    }

    #[test]
    fn chat_message_lines_render_errors_with_system_prefix() {
        let mut app = ready_app();
        app.chat.messages.push(gateway::ChatMessage {
            role: "system".to_string(),
            error: Some("request failed badly".to_string()),
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 40);

        assert_eq!(line_text(&lines[0]), "⚠️ ❯ request failed badly");
        assert_eq!(lines[0].spans[0].style, theme::danger_style());
        assert_eq!(lines[0].spans[1].style, theme::danger_style());
        assert_eq!(line_text(&lines[1]), "");
    }

    #[test]
    fn thinking_lines_only_render_for_submitted_and_working() {
        let mut app = ready_app();

        assert!(thinking_lines(&app).is_empty());

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Idle,
            ..Default::default()
        });
        assert!(thinking_lines(&app).is_empty());

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Submitted,
            ..Default::default()
        });
        assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking");

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Working,
            ..Default::default()
        });
        assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking");

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Completed,
            ..Default::default()
        });
        assert!(thinking_lines(&app).is_empty());
    }
}
