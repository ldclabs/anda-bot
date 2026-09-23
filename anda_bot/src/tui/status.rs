use ratatui::text::{Line, Span};

use crate::config::APP_VERSION;

use super::{
    App, STATUS_FOOTER_MAX_LINES,
    text::{display_width, truncate_visual},
    theme,
};

pub(super) fn status_footer_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    if let Some(line) = app.action_footer_line(width) {
        lines.push(line);
    }

    if !app.notice.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("! ", theme::warn_style()),
            Span::styled(
                truncate_visual(&app.notice, width.saturating_sub(2)),
                theme::warn_style(),
            ),
        ]));
    }

    if app.input_focused
        && app.chat_enabled()
        && !app.chat.sending
        && !app.action_response_pending()
    {
        lines.extend([
            Line::from(vec![
                Span::styled("? ", theme::accent_style()),
                Span::styled(
                    truncate_visual(
                        "Enter send  •  Shift+Enter/Ctrl+J newline  •  ↑/↓ move lines  •  Ctrl+U clear  •  Ctrl+C quit",
                        width.saturating_sub(2),
                    ),
                    theme::subtle_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("? ", theme::accent_style()),
                Span::styled(
                    truncate_visual(
                        "/new [message]  •  /memory  •  /goal message  •  /loop message  •  /skill skill-name message  •  /side message  •  /steer message  •  /stop task  •  /cancel session",
                        width.saturating_sub(2),
                    ),
                    theme::subtle_style(),
                ),
            ]),
        ]);
    } else {
        lines.push(status_line(app, width));
    }

    lines.truncate(STATUS_FOOTER_MAX_LINES);
    lines
}

pub(super) fn status_line(app: &App, width: usize) -> Line<'static> {
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
        let access = if app.full_access {
            " · full-access"
        } else {
            ""
        };
        (
            "READY",
            theme::success_style(),
            format!(
                "conversation {conversation} · state {}{access}",
                app.chat.status_label()
            ),
        )
    };

    let prefix = format!("{badge} ");
    Line::from(vec![
        Span::styled(prefix.clone(), style),
        Span::styled(
            truncate_visual(&text, width.saturating_sub(display_width(&prefix))),
            theme::subtle_style(),
        ),
    ])
}
pub(super) fn panel_header_line(_app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Born of panda. Awakened as Anda. ", theme::subtle_style()),
        Span::styled(format!(" ANDA.Bot v{APP_VERSION} "), theme::accent_style()),
    ])
}
