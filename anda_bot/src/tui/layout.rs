use ratatui::layout::Rect;

use super::{
    App,
    input::{input_height, input_placeholder, input_viewport, split_input_area},
    status::{STATUS_FOOTER_MAX_LINES, status_footer_lines},
    widgets::Banner,
};

pub(super) fn input_navigation_content_width(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 1;
    }

    let (input_height, _) = dynamic_layout_heights(app, area);
    let input_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: input_height,
    };
    let (_, prompt_area) = split_input_area(input_area);
    if prompt_area.width == 0 || prompt_area.height == 0 {
        return 1;
    }

    let placeholder = input_placeholder(app);
    let (_, _, viewport) = input_viewport(app, placeholder, prompt_area);
    viewport.content_width.max(1)
}

pub(super) fn dynamic_layout_heights(app: &App, area: Rect) -> (u16, u16) {
    layout_heights(
        app,
        area,
        status_footer_lines(app, area.width as usize).len(),
    )
}

/// Splits `area` between the composer and a status footer of `footer_lines`.
pub(super) fn layout_heights(app: &App, area: Rect, footer_lines: usize) -> (u16, u16) {
    if area.width == 0 || area.height == 0 {
        return (0, 0);
    }

    let input = input_height(app, area).min(area.height);
    let status = status_footer_height(footer_lines).min(area.height - input);
    (input, status)
}

/// Compute the inline viewport height for the dynamic bottom area only. The
/// static panel and messages are written above the viewport once via
/// `insert_before`, so they can naturally become shell scrollback.
pub(super) fn dynamic_viewport_height(app: &App, term_w: u16, term_h: u16) -> u16 {
    let (input, status) = dynamic_layout_heights(app, Rect::new(0, 0, term_w, term_h.max(1)));
    (input + status).max(1)
}

/// The banner plus its header line, written once into scrollback.
pub(super) fn static_panel_height() -> u16 {
    Banner::height() + 1
}

/// Footer lines plus the divider row above them.
pub(super) fn status_footer_height(footer_lines: usize) -> u16 {
    footer_lines.clamp(1, STATUS_FOOTER_MAX_LINES) as u16 + 1
}

pub(super) fn status_footer_panel(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    }
}

pub(super) fn centered_area(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width);
    let offset = area.width.saturating_sub(width) / 2;

    Rect {
        x: area.x + offset,
        y: area.y,
        width,
        height: area.height,
    }
}
