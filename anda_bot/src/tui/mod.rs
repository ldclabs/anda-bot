use anda_core::{BoxError, ContentPart, Message};
use crossterm::{
    ExecutableCommand,
    cursor::{MoveTo, MoveToNextLine},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    terminal::{
        Clear, ClearType, disable_raw_mode, enable_raw_mode, size, supports_keyboard_enhancement,
    },
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};
use std::{
    borrow::Cow,
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    config::{APP_VERSION, Config},
    daemon::{Daemon, LaunchState, process_exists},
    gateway,
};

mod theme;
mod widgets;

use self::widgets::{Banner, PackedLines};

const STATUS_MAX_WIDTH: u16 = 92;
const MAX_INPUT_LINES: u16 = 4;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const INPUT_PROMPT_PREFIX: &str = "❯ ";
const INPUT_CONTINUATION_PREFIX: &str = "  ";
const INPUT_DIVIDER_PREFIX: &str = "── ";
const INPUT_DIVIDER_LABEL: &str = "compose";
const THINKING_LABEL: &str = "thinking";
const INPUT_DIVIDER_PADDED_HEIGHT: u16 = 3;
const INPUT_DIVIDER_FLOW_SPEED: f32 = 1.25;
const INPUT_DIVIDER_GLOW_RADIUS: f32 = 15.0;
const INPUT_DIVIDER_TRAIL_OFFSET: f32 = 9.0;
const STATUS_FOOTER_MAX_LINES: usize = 3;
const SECONDARY_PART_MAX_LINES: usize = 3;
const THINKING_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub async fn run(daemon: Daemon, client: gateway::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client);
    app.bootstrap().await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnableBracketedPaste)?;
    // Push kitty keyboard enhancement flags so Shift+Enter, Ctrl+Enter, etc.
    // are reported as distinct key events. Some terminals (e.g. macOS
    // Terminal.app) don't support this; in that case fall back silently.
    let keyboard_enhancement_pushed = match supports_keyboard_enhancement() {
        Ok(true) => stdout
            .execute(PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            ))
            .is_ok(),
        _ => false,
    };

    // Size the inline viewport to the initial content, not the full terminal,
    // so the TUI expands from the current cursor row instead of reserving the
    // whole screen (which would push prior history up and anchor us at the
    // bottom).
    let (term_w, term_h) = size()?;
    let initial_height = dynamic_viewport_height(&app, term_w, term_h.max(1));
    let run_result = run_app(create_terminal_with_height(initial_height)?, &mut app).await;

    if keyboard_enhancement_pushed {
        let _ = stdout.execute(PopKeyboardEnhancementFlags);
    }
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
    static_panel_flushed: bool,
    flushed_message_count: usize,
    input_focused: bool,
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
            static_panel_flushed: false,
            flushed_message_count: 0,
            input_focused: true,
        }
    }

    fn runtime_daemon(&self) -> Daemon {
        Daemon::new(self.home.clone(), self.runtime_cfg.clone())
    }

    fn config_file_path(&self) -> PathBuf {
        self.home.join("config.yaml")
    }

    fn log_file_path(&self) -> PathBuf {
        crate::logger::current_daily_log_file_path(
            self.home.join("logs"),
            crate::logger::DAEMON_LOG_FILE_PREFIX,
        )
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
        self.reset_message_view();
    }

    fn reset_message_view(&mut self) {
        self.flushed_message_count = 0;
        self.input_focused = true;
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
        let cursor = self.input_cursor + text.chars().count();
        let (normalized, cursor) = compact_cjk_spacing_with_cursor(&new, cursor);
        self.input_buf = normalized;
        self.input_cursor = cursor;
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

    // fn new_conversation(&mut self) {
    //     self.chat.reset();
    //     self.input_buf.clear();
    //     self.input_cursor = 0;
    //     self.reset_message_view();
    //     self.notice = "New conversation.".to_string();
    // }

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
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_input_text("\n");
            }
            KeyCode::Enter => {
                self.submit_input().await?;
            }
            KeyCode::Backspace
                if self.input_cursor > 0 => {
                    let chars: Vec<char> = self.input_buf.chars().collect();
                    let pos = self.input_cursor - 1;
                    self.input_buf = chars[..pos].iter().chain(chars[pos + 1..].iter()).collect();
                    self.input_cursor -= 1;
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
            KeyCode::Left
                if self.input_cursor > 0 => {
                    self.input_cursor -= 1;
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
    let mut current_viewport_height = terminal.get_frame().area().height;

    loop {
        app.animation_tick = app.animation_tick.wrapping_add(1);

        // Recreate the terminal when:
        //  - the outer terminal was resized, or
        //  - the inline viewport needs to grow to fit new content (capped at
        //    the terminal height).
        // We never shrink the viewport, so the layout doesn't jitter as
        // messages arrive and disappear.
        let (w, h) = size()?;
        let h = h.max(1);
        let terminal_resized = w != term_w || h != term_h;
        if terminal_resized {
            term_w = w;
            term_h = h;
        }

        let desired = dynamic_viewport_height(app, term_w, term_h);
        let new_height = desired;
        let new_height = new_height.min(term_h).max(1);
        if new_height != current_viewport_height || terminal_resized {
            // Clear the previous viewport area before recreating so that the
            // re-anchored viewport does not leave a ghost copy of the old
            // frame above it. Anything that was already pushed into
            // scrollback (above the viewport via `insert_before`) is
            // preserved.
            let old_area = terminal.get_frame().area();
            let mut stdout = io::stdout();
            stdout.execute(MoveTo(old_area.x, old_area.y))?;
            stdout.execute(Clear(ClearType::FromCursorDown))?;
            drop(terminal);
            terminal = create_terminal_with_height(new_height)?;
            current_viewport_height = new_height;
        }

        terminal.autoresize()?;
        flush_static_scrollback(&mut terminal, app)?;
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
    let (input_height, status_height) = dynamic_layout_heights(app, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(input_height),
            Constraint::Length(status_height),
        ])
        .split(area);

    render_input(frame, app, chunks[0]);
    render_status_footer(frame, app, chunks[1]);
}

fn render_static_panel_to_buffer(app: &App, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let area = centered_area(area, STATUS_MAX_WIDTH);

    let header = panel_header_line(app);
    let lines = panel_lines(app);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(Banner::height().min(area.height.saturating_sub(1))),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    Banner {}.render(sections[0], buf);
    PackedLines::new(vec![header])
        .alignment(ratatui::layout::Alignment::Center)
        .render(sections[1], buf);

    if sections[2].height == 0 || lines.is_empty() {
        return;
    }

    PackedLines::new(lines)
        .style(theme::panel_glow_style())
        .alignment(ratatui::layout::Alignment::Center)
        .render(sections[2], buf);
}

fn render_status_footer(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let panel = status_footer_panel(area);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .style(theme::footer_panel_style())
            .border_style(theme::footer_border_style()),
        area,
    );

    if panel.width == 0 || panel.height == 0 {
        return;
    }

    let lines = status_footer_lines(app, panel.width as usize);
    if lines.is_empty() {
        return;
    }

    // Paint the panel background, then draw text via the CJK-safe widget.
    frame.render_widget(Block::default().style(theme::footer_panel_style()), panel);
    frame.render_widget(
        PackedLines::new(lines).style(theme::footer_text_style()),
        panel,
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
            PackedLines::new(input_separator_lines(app, separator_area)),
            separator_area,
        );
    }

    if prompt_area.height == 0 {
        return;
    }

    let prompt_lines = build_prompt_lines(app, placeholder, prompt_area.width as usize);
    frame.render_widget(
        PackedLines::new(prompt_lines).style(theme::body_style()),
        prompt_area,
    );

    if !app.chat_enabled()
        || !app.input_focused
        || app.chat.sending
        || prompt_area.width <= 2
        || prompt_area.height == 0
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

