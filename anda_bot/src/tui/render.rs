use std::io;

use anda_core::BoxError;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget},
};

use super::{
    App, STATUS_MAX_WIDTH,
    input::{
        input_placeholder, input_prompt_prefix_width, input_separator_lines, input_viewport,
        split_input_area,
    },
    layout::{centered_area, dynamic_layout_heights, static_panel_height, status_footer_panel},
    status::{panel_header_line, panel_lines, status_footer_lines},
    theme,
    transcript::chat_message_lines_for_messages,
    widgets::{Banner, PackedLines},
};

pub(super) fn render(frame: &mut Frame, app: &mut App) {
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

    let (text_area, scrollbar_area, input_viewport) = input_viewport(app, placeholder, prompt_area);
    frame.render_widget(
        PackedLines::new(input_viewport.lines).style(theme::body_style()),
        text_area,
    );

    if let Some(scrollbar_area) = scrollbar_area {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .track_style(theme::dim_style())
            .thumb_style(theme::accent_style());
        let mut scrollbar_state = ScrollbarState::new(input_viewport.total_lines)
            .position(input_viewport.scroll_top)
            .viewport_content_length(text_area.height as usize);

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }

    if !app.chat_enabled()
        || !app.input_focused
        || app.chat.sending
        || text_area.width <= 2
        || text_area.height == 0
    {
        return;
    }

    let prompt_prefix_width = input_prompt_prefix_width();
    let cursor_row = input_viewport
        .cursor_row
        .saturating_sub(input_viewport.scroll_top);
    if cursor_row < text_area.height as usize {
        frame.set_cursor_position((
            text_area.x
                + prompt_prefix_width
                + input_viewport
                    .cursor_col
                    .min(input_viewport.content_width.saturating_sub(1)),
            text_area.y + cursor_row as u16,
        ));
    }
}
pub(super) fn flush_static_scrollback(
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
