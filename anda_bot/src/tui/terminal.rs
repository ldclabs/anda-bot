use std::{
    io,
    time::{Duration, Instant},
};

use anda_core::BoxError;
use crossterm::{
    ExecutableCommand,
    cursor::{MoveTo, MoveToNextLine},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    terminal::{
        Clear, ClearType, disable_raw_mode, enable_raw_mode, size, supports_keyboard_enhancement,
    },
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
#[cfg(unix)]
use std::io::IsTerminal;

use crate::{daemon::Daemon, gateway};

use super::{
    App, STATUS_REFRESH_INTERVAL,
    action::{TuiActionState, action_state_snapshot},
    layout::{dynamic_viewport_height, input_navigation_content_width},
    render::{flush_static_scrollback, render},
};

pub async fn run(daemon: Daemon, client: gateway::Client) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client);
    app.bootstrap().await;

    enable_raw_mode()?;
    let mut terminal_modes_guard = TerminalModesGuard::new();
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
    terminal_modes_guard.keyboard_enhancement_pushed = keyboard_enhancement_pushed;

    // Size the inline viewport to the initial content, not the full terminal,
    // so the TUI expands from the current cursor row instead of reserving the
    // whole screen (which would push prior history up and anchor us at the
    // bottom).
    let (term_w, term_h) = size()?;
    let initial_height = dynamic_viewport_height(&app, term_w, term_h.max(1));
    let mut terminal = create_terminal_with_height(initial_height)?;
    let run_result = run_app(&mut terminal, &mut app).await;
    let cleanup_result = cleanup_inline_viewport(&mut stdout, terminal.get_frame().area());
    drop(terminal);

    // Normal exit path: run the ordered cleanup ourselves and disarm the
    // unwind guard so terminal modes are not restored twice.
    terminal_modes_guard.disarm();
    if keyboard_enhancement_pushed {
        let _ = stdout.execute(PopKeyboardEnhancementFlags);
    }
    let paste_mode_result = stdout.execute(DisableBracketedPaste);
    let raw_mode_result = disable_raw_mode();

    paste_mode_result?;
    raw_mode_result?;
    cleanup_result?;
    run_result
}

/// Restores terminal modes if `run` unwinds (panic or an early `?` return
/// after raw mode was enabled). Leaving raw mode on would break the user's
/// shell session.
struct TerminalModesGuard {
    keyboard_enhancement_pushed: bool,
    armed: bool,
}

impl TerminalModesGuard {
    fn new() -> Self {
        Self {
            keyboard_enhancement_pushed: false,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TerminalModesGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut stdout = io::stdout();
        if self.keyboard_enhancement_pushed {
            let _ = stdout.execute(PopKeyboardEnhancementFlags);
        }
        let _ = stdout.execute(DisableBracketedPaste);
        let _ = disable_raw_mode();
    }
}
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let mut last_status_refresh = Instant::now();
    let (mut term_w, mut term_h) = size()?;
    term_h = term_h.max(1);
    let mut current_viewport_height = terminal.get_frame().area().height;
    let mut needs_render = true;

    loop {
        let before_send = chat_render_snapshot(app);
        let notice_before_send = app.notice.clone();
        if app.chat_enabled() {
            if let Some(err) = app.chat.finish_pending_send().await {
                app.notice = err;
            }
            needs_render |= app.finish_pending_action_response().await;
            needs_render |=
                before_send != chat_render_snapshot(app) || notice_before_send != app.notice;
        }

        needs_render |= app.finish_pending_update_check();

        // Recreate the terminal when the outer terminal was resized, or when
        // the dynamic bottom area (input + status footer) changed height in
        // either direction, so the inline viewport always hugs the prompt
        // without leaving dead rows behind.
        let (w, h) = size()?;
        let h = h.max(1);
        let terminal_resized = w != term_w || h != term_h;
        if terminal_resized {
            term_w = w;
            term_h = h;
            needs_render = true;
        }

        let new_height = dynamic_viewport_height(app, term_w, term_h).clamp(1, term_h);
        if app.pending_scrollback_purge {
            let old_area = terminal.get_frame().area();
            let mut stdout = io::stdout();
            stdout.execute(MoveTo(old_area.x, old_area.y))?;
            stdout.execute(Clear(ClearType::Purge))?;
            stdout.execute(Clear(ClearType::FromCursorDown))?;
            *terminal = create_terminal_with_height(new_height)?;
            current_viewport_height = new_height;
            app.pending_scrollback_purge = false;
            needs_render = true;
        }
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
            *terminal = create_terminal_with_height(new_height)?;
            current_viewport_height = new_height;
            needs_render = true;
        }