fn dynamic_layout_heights(app: &App, area: Rect) -> (u16, u16) {
    if area.width == 0 || area.height == 0 {
        return (0, 0);
    }

    let input = input_height(app, area).min(area.height);
    let status =
        status_footer_height(app, area.width as usize).min(area.height.saturating_sub(input));

    (input, status)
}

/// Compute the inline viewport height for the dynamic bottom area only. The
/// static panel and messages are written above the viewport once via
/// `insert_before`, so they can naturally become shell scrollback.
fn dynamic_viewport_height(app: &App, term_w: u16, term_h: u16) -> u16 {
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
    let status = status_footer_height(app, term_w as usize).min(term_h.saturating_sub(input));

    (input + status).clamp(1, term_h)
}

fn status_panel_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let lines = panel_lines(app).len() as u16;
    let desired = 1 + Banner::height() + lines;
    desired.min(area.height)
}

fn static_panel_height(app: &App, width: u16) -> u16 {
    status_panel_height(
        app,
        Rect {
            x: 0,
            y: 0,
            width,
            height: u16::MAX,
        },
    )
}

fn status_footer_height(app: &App, width: usize) -> u16 {
    status_footer_lines(app, width)
        .len()
        .clamp(1, STATUS_FOOTER_MAX_LINES)
        .saturating_add(1) as u16
}

fn status_footer_panel(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    }
}

fn status_footer_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = if app.input_focused && app.chat_enabled() && !app.chat.sending {
        vec![
            Line::from(vec![
                Span::styled("? ", theme::accent_style()),
                Span::styled(
                    truncate_visual(
                        "Shift+Enter newline  •  Ctrl+U clear  •  Ctrl+A/E move  •  Ctrl+C quit  •  Esc status",
                        width.saturating_sub(2),
                    ),
                    theme::subtle_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("? ", theme::accent_style()),
                Span::styled(
                    truncate_visual(
                        "/reload config  •  /steer message  •  /stop message",
                        width.saturating_sub(2),
                    ),
                    theme::subtle_style(),
                ),
            ]),
        ]
    } else {
        vec![status_line(app, width)]
    };

    if !app.notice.is_empty() && lines.len() < STATUS_FOOTER_MAX_LINES {
        let notice = compact_cjk_spacing(&app.notice);
        lines.push(Line::from(vec![
            Span::styled("! ", theme::warn_style()),
            Span::styled(
                truncate_visual(notice.as_ref(), width.saturating_sub(2)),
                theme::warn_style(),
            ),
        ]));
    }

    lines.truncate(STATUS_FOOTER_MAX_LINES);
    lines
}

