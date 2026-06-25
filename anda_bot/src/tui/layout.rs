use ratatui::layout::Rect;

use super::{
    App, STATUS_FOOTER_MAX_LINES,
    input::{input_height, input_placeholder, input_viewport, split_input_area},
    status::{panel_lines, status_footer_lines},
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
pub(super) fn dynamic_viewport_height(app: &App, term_w: u16, term_h: u16) -> u16 {
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

pub(super) fn status_panel_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let lines = panel_lines(app).len() as u16;
    let desired = 1 + Banner::height() + lines;
    desired.min(area.height)
}

pub(super) fn static_panel_height(app: &App, width: u16) -> u16 {
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

pub(super) fn status_footer_height(app: &App, width: usize) -> u16 {
    status_footer_lines(app, width)
        .len()
        .clamp(1, STATUS_FOOTER_MAX_LINES)
        .saturating_add(1) as u16
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
