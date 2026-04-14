mod theme;
mod widgets;

use anda_core::BoxError;
use anda_engine::model::reqwest;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::daemon::{self, BackgroundDaemon, Daemon, DaemonArgs};

use self::widgets::{Banner, InfoPanel};

const FIELD_COUNT: usize = 7;
const ACTION_SAVE: usize = FIELD_COUNT;
const ACTION_APPLY: usize = FIELD_COUNT + 1;
const ACTION_STOP: usize = FIELD_COUNT + 2;
const ACTION_RELOAD: usize = FIELD_COUNT + 3;
const ACTION_QUIT: usize = FIELD_COUNT + 4;
const TOTAL_ITEMS: usize = FIELD_COUNT + 5;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub async fn run(daemon: Daemon, http: reqwest::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.workspace, daemon.cfg, http);
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

struct App {
    workspace: PathBuf,
    http: reqwest::Client,
    runtime_cfg: DaemonArgs,
    persisted_cfg: DaemonArgs,
    draft_cfg: DaemonArgs,
    selected: usize,
    should_quit: bool,
    pid: Option<u32>,
    daemon_running: bool,
    status_message: String,
    notice: String,
}

impl App {
    fn new(workspace: PathBuf, cfg: DaemonArgs, http: reqwest::Client) -> Self {
        let base_url = cfg.base_url();
        Self {
            workspace,
            http,
            runtime_cfg: cfg.clone(),
            persisted_cfg: cfg.clone(),
            draft_cfg: cfg,
            selected: 0,
            should_quit: false,
            pid: None,
            daemon_running: false,
            status_message: "Waiting for status refresh...".to_string(),
            notice: format!("Opening daemon control for {base_url}"),
        }
    }

    async fn bootstrap(&mut self) {
        match ensure_daemon_running(&self.http, &self.runtime_daemon()).await {
            Ok(LaunchState::AlreadyRunning) => {
                self.notice = format!(
                    "Connected to anda daemon at {}.",
                    self.runtime_cfg.base_url()
                )
            }
            Ok(LaunchState::Started(child)) => {
                self.notice = format!(
                    "Started anda daemon in the background (pid {}). Logs: {}",
                    child.pid,
                    child.log_path.display()
                )
            }
            Err(err) => {
                self.notice = format!("Auto-start failed: {err}");
            }
        }

        if let Err(err) = self.refresh_status().await {
            self.status_message = format!("Status refresh failed: {err}");
        }
    }

    fn runtime_daemon(&self) -> Daemon {
        Daemon::new(self.workspace.clone(), self.runtime_cfg.clone())
    }

    fn draft_daemon(&self) -> Daemon {
        Daemon::new(self.workspace.clone(), self.draft_cfg.clone())
    }

    fn is_dirty(&self) -> bool {
        self.draft_cfg != self.persisted_cfg
    }

    fn next_item(&mut self) {
        self.selected = (self.selected + 1) % TOTAL_ITEMS;
    }

    fn prev_item(&mut self) {
        self.selected = if self.selected == 0 {
            TOTAL_ITEMS - 1
        } else {
            self.selected - 1
        };
    }

    fn is_action_selected(&self) -> bool {
        self.selected >= ACTION_SAVE
    }