fn status_line(app: &App, width: usize) -> Line<'static> {
    let (badge, style, text) = if app.setup_required() {
        (
            "SETUP",
            theme::warn_style(),
            format!("fill config: {}", app.setup.issues.join(", ")),
        )
    } else if !app.daemon_running {
        (
            "OFFLINE",
            theme::danger_style(),
            format!(
                "gateway {} unavailable; logs {}; press Enter to retry",
                app.runtime_cfg.base_url(),
                app.log_file_path().display()
            ),
        )
    } else {
        let conversation = app
            .chat
            .conversation
            .as_ref()
            .map(|c| format!("#{}", c._id))
            .unwrap_or_else(|| "new".to_string());
        (
            "READY",
            theme::success_style(),
            format!(
                "conversation {conversation} · state {}",
                app.chat.status_label()
            ),
        )
    };

    let prefix = format!("{badge} ");
    let text = compact_cjk_spacing(&text);
    Line::from(vec![
        Span::styled(prefix.clone(), style),
        Span::styled(
            truncate_visual(text.as_ref(), width.saturating_sub(display_width(&prefix))),
            theme::subtle_style(),
        ),
    ])
}

fn input_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let inner_width = area.width.saturating_sub(input_prompt_prefix_width()) as usize;
    wrapped_line_count(&input_display_text(app), inner_width)
        .clamp(1, MAX_INPUT_LINES)
        .saturating_add(input_separator_block_height(area.height))
        .min(area.height)
}

fn create_terminal_with_height(
    viewport_height: u16,
) -> Result<Terminal<CrosstermBackend<io::Stdout>>, BoxError> {
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(viewport_height.max(1)),
        },
    )?;
    terminal.clear()?;
    Ok(terminal)
}

fn panel_lines(_app: &App) -> Vec<Line<'static>> {
    // vec![Line::from(format!(
    //     "Gateway {}",
    //     app.runtime_cfg.base_url()
    // ))]
    vec![]
}

fn panel_header_line(_app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Born of panda. Awakened as Anda. ", theme::subtle_style()),
        Span::styled(format!(" ANDA.Bot v{APP_VERSION} "), theme::badge_style()),
    ])
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
    } else if !app.input_focused {
        "Press Enter or start typing to focus the input."
    } else {
        ""
    }
}

fn build_prompt_lines(app: &App, placeholder: &str, width: usize) -> Vec<Line<'static>> {
    let content_width = width
        .saturating_sub(input_prompt_prefix_width() as usize)
        .max(1);

    if !app.chat_enabled() || app.input_buf.is_empty() {
        if placeholder.is_empty() {
            return vec![Line::from(vec![Span::styled(
                INPUT_PROMPT_PREFIX.to_string(),
                theme::accent_style(),
            )])];
        }

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
        if placeholder.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                INPUT_PROMPT_PREFIX.to_string(),
                theme::accent_style(),
            )]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(INPUT_PROMPT_PREFIX.to_string(), theme::accent_style()),
                Span::styled(placeholder.to_string(), theme::dim_style()),
            ]));
        }
    }

    lines
}

fn input_prompt_prefix_width() -> u16 {
    display_width(INPUT_PROMPT_PREFIX) as u16
}

fn split_input_area(area: Rect) -> (Option<Rect>, Rect) {
    let separator_height = input_separator_block_height(area.height);
    if separator_height == 0 {
        return (None, area);
    }

    (
        Some(Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: separator_height,
        }),
        Rect {
            x: area.x,
            y: area.y + separator_height,
            width: area.width,
            height: area.height - separator_height,
        },
    )
}

#[cfg(test)]
fn chat_message_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    chat_message_lines_for_messages(&app.chat.messages, width)
}

fn chat_message_lines_for_messages(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    let mut rendered_lines = Vec::new();
    for msg in messages {
        rendered_lines.extend(chat_message_lines_for_message(msg, width));
    }
    rendered_lines
}