        if app.chat.is_thinking() {
            needs_render = true;
        }

        if needs_render {
            app.animation_tick = app.animation_tick.wrapping_add(1);
            terminal.autoresize()?;
            flush_static_scrollback(terminal, app)?;
            terminal.draw(|frame| render(frame, app))?;
            needs_render = false;
        }

        if app.should_quit {
            break;
        }

        if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
            let status_before = status_render_snapshot(app);
            let was_running = app.daemon_running;
            let _ = app.refresh_status().await;
            if app.setup.is_ready() && was_running && !app.daemon_running && app.notice.is_empty() {
                app.notice =
                    "Daemon connection lost. Press Enter to reload config.yaml and reconnect."
                        .to_string();
            }
            needs_render |= status_before != status_render_snapshot(app);
            last_status_refresh = Instant::now();
        }

        if app.chat_enabled() {
            let before_poll = chat_render_snapshot(app);
            let received = app.chat.poll(None).await;
            let after_poll = chat_render_snapshot(app);
            needs_render |= received || before_poll != after_poll;
        }

        if !event::poll(Duration::from_millis(150))? {
            continue;
        }

        // Drain every buffered event before looping back to render, so bursts
        // of keystrokes (fast typing, IME composition, key auto-repeat)
        // coalesce into a single frame instead of one render per key.
        loop {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let input_content_width =
                        input_navigation_content_width(app, terminal.get_frame().area());
                    if let Err(err) = app.handle_key(key, input_content_width).await {
                        app.notice = err.to_string();
                    }
                    needs_render = true;
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                    needs_render = true;
                }
                Event::Resize(_, _) => {
                    needs_render = true;
                }
                _ => {}
            }

            if app.should_quit || !event::poll(Duration::ZERO)? {
                break;
            }
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct ChatRenderSnapshot {
    conv_id: Option<u64>,
    conversation_id: Option<u64>,
    messages_len: usize,
    errors_len: usize,
    sending: bool,
    thinking: bool,
    status_label: &'static str,
    action_states: Vec<TuiActionState>,
}

fn chat_render_snapshot(app: &App) -> ChatRenderSnapshot {
    ChatRenderSnapshot {
        conv_id: app.chat.conv_id,
        conversation_id: app.chat.conversation.as_ref().map(|conv| conv._id),
        messages_len: app.chat.messages.len(),
        errors_len: app.chat.errors.len(),
        sending: app.chat.sending,
        thinking: app.chat.is_thinking(),
        status_label: app.chat.status_label(),
        action_states: action_state_snapshot(&app.chat.messages),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct StatusRenderSnapshot {
    pid: Option<u32>,
    daemon_running: bool,
    setup_ready: bool,
    notice: String,
}

fn status_render_snapshot(app: &App) -> StatusRenderSnapshot {
    StatusRenderSnapshot {
        pid: app.pid,
        daemon_running: app.daemon_running,
        setup_ready: app.setup.is_ready(),
        notice: app.notice.clone(),
    }
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

pub(super) fn cleanup_inline_viewport<W: io::Write>(writer: &mut W, area: Rect) -> io::Result<()> {
    writer.execute(MoveTo(area.x, area.y))?;
    writer.execute(Clear(ClearType::FromCursorDown))?;
    writer.execute(MoveToNextLine(area.height.max(1)))?;
    Ok(())
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