    fn selected_help(&self) -> &'static str {
        match self.selected {
            0 => "Gateway listen address for the local daemon, for example 127.0.0.1:8042.",
            1 => "Enable the sandbox workspace for tool execution. Press Space or Enter to toggle.",
            2 => "Model family used by anda_bot, such as gemini, anthropic, openai, or deepseek.",
            3 => "Concrete model identifier sent to the provider.",
            4 => "Provider API key. The value is masked in the TUI.",
            5 => "Optional base URL for provider or proxy endpoints.",
            6 => "Optional HTTPS proxy URL. Clear this field to remove the proxy.",
            ACTION_SAVE => {
                "Write the draft configuration to workspace/.env without restarting the daemon."
            }
            ACTION_APPLY => {
                "Persist the draft config, then start or restart the daemon with the updated DaemonArgs."
            }
            ACTION_STOP => {
                "Stop the running background daemon and clear stale pid state if needed."
            }
            ACTION_RELOAD => {
                "Discard local edits and reload the last saved draft from this TUI session."
            }
            ACTION_QUIT => "Leave the TUI. The daemon keeps running unless you stop it first.",
            _ => "",
        }
    }

    fn selection_title(&self) -> String {
        match self.selected {
            0 => "addr".to_string(),
            1 => "sandbox".to_string(),
            2 => "model_family".to_string(),
            3 => "model_name".to_string(),
            4 => "model_api_key".to_string(),
            5 => "model_api_base".to_string(),
            6 => "https_proxy".to_string(),
            ACTION_SAVE => "save".to_string(),
            ACTION_APPLY => "apply".to_string(),
            ACTION_STOP => "stop".to_string(),
            ACTION_RELOAD => "reload".to_string(),
            ACTION_QUIT => "quit".to_string(),
            _ => String::new(),
        }
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
                .is_none_or(|value| value.is_empty()),
            _ => true,
        }
    }

    fn action_label(&self, index: usize) -> String {
        match index {
            ACTION_SAVE => "[ Save draft ]".to_string(),
            ACTION_APPLY => {
                if self.daemon_running || self.pid.is_some() {
                    "[ Apply + restart ]".to_string()
                } else {
                    "[ Apply + start ]".to_string()
                }
            }
            ACTION_STOP => "[ Stop daemon ]".to_string(),
            ACTION_RELOAD => "[ Reload saved ]".to_string(),
            ACTION_QUIT => "[ Quit ]".to_string(),
            _ => String::new(),
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

    async fn refresh_status(&mut self) -> Result<(), BoxError> {
        let daemon = self.runtime_daemon();
        self.pid = daemon.read_pid_file().await?;
        if let Some(pid) = self.pid
            && !daemon::process_exists(pid)
        {
            let _ = tokio::fs::remove_file(daemon.pid_file_path()).await;
            self.pid = None;
        }

        match probe_daemon_status(&self.http, &daemon.base_url()).await {
            Ok(()) => {
                self.daemon_running = true;
                self.status_message = format!("Daemon is serving {}.", daemon.base_url());
            }
            Err(err) => {
                self.daemon_running = false;
                self.status_message = if let Some(pid) = self.pid {
                    format!(
                        "PID {pid} exists but {} is not ready: {}",
                        daemon.base_url(),
                        err
                    )
                } else {
                    format!("No reachable daemon at {}: {}", daemon.base_url(), err)
                };
            }
        }

        Ok(())
    }

    async fn save_only(&mut self) -> Result<(), BoxError> {
        let daemon = self.draft_daemon();
        daemon.persist_config().await?;
        self.persisted_cfg = self.draft_cfg.clone();
        self.notice = if self.runtime_cfg == self.persisted_cfg {
            format!(
                "Saved daemon config to {}.",
                daemon.env_file_path().display()
            )
        } else {
            format!(
                "Saved daemon config to {}. Restart the daemon to apply changes.",
                daemon.env_file_path().display()
            )
        };
        Ok(())
    }

    async fn apply_and_restart(&mut self) -> Result<(), BoxError> {
        let daemon = self.draft_daemon();
        daemon.persist_config().await?;
        self.persisted_cfg = self.draft_cfg.clone();

        let had_running_daemon = self.daemon_running || self.pid.is_some();
        self.runtime_daemon()
            .stop_background(Duration::from_secs(10))
            .await?;

        let child = daemon.spawn_background()?;
        if let Err(err) =
            wait_for_daemon_ready(&self.http, &daemon.base_url(), Duration::from_secs(20)).await
        {
            self.notice = format!(
                "Daemon did not become ready. Inspect {}.",
                child.log_path.display()
            );
            let _ = self.refresh_status().await;
            return Err(format!(
                "{}; inspect {} for daemon logs",
                err,
                child.log_path.display()
            )
            .into());
        }

        self.runtime_cfg = self.draft_cfg.clone();
        self.notice = if had_running_daemon {
            format!(
                "Restarted anda daemon (pid {}) on {}. Logs: {}",
                child.pid,
                daemon.base_url(),
                child.log_path.display()
            )
        } else {
            format!(
                "Started anda daemon (pid {}) on {}. Logs: {}",
                child.pid,
                daemon.base_url(),
                child.log_path.display()
            )
        };
        self.refresh_status().await?;
        Ok(())
    }

    async fn stop_daemon(&mut self) -> Result<(), BoxError> {
        let stopped = self
            .runtime_daemon()
            .stop_background(Duration::from_secs(10))
            .await?;
        self.notice = if stopped {
            "Stopped anda daemon.".to_string()
        } else {
            "anda daemon was not running.".to_string()
        };
        self.refresh_status().await?;
        Ok(())
    }

    fn reload_saved(&mut self) {
        self.draft_cfg = self.persisted_cfg.clone();
        self.notice = "Reloaded saved daemon config into the draft editor.".to_string();
    }

    async fn activate_selected(&mut self) -> Result<(), BoxError> {
        match self.selected {
            ACTION_SAVE => self.save_only().await,
            ACTION_APPLY => self.apply_and_restart().await,
            ACTION_STOP => self.stop_daemon().await,
            ACTION_RELOAD => {
                self.reload_saved();
                Ok(())
            }
            ACTION_QUIT => {
                self.should_quit = true;
                Ok(())
            }
            _ => {
                self.toggle_selected_bool();
                Ok(())
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        let result = self.handle_key_inner(key).await;
        if let Err(err) = result {
            self.notice = err.to_string();
            let _ = self.refresh_status().await;
        }
    }

    async fn handle_key_inner(&mut self, key: KeyEvent) -> Result<(), BoxError> {
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
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.prev_item(),
            KeyCode::Down | KeyCode::Char('j') => self.next_item(),
            KeyCode::Tab => self.next_item(),
            KeyCode::BackTab => self.prev_item(),
            KeyCode::Enter => {
                self.activate_selected().await?;
            }
            KeyCode::Char(' ') => {
                if self.selected == 1 {
                    self.toggle_selected_bool();
                } else if self.is_action_selected() {
                    self.activate_selected().await?;
                } else {
                    self.push_char(' ');
                }
            }
            KeyCode::Backspace => self.pop_char(),
            KeyCode::Delete => self.clear_selected_field(),
            KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::ALT) => {
                if self.selected == 1 {
                    match ch {
                        't' | 'T' | 'y' | 'Y' | '1' => self.draft_cfg.sandbox = true,
                        'f' | 'F' | 'n' | 'N' | '0' => self.draft_cfg.sandbox = false,
                        _ => self.push_char(ch),
                    }
                } else {
                    self.push_char(ch);
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn status_label(&self) -> &'static str {
        if self.daemon_running {
            "RUNNING"
        } else if self.pid.is_some() {
            "UNHEALTHY"
        } else {
            "STOPPED"
        }
    }

    fn status_style(&self) -> Style {
        if self.daemon_running {
            theme::success_style()
        } else if self.pid.is_some() {
            theme::warn_style()
        } else {
            theme::danger_style()
        }
    }
}

enum LaunchState {
    AlreadyRunning,
    Started(BackgroundDaemon),
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let mut last_refresh = Instant::now();
    loop {
        terminal.draw(|frame| render(frame, app))?;

        if app.should_quit {
            break;
        }

        if last_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
            if let Err(err) = app.refresh_status().await {
                app.notice = format!("Status refresh failed: {err}");
            }
            last_refresh = Instant::now();
        }

        if !event::poll(Duration::from_millis(200))? {
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

fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 90 || area.height < 26 {
        render_small_terminal(frame, area);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(14),
            Constraint::Length(4),
        ])
        .split(area);

    frame.render_widget(
        Banner {
            subtitle: "Edit DaemonArgs, save them into .env, and start or restart the local daemon.",
        },
        outer[0],
    );

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(outer[1]);

    render_form_panel(frame, app, body[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Length(10),
            Constraint::Min(8),
        ])
        .split(body[1]);

    render_status_panel(frame, app, right[0]);
    render_selection_panel(frame, app, right[1]);
    render_shortcuts_panel(frame, app, right[2]);
    render_footer(frame, app, outer[2]);
}

fn render_small_terminal(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" anda ", theme::heading_style()));
    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            "Terminal is too small for the anda TUI.",
            theme::warn_style(),
        )),
        Line::from("Resize to at least 90x26 and re-open the interface."),
    ])
    .block(block)
    .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_form_panel(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    for index in 0..FIELD_COUNT {
        let selected = app.selected == index;
        let label_style = if selected {
            theme::selected_style()
        } else {
            theme::body_style().add_modifier(Modifier::BOLD)
        };
        let value_style = if selected {
            theme::selected_style()
        } else if app.field_is_empty(index) {
            theme::dim_style()
        } else {
            theme::input_style()
        };

        lines.push(Line::from(vec![
            Span::styled(if selected { "> " } else { "  " }, label_style),
            Span::styled(format!("{:<16}", App::field_label(index)), label_style),
            Span::styled(app.field_value(index), value_style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Actions", theme::heading_style())));

    for index in ACTION_SAVE..TOTAL_ITEMS {
        let selected = app.selected == index;
        let style = if selected {
            theme::selected_style()
        } else {
            match index {
                ACTION_APPLY => theme::accent_style(),
                ACTION_STOP => theme::danger_style(),
                _ => theme::body_style(),
            }
        };

        lines.push(Line::from(Span::styled(
            format!(
                "{}{}",
                if selected { "> " } else { "  " },
                app.action_label(index)
            ),
            style,
        )));
    }

    let title = if app.is_dirty() {
        "Daemon Draft (modified)"
    } else {
        "Daemon Draft"
    };
    frame.render_widget(InfoPanel { title, lines }, area);
}

fn render_status_panel(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("State: ", theme::dim_style()),
            Span::styled(app.status_label(), app.status_style()),
        ]),
        Line::from(vec![
            Span::styled("PID:   ", theme::dim_style()),
            Span::styled(
                app.pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                theme::body_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Live:  ", theme::dim_style()),
            Span::styled(app.runtime_cfg.base_url(), theme::body_style()),
        ]),
        Line::from(vec![
            Span::styled("Draft: ", theme::dim_style()),
            Span::styled(app.draft_cfg.base_url(), theme::body_style()),
        ]),
        Line::from(vec![
            Span::styled("Env:   ", theme::dim_style()),
            Span::styled(
                app.draft_daemon().env_file_path().display().to_string(),
                theme::body_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Logs:  ", theme::dim_style()),
            Span::styled(
                app.runtime_daemon().log_file_path().display().to_string(),
                theme::body_style(),
            ),
        ]),
    ];

    if app.runtime_cfg != app.draft_cfg {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Draft differs from the running daemon.",
            theme::warn_style(),
        )));
    }

    frame.render_widget(
        InfoPanel {
            title: "Status",
            lines,
        },
        area,
    );
}

fn render_selection_panel(frame: &mut Frame, app: &App, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Selected: ", theme::dim_style()),
            Span::styled(app.selection_title(), theme::heading_style()),
        ]),
        Line::from(""),
        Line::from(app.selected_help()),
        Line::from(""),
        Line::from(vec![
            Span::styled("Workspace: ", theme::dim_style()),
            Span::styled(app.workspace.display().to_string(), theme::body_style()),
        ]),
    ];
    frame.render_widget(
        InfoPanel {
            title: "Selection",
            lines,
        },
        area,
    );
}