fn chat_message_lines_for_message(msg: &Message, width: usize) -> Vec<Line<'static>> {
    let mut rendered_lines: Vec<Line> = Vec::new();
    let (prefix, prefix_style, body_style) = match msg.role.as_str() {
        "user" => ("❯ ", theme::accent_style(), theme::body_style()),
        "assistant" => ("🐼 ❯ ", theme::success_style(), theme::body_style()),
        "system" => ("⚠️ ❯ ", theme::danger_style(), theme::danger_style()),
        "tool" => ("🔧 ❯ ", theme::dim_style(), theme::dim_style()),
        _ => ("  ", theme::dim_style(), theme::body_style()),
    };

    let prefix_width = display_width(prefix);
    let continuation_prefix = " ".repeat(prefix_width);
    let content_width = width.saturating_sub(prefix_width).max(1);
    let mut first = true;
    let mut prev_kind: Option<PartKind> = None;

    for part in &msg.content {
        let kind = part_kind(part);
        ensure_part_spacing(&mut rendered_lines, prev_kind, kind);
        match part {
            ContentPart::Text { text } => {
                push_wrapped_block(
                    &mut rendered_lines,
                    text,
                    &mut first,
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    body_style,
                    content_width,
                );
            }
            ContentPart::Reasoning { text } => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("thinking: {text}"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::ToolCall { name, args, .. } => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("→ {name}({args})"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::ToolOutput { name, output, .. } => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("← {name}: {output}"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::FileData {
                file_uri,
                mime_type,
            } => {
                let mime = mime_type.as_deref().unwrap_or("file");
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("📎 [{mime}] {file_uri}"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::InlineData { mime_type, .. } => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("[inline {mime_type}]"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::Action { name, .. } => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &format!("⚡ {name}"),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
            ContentPart::Any(json) => {
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &json.to_string(),
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    SECONDARY_PART_MAX_LINES,
                );
            }
        }
        prev_kind = Some(kind);
    }

    if prev_kind.is_some() {
        rendered_lines.push(Line::from(""));
    }

    rendered_lines
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Normal,
    Limited,
}

fn part_kind(part: &ContentPart) -> PartKind {
    match part {
        ContentPart::Text { .. } => PartKind::Normal,
        _ => PartKind::Limited,
    }
}

fn ensure_part_spacing(
    rendered_lines: &mut Vec<Line<'static>>,
    prev_kind: Option<PartKind>,
    next_kind: PartKind,
) {
    let Some(prev_kind) = prev_kind else {
        return;
    };
    if prev_kind == next_kind {
        return;
    }
    if rendered_lines.last().is_some_and(line_is_blank) {
        return;
    }
    rendered_lines.push(Line::from(""));
}

#[cfg(test)]
fn thinking_lines(app: &App) -> Vec<Line<'static>> {
    if !app.chat.is_thinking() {
        return Vec::new();
    }

    vec![Line::from(vec![
        Span::styled("🐼 ❯ ", theme::success_style()),
        Span::styled(
            input_separator_label(app).into_owned(),
            theme::subtle_style(),
        ),
    ])]
}

fn flush_static_scrollback(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let area = terminal.get_frame().area();
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }

    if !app.static_panel_flushed {
        let height = static_panel_height(app, area.width);
        terminal.insert_before(height, |buf| {
            render_static_panel_to_buffer(app, buf.area, buf);
        })?;
        app.static_panel_flushed = true;
    }

    if app.flushed_message_count >= app.chat.messages.len() {
        return Ok(());
    }

    let lines = chat_message_lines_for_messages(
        &app.chat.messages[app.flushed_message_count..],
        area.width as usize,
    );
    if !lines.is_empty() {
        insert_lines_before(terminal, lines)?;
    }
    app.flushed_message_count = app.chat.messages.len();

    Ok(())
}

fn insert_lines_before(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    lines: Vec<Line<'static>>,
) -> Result<(), BoxError> {
    if lines.is_empty() {
        return Ok(());
    }

    terminal.insert_before(lines.len() as u16, |buf| {
        PackedLines::new(lines)
            .style(theme::body_style())
            .render(buf.area, buf);
    })?;

    Ok(())
}

fn input_separator_block_height(area_height: u16) -> u16 {
    if area_height <= 1 {
        0
    } else if area_height > INPUT_DIVIDER_PADDED_HEIGHT {
        INPUT_DIVIDER_PADDED_HEIGHT
    } else {
        1
    }
}

fn input_separator_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    if area.height == 0 {
        return Vec::new();
    }

    let separator = input_separator_line(app, area.width as usize);
    if area.height >= INPUT_DIVIDER_PADDED_HEIGHT {
        vec![Line::from(""), separator, Line::from("")]
    } else {
        vec![separator]
    }
}

fn input_separator_line(app: &App, width: usize) -> Line<'static> {
    let label = input_separator_label(app);
    let content = input_separator_text(label.as_ref(), width);
    let total_width = content.chars().count();
    if total_width == 0 {
        return Line::from("");
    }

    let label_start = INPUT_DIVIDER_PREFIX.chars().count().min(total_width);
    let label_end = (label_start + label.chars().count()).min(total_width);
    let spans: Vec<Span<'static>> = content
        .chars()
        .enumerate()
        .map(|(idx, ch)| {
            Span::styled(
                ch.to_string(),
                input_separator_style(
                    idx,
                    total_width,
                    label_start <= idx && idx < label_end,
                    app.animation_tick,
                ),
            )
        })
        .collect();

    Line::from(spans)
}

fn input_separator_label(app: &App) -> Cow<'static, str> {
    if app.chat.is_thinking() {
        Cow::Owned(format!("{THINKING_LABEL} {}", thinking_frame(app)))
    } else {
        Cow::Borrowed(INPUT_DIVIDER_LABEL)
    }
}

fn thinking_frame(app: &App) -> &'static str {
    THINKING_FRAMES[(app.animation_tick as usize / 2) % THINKING_FRAMES.len()]
}

fn input_separator_text(label: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let compact = format!("{INPUT_DIVIDER_PREFIX}{label}");
    let compact_width = display_width(&compact);

    if width <= compact_width {
        return truncate_visual(&compact, width);
    }

    let filler_width = width.saturating_sub(compact_width + 1);
    format!("{compact} {}", "─".repeat(filler_width))
}

