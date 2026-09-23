use std::{
    io,
    time::{Duration, Instant},
};

use anda_core::BoxError;
use crossterm::{
    ExecutableCommand,
    cursor::{MoveTo, MoveToNextLine},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    terminal::{
        Clear, ClearType, disable_raw_mode, enable_raw_mode, size, supports_keyboard_enhancement,
    },
};
use futures::{FutureExt, StreamExt};
use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect};
#[cfg(unix)]
use std::io::IsTerminal;

use crate::{daemon::Daemon, gateway};

use super::{
    App, STATUS_REFRESH_INTERVAL,
    backend::TuiBackend,
    layout::{dynamic_viewport_height, input_navigation_content_width},
    render::{flush_static_scrollback, render},
};

pub async fn run(
    daemon: Daemon,
    client: gateway::Client,
    full_access: bool,
) -> Result<(), BoxError> {
    #[cfg(unix)]
    reopen_stdin_from_tty()?;

    let mut app = App::new(daemon.home, daemon.cfg, client, full_access);
    app.start_bootstrap();

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
    terminal: &mut Terminal<TuiBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), BoxError> {
    let mut last_status_refresh = Instant::now();
    let (mut term_w, mut term_h) = size()?;
    term_h = term_h.max(1);
    let mut current_viewport_height = terminal.get_frame().area().height;
    let mut needs_render = true;
    let mut events = event::EventStream::new();
    let mut ticks = tokio::time::interval(Duration::from_millis(150));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        needs_render |= app.finish_pending_bootstrap();
        needs_render |= app.finish_pending_status();
        let revision = app.chat.revision();
        if app.chat_enabled() {
            if let Some(err) = app.chat.finish_pending_send() {
                app.notice = err;
                needs_render = true;
            }
            needs_render |= app.chat.finish_pending_poll();
            needs_render |= app.finish_pending_action_response();
        }
        needs_render |= revision != app.chat.revision();
        needs_render |= app.finish_pending_update_check();
        needs_render |= app.finish_pending_memory();
        needs_render |= app.finish_pending_memory_inbox();
        needs_render |= app.refresh_actions();

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
            app.start_status_refresh();
            last_status_refresh = Instant::now();
        }
        if app.chat_enabled() {
            app.chat.start_poll(None);
        }

        let mut next_event = tokio::select! {
            _ = ticks.tick() => continue,
            event = events.next() => event,
        };
        if next_event.is_none() {
            break;
        }
        // Coalesce buffered typing into one frame, without blocking a Tokio
        // worker or waiting for any HTTP request in the input path.
        while let Some(event) = next_event {
            match event? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let width = if matches!(key.code, KeyCode::Up | KeyCode::Down) {
                        input_navigation_content_width(app, terminal.get_frame().area())
                    } else {
                        1
                    };
                    if let Err(err) = app.handle_key(key, width).await {
                        app.notice = err.to_string();
                    }
                    needs_render = true;
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                    needs_render = true;
                }
                Event::Resize(_, _) => needs_render = true,
                _ => {}
            }
            if app.should_quit {
                break;
            }
            next_event = events.next().now_or_never().flatten();
        }
    }
    Ok(())
}

fn create_terminal_with_height(
    viewport_height: u16,
) -> Result<Terminal<TuiBackend<io::Stdout>>, BoxError> {
    let mut terminal = Terminal::with_options(
        TuiBackend::new(io::stdout()),
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