fn render_shortcuts_panel(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Up/Down, Tab", theme::heading_style()),
            Span::raw(" move between fields and actions"),
        ]),
        Line::from(vec![
            Span::styled("Type / Backspace", theme::heading_style()),
            Span::raw(" edit the selected text field"),
        ]),
        Line::from(vec![
            Span::styled("Space / Enter", theme::heading_style()),
            Span::raw(" toggle sandbox or run the selected action"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+S", theme::heading_style()),
            Span::raw(" save the draft to .env"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+R", theme::heading_style()),
            Span::raw(" apply the draft and restart the daemon"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+X", theme::heading_style()),
            Span::raw(" stop the background daemon"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+L", theme::heading_style()),
            Span::raw(" reload the last saved draft"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+U or Delete", theme::heading_style()),
            Span::raw(" clear the selected field"),
        ]),
        Line::from(vec![
            Span::styled("Q or Ctrl+Q", theme::heading_style()),
            Span::raw(" quit the TUI"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            &app.status_message,
            if app.daemon_running {
                theme::success_style()
            } else {
                theme::dim_style()
            },
        )),
    ];

    if app.is_dirty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Unsaved edits are present in the draft.",
            theme::warn_style(),
        )));
    }

    frame.render_widget(
        InfoPanel {
            title: "Keys",
            lines,
        },
        area,
    );
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Notice: ", theme::heading_style()),
            Span::styled(&app.notice, theme::body_style()),
        ]),
        Line::from(vec![
            Span::styled("Saved state: ", theme::dim_style()),
            Span::styled(
                if app.is_dirty() { "modified" } else { "clean" },
                if app.is_dirty() {
                    theme::warn_style()
                } else {
                    theme::success_style()
                },
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style())
            .title(Span::styled(" Notice ", theme::heading_style())),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn display_value(value: &str) -> String {
    if value.trim().is_empty() {
        "(empty)".to_string()
    } else {
        value.to_string()
    }
}

fn display_optional(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.trim().is_empty() => value.to_string(),
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

    let tail = chars[chars.len() - 4..].iter().collect::<String>();
    format!("{}{}", "*".repeat(chars.len() - 4), tail)
}

async fn ensure_daemon_running(
    http: &reqwest::Client,
    daemon: &Daemon,
) -> Result<LaunchState, BoxError> {
    if probe_daemon_status(http, &daemon.base_url()).await.is_ok() {
        return Ok(LaunchState::AlreadyRunning);
    }

    let pid_path = daemon.pid_file_path();
    if let Some(pid) = daemon.read_pid_file().await? {
        if daemon::process_exists(pid) {
            wait_for_daemon_ready(http, &daemon.base_url(), Duration::from_secs(10)).await?;
            return Ok(LaunchState::AlreadyRunning);
        }
        let _ = tokio::fs::remove_file(&pid_path).await;
    }

    let child = daemon.spawn_background()?;
    if let Err(err) = wait_for_daemon_ready(http, &daemon.base_url(), Duration::from_secs(20)).await
    {
        return Err(format!(
            "{}; inspect {} for daemon logs",
            err,
            child.log_path.display()
        )
        .into());
    }

    Ok(LaunchState::Started(child))
}

async fn wait_for_daemon_ready(
    http: &reqwest::Client,
    base_url: &str,
    timeout: Duration,
) -> Result<(), BoxError> {
    let deadline = Instant::now() + timeout;
    let detail = loop {
        match probe_daemon_status(http, base_url).await {
            Ok(()) => return Ok(()),
            Err(err) if Instant::now() >= deadline => break err.to_string(),
            Err(_) => {}
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    };

    Err(format!(
        "anda daemon did not become ready on {base_url} within {:?}: {detail}",
        timeout
    )
    .into())
}

async fn probe_daemon_status(http: &reqwest::Client, base_url: &str) -> Result<(), BoxError> {
    let response = http.get(base_url).send().await?;
    match response.status() {
        http::StatusCode::OK => Ok(()),
        status => {
            let body = response.text().await.unwrap_or_default();
            Err(format!("status probe failed, status: {status}, body: {body}").into())
        }
    }
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