fn input_separator_style(position: usize, total_width: usize, in_label: bool, tick: u64) -> Style {
    let glow = input_separator_glow_strength(position, total_width, tick);
    let shimmer = (((position as f32 * 0.24) - (tick as f32 * 0.42)).sin() + 1.0) * 0.5;
    let fg_mix = if in_label {
        (0.38 + shimmer * 0.18 + glow * 0.54).min(1.0)
    } else {
        (0.20 + shimmer * 0.12 + glow * 0.82).min(1.0)
    };
    let bg_mix = if in_label {
        (0.14 + glow * 0.82 + shimmer * 0.06).min(1.0)
    } else {
        (0.04 + glow * 0.72).min(1.0)
    };

    let fg = if in_label {
        blend_color(Color::Rgb(92, 242, 255), theme::PANDA_WHITE, fg_mix)
    } else {
        blend_color(Color::Rgb(82, 134, 136), Color::Rgb(220, 255, 245), fg_mix)
    };
    let bg = if in_label {
        None
    } else {
        Some(blend_color(
            Color::Rgb(3, 10, 11),
            Color::Rgb(0, 108, 98),
            bg_mix,
        ))
    };

    let style = if let Some(bg) = bg {
        Style::default().fg(fg).bg(bg)
    } else {
        Style::default().fg(fg)
    };
    if in_label || glow > 0.22 {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn input_separator_glow_strength(position: usize, total_width: usize, tick: u64) -> f32 {
    if total_width == 0 {
        return 0.0;
    }

    let total_width = total_width as f32;
    let center = (tick as f32 * INPUT_DIVIDER_FLOW_SPEED).rem_euclid(total_width);
    let position = position as f32;
    let head = wrapped_glow_strength(position, total_width, center, INPUT_DIVIDER_GLOW_RADIUS);
    let tail = wrapped_glow_strength(
        position,
        total_width,
        center - INPUT_DIVIDER_TRAIL_OFFSET,
        INPUT_DIVIDER_GLOW_RADIUS * 1.1,
    ) * 0.58;
    let lead = wrapped_glow_strength(
        position,
        total_width,
        center + INPUT_DIVIDER_TRAIL_OFFSET * 0.45,
        INPUT_DIVIDER_GLOW_RADIUS * 0.65,
    ) * 0.22;

    (head + tail + lead).min(1.0)
}

fn wrapped_glow_strength(position: f32, total_width: f32, center: f32, radius: f32) -> f32 {
    let distance = (position - center).abs();
    let wrapped_distance = distance.min(total_width - distance);
    (1.0 - wrapped_distance / radius).max(0.0).powf(1.45)
}

fn blend_color(base: Color, peak: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let (base_r, base_g, base_b) = color_components(base);
    let (peak_r, peak_g, peak_b) = color_components(peak);

    Color::Rgb(
        mix_channel(base_r, peak_r, amount),
        mix_channel(base_g, peak_g, amount),
        mix_channel(base_b, peak_b, amount),
    )
}

fn color_components(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (255, 255, 255),
    }
}

fn mix_channel(base: u8, peak: u8, amount: f32) -> u8 {
    let base = base as f32;
    let peak = peak as f32;
    (base + (peak - base) * amount).round().clamp(0.0, 255.0) as u8
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn compact_cjk_spacing<'a>(text: &'a str) -> Cow<'a, str> {
    if !text.as_bytes().contains(&b' ') {
        return Cow::Borrowed(text);
    }

    let chars: Vec<char> = text.chars().collect();
    let mut compacted = String::with_capacity(text.len());
    let mut changed = false;

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch == ' '
            && should_compact_cjk_space(
                idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied(),
                chars.get(idx + 1).copied(),
            )
        {
            changed = true;
            continue;
        }

        compacted.push(ch);
    }

    if changed {
        Cow::Owned(compacted)
    } else {
        Cow::Borrowed(text)
    }
}

fn compact_cjk_spacing_with_cursor(text: &str, cursor_chars: usize) -> (String, usize) {
    if !text.as_bytes().contains(&b' ') {
        return (text.to_string(), cursor_chars.min(text.chars().count()));
    }

    let chars: Vec<char> = text.chars().collect();
    let cursor_chars = cursor_chars.min(chars.len());
    let mut compacted = String::with_capacity(text.len());
    let mut normalized_cursor = 0;

    for (idx, ch) in chars.iter().copied().enumerate() {
        let keep = ch != ' '
            || !should_compact_cjk_space(
                idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied(),
                chars.get(idx + 1).copied(),
            );

        if keep {
            compacted.push(ch);
            if idx < cursor_chars {
                normalized_cursor += 1;
            }
        }
    }

    (compacted, normalized_cursor)
}

fn should_compact_cjk_space(prev: Option<char>, next: Option<char>) -> bool {
    matches!((prev, next), (Some(prev), Some(next)) if is_cjk_display_char(prev) && is_cjk_display_char(next))
}

fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
}

fn is_cjk_display_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x31C0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE6F
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
    )
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
    ch.width().unwrap_or(0)
}

fn display_width(text: &str) -> usize {
    text.width()
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

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let grapheme_width = display_width(grapheme);
        if current_width + grapheme_width > target_width {
            break;
        }
        truncated.push_str(grapheme);
        current_width += grapheme_width;
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
        let text_line = compact_cjk_spacing(text_line);
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

        for chunk in wrap_visual(text_line.as_ref(), content_width) {
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

#[allow(clippy::too_many_arguments)]
fn push_limited_block(
    rendered_lines: &mut Vec<Line<'static>>,
    first: &mut bool,
    text: &str,
    prefix: &str,
    continuation_prefix: &str,
    first_prefix_style: Style,
    continuation_prefix_style: Style,
    body_style: Style,
    content_width: usize,
    max_lines: usize,
) {
    for chunk in limited_visual_lines(text, content_width, max_lines) {
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

fn limited_visual_lines(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(1);
    let max_lines = max_lines.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let mut truncated = false;
    let normalized = normalize_newlines(text);
    let normalized = compact_cjk_spacing(&normalized);

    for grapheme in UnicodeSegmentation::graphemes(normalized.as_ref(), true) {
        if grapheme == "\n" {
            if !push_limited_line(&mut lines, &mut current, max_lines) {
                truncated = true;
                break;
            }
            current_width = 0;
            continue;
        }

        if grapheme.chars().any(char::is_control) {
            continue;
        }

        let grapheme_width = display_width(grapheme);
        if current_width + grapheme_width > width && !current.is_empty() {
            if !push_limited_line(&mut lines, &mut current, max_lines) {
                truncated = true;
                break;
            }
            current_width = 0;
        }

        current.push_str(grapheme);
        current_width += grapheme_width;
    }

    if !truncated && (!current.is_empty() || lines.is_empty()) {
        if lines.len() < max_lines {
            lines.push(current);
        } else {
            truncated = true;
        }
    }

    if truncated {
        append_ellipsis_to_last(&mut lines, width);
    }

    lines
}

fn push_limited_line(lines: &mut Vec<String>, current: &mut String, max_lines: usize) -> bool {
    if lines.len() >= max_lines {
        current.clear();
        return false;
    }

    lines.push(std::mem::take(current));
    true
}

fn append_ellipsis_to_last(lines: &mut Vec<String>, width: usize) {
    if lines.is_empty() {
        lines.push("...".to_string());
        return;
    }

    let last = lines.last_mut().expect("line exists");
    *last = truncate_with_ellipsis(last, width);
}

fn truncate_with_ellipsis(text: &str, width: usize) -> String {
    let width = width.max(1);
    if width <= 3 {
        return ".".repeat(width);
    }

    let target_width = width - 3;
    let mut truncated = String::new();
    let mut current_width = 0;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let grapheme_width = display_width(grapheme);
        if current_width + grapheme_width > target_width {
            break;
        }
        truncated.push_str(grapheme);
        current_width += grapheme_width;
    }

    truncated.push_str("...");
    truncated
}

fn wrap_visual(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let grapheme_width = display_width(grapheme);
        if current_width + grapheme_width > width && !current.is_empty() {
            wrapped.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push_str(grapheme);
        current_width += grapheme_width;
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

    fn push_text_message(app: &mut App, role: &str, text: &str) {
        app.chat.messages.push(anda_core::Message {
            role: role.to_string(),
            content: vec![ContentPart::Text {
                text: text.to_string(),
            }],
            ..Default::default()
        });
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
    fn insert_input_text_compacts_cjk_spaces_but_keeps_english_spaces() {
        let mut app = ready_app();

        app.insert_input_text("你 好 hello world 再 见");

        assert_eq!(app.input_buf, "你好 hello world 再见");
        assert_eq!(app.input_cursor, "你好 hello world 再见".chars().count());
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
    fn build_prompt_lines_hides_placeholder_while_input_is_focused() {
        let app = ready_app();

        let lines = build_prompt_lines(&app, input_placeholder(&app), 24);

        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "❯ ");
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
            4
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
    fn split_input_area_reserves_padding_when_room_allows() {
        let area = Rect {
            x: 2,
            y: 3,
            width: 40,
            height: 6,
        };

        let (separator, prompt) = split_input_area(area);

        assert_eq!(separator.unwrap().height, INPUT_DIVIDER_PADDED_HEIGHT);
        assert_eq!(prompt.y, area.y + INPUT_DIVIDER_PADDED_HEIGHT);
        assert_eq!(prompt.height, area.height - INPUT_DIVIDER_PADDED_HEIGHT);
    }

    #[test]
    fn input_separator_lines_add_blank_padding_when_room_allows() {
        let app = ready_app();
        let lines = input_separator_lines(
            &app,
            Rect {
                x: 0,
                y: 0,
                width: 24,
                height: INPUT_DIVIDER_PADDED_HEIGHT,
            },
        );

        assert_eq!(line_text(&lines[0]), "");
        assert!(line_text(&lines[1]).starts_with("── compose "));
        assert_eq!(line_text(&lines[2]), "");
    }

    #[test]
    fn input_separator_line_labels_compose_section() {
        let app = ready_app();
        let line = input_separator_line(&app, 24);

        assert!(line_text(&line).starts_with("── compose "));
    }

    #[test]
    fn input_separator_line_shows_fixed_width_thinking_status() {
        let mut app = ready_app();
        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Submitted,
            ..Default::default()
        });

        app.animation_tick = 0;
        let first = line_text(&input_separator_line(&app, 32));
        app.animation_tick = 2;
        let second = line_text(&input_separator_line(&app, 32));
        let frame_widths: Vec<_> = THINKING_FRAMES
            .iter()
            .map(|frame| display_width(&format!("{THINKING_LABEL} {frame}")))
            .collect();

        assert!(first.starts_with("── thinking ⠋ "));
        assert!(second.starts_with("── thinking ⠙ "));
        assert_ne!(first, second);
        assert_eq!(display_width(&first), 32);
        assert_eq!(display_width(&second), 32);
        assert!(
            THINKING_FRAMES
                .iter()
                .all(|frame| display_width(frame) == 1)
        );
        assert!(frame_widths.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn input_separator_title_has_no_background() {
        let app = ready_app();
        let line = input_separator_line(&app, 24);
        let label_start = INPUT_DIVIDER_PREFIX.chars().count();
        let label_end = label_start + INPUT_DIVIDER_LABEL.chars().count();

        assert!(
            line.spans[label_start..label_end]
                .iter()
                .all(|span| span.style.bg.is_none())
        );
        assert!(
            line.spans[..label_start]
                .iter()
                .any(|span| matches!(span.style.bg, Some(Color::Rgb(..))))
        );
    }

    #[test]
    fn input_separator_line_animates_truecolor_glow() {
        let mut app = ready_app();
        app.animation_tick = 0;
        let before = input_separator_line(&app, 24);

        app.animation_tick = 8;
        let after = input_separator_line(&app, 24);

        let before_colors: Vec<_> = before.spans.iter().map(|span| span.style.fg).collect();
        let after_colors: Vec<_> = after.spans.iter().map(|span| span.style.fg).collect();
        let before_backgrounds: Vec<_> = before.spans.iter().map(|span| span.style.bg).collect();
        let after_backgrounds: Vec<_> = after.spans.iter().map(|span| span.style.bg).collect();

        assert_eq!(line_text(&before), line_text(&after));
        assert!(
            before_colors
                .iter()
                .any(|color| matches!(color, Some(Color::Rgb(..))))
        );
        assert!(
            before_backgrounds
                .iter()
                .any(|color| matches!(color, Some(Color::Rgb(..))))
        );
        assert_ne!(before_colors, after_colors);
        assert_ne!(before_backgrounds, after_backgrounds);
    }

    #[test]
    fn chat_message_lines_render_thoughts_as_limited_dim_excerpt() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "assistant".to_string(),
            content: vec![
                ContentPart::Text {
                    text: "Answer ready".to_string(),
                },
                ContentPart::Reasoning {
                    text: "first line of reasoning\nsecond line that is long enough to truncate"
                        .to_string(),
                },
            ],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 28);

        assert_eq!(line_text(&lines[0]), "🐼 ❯ Answer ready");
        assert_eq!(line_text(&lines[1]), "");
        let secondary = &lines[2..lines.len() - 1];
        assert!(secondary.len() <= SECONDARY_PART_MAX_LINES);
        assert_eq!(secondary[0].spans[0].style, theme::dim_style());
        assert_eq!(secondary[0].spans[1].style, theme::dim_style());
        assert!(line_text(&secondary[0]).contains("thinking:"));
        assert!(line_text(secondary.last().expect("secondary line")).ends_with("..."));
        assert_eq!(line_text(lines.last().expect("blank line")), "");
    }

    #[test]
    fn chat_message_lines_insert_gap_before_limited_parts() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "assistant".to_string(),
            content: vec![
                ContentPart::Text {
                    text: "正文内容".to_string(),
                },
                ContentPart::ToolCall {
                    name: "search".to_string(),
                    args: serde_json::json!({"q": "anda"}),
                    call_id: None,
                },
            ],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 80);

        assert_eq!(line_text(&lines[0]), "🐼 ❯ 正文内容");
        assert_eq!(line_text(&lines[1]), "");
        assert!(line_text(&lines[2]).starts_with("     → search("));
    }

    #[test]
    fn chat_message_lines_expand_multiline_text() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "assistant".to_string(),
            content: vec![ContentPart::Text {
                text: "line one\nline two\nline three".to_string(),
            }],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 40);

        assert_eq!(line_text(&lines[0]), "🐼 ❯ line one");
        assert_eq!(line_text(&lines[1]), "     line two");
        assert_eq!(line_text(&lines[2]), "     line three");
        assert_eq!(line_text(&lines[3]), "");
    }

    #[test]
    fn chat_message_lines_render_tool_calls_as_dim_summary() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "assistant".to_string(),
            content: vec![ContentPart::ToolCall {
                name: "search".to_string(),
                args: serde_json::json!({"q": "anda"}),
                call_id: None,
            }],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 80);

        assert!(line_text(&lines[0]).starts_with("🐼 ❯ → search("));
        assert_eq!(lines[0].spans[1].style, theme::dim_style());
    }

    #[test]
    fn chat_message_lines_limit_tool_output_to_three_lines() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "tool".to_string(),
            content: vec![ContentPart::ToolOutput {
                name: "shell".to_string(),
                output: serde_json::json!({"stdout": "a very long output line that should wrap and be truncated before it can take over the transcript area"}),
                call_id: None,
                remote_id: None,
            }],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 28);
        let secondary = &lines[..lines.len() - 1];

        assert_eq!(secondary.len(), SECONDARY_PART_MAX_LINES);
        assert!(
            secondary
                .iter()
                .all(|line| line.spans[1].style == theme::dim_style())
        );
        assert!(line_text(secondary.last().expect("secondary line")).ends_with("..."));
    }

    #[test]
    fn chat_message_lines_render_errors_with_system_prefix() {
        let mut app = ready_app();
        app.chat.messages.push(anda_core::Message {
            role: "system".to_string(),
            content: vec![ContentPart::Text {
                text: "request failed badly".to_string(),
            }],
            ..Default::default()
        });

        let lines = chat_message_lines(&app, 40);

        assert_eq!(line_text(&lines[0]), "⚠️ ❯ request failed badly");
        assert_eq!(lines[0].spans[0].style, theme::danger_style());
        assert_eq!(lines[0].spans[1].style, theme::danger_style());
        assert_eq!(line_text(&lines[1]), "");
    }

    #[test]
    fn chat_message_lines_keep_cjk_text_contiguous() {
        let mut app = ready_app();
        push_text_message(&mut app, "user", "前面出错了，再试试。");

        let lines = chat_message_lines(&app, 80);

        assert_eq!(line_text(&lines[0]), "❯ 前面出错了，再试试。");
        assert_eq!(display_width("前面出错了，再试试。"), 20);
    }

    #[test]
    fn chat_message_lines_compact_ascii_spaces_between_cjk() {
        let mut app = ready_app();
        push_text_message(&mut app, "assistant", "已 提 交，依 赖 版 本 升 级。");

        let lines = chat_message_lines(&app, 80);

        assert_eq!(line_text(&lines[0]), "🐼 ❯ 已提交，依赖版本升级。");
    }

    #[test]
    fn wrap_visual_splits_cjk_by_display_width_without_spaces() {
        assert_eq!(wrap_visual("前面出错", 4), vec!["前面", "出错"]);
        assert_eq!(truncate_visual("前面出错", 7), "前面...");
    }

    #[test]
    fn dynamic_viewport_height_excludes_static_messages() {
        let mut app = ready_app();
        let before = dynamic_viewport_height(&app, 80, 30);
        for idx in 0..6 {
            push_text_message(&mut app, "user", &format!("message {idx}"));
        }

        assert_eq!(dynamic_viewport_height(&app, 80, 30), before);
    }

    #[test]
    fn status_footer_switches_between_help_and_status() {
        let mut app = ready_app();

        let help = status_footer_lines(&app, 80);
        assert!(line_text(&help[0]).starts_with("? "));

        app.input_focused = false;
        let status = status_footer_lines(&app, 80);
        assert!(line_text(&status[0]).starts_with("READY "));
    }

    #[test]
    fn status_footer_height_reserves_separator_row() {
        let app = ready_app();

        assert_eq!(status_footer_height(&app, 80), 3);
    }

    #[test]
    fn status_footer_panel_starts_below_divider() {
        assert_eq!(
            status_footer_panel(Rect {
                x: 2,
                y: 4,
                width: 30,
                height: 3,
            }),
            Rect {
                x: 2,
                y: 5,
                width: 30,
                height: 2,
            }
        );
    }

    #[test]
    fn status_footer_notice_compacts_spaced_cjk() {
        let mut app = ready_app();
        app.notice = "连 接 失 败，请 重 试。".to_string();

        let lines = status_footer_lines(&app, 80);

        assert_eq!(
            line_text(lines.last().expect("notice line")),
            "! 连接失败，请重试。"
        );
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
        assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking ⠋");

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Working,
            ..Default::default()
        });
        assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking ⠋");

        app.chat.conversation = Some(Conversation {
            status: ConversationStatus::Completed,
            ..Default::default()
        });
        assert!(thinking_lines(&app).is_empty());
    }

    #[test]
    fn packed_lines_writes_each_grapheme_to_its_own_cell() {
        use ratatui::widgets::Widget;

        let area = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 1,
        };
        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![Line::from(vec![
            Span::styled("❯ ", theme::accent_style()),
            Span::styled("中文", theme::body_style()),
        ])])
        .render(area, &mut buf);

        // Prefix grapheme-by-grapheme: "❯" at col 0, " " at col 1.
        assert_eq!(buf[(0, 0)].symbol(), "❯");
        assert_eq!(buf[(1, 0)].symbol(), " ");
        // CJK graphemes occupy width-2 cells: cell at col 2 holds "中"
        // and ratatui implicitly skips col 3; "文" lands at col 4.
        assert_eq!(buf[(2, 0)].symbol(), "中");
        assert_eq!(buf[(4, 0)].symbol(), "文");
        // Style preserved on the body cells.
        assert_eq!(buf[(2, 0)].fg, theme::body_style().fg.unwrap());
    }
}
