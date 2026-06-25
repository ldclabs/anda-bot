use std::borrow::Cow;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::{
    App, INPUT_CONTINUATION_PREFIX, INPUT_DIVIDER_FLOW_SPEED, INPUT_DIVIDER_GLOW_RADIUS,
    INPUT_DIVIDER_LABEL, INPUT_DIVIDER_PADDED_HEIGHT, INPUT_DIVIDER_PREFIX,
    INPUT_DIVIDER_TRAIL_OFFSET, INPUT_PROMPT_PREFIX, INPUT_SCROLLBAR_WIDTH, MAX_INPUT_LINES,
    THINKING_FRAMES, THINKING_LABEL,
    text::{char_display_width, display_width, truncate_visual, wrap_visual, wrapped_line_count},
    theme,
};

#[derive(Clone, Copy)]
pub(super) enum InputCursorDirection {
    Up,
    Down,
}

pub(super) fn input_newline_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT))
        || matches!(key.code, KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL)
}
pub(super) fn input_height(app: &App, area: Rect) -> u16 {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let inner_width = area.width.saturating_sub(input_prompt_prefix_width()) as usize;
    wrapped_line_count(&input_display_text(app), inner_width)
        .clamp(1, MAX_INPUT_LINES)
        .saturating_add(input_separator_block_height(area.height))
        .min(area.height)
}
pub(super) fn input_display_text(app: &App) -> String {
    if app.chat_enabled() && !app.input_buf.is_empty() {
        return app.input_buf.clone();
    }
    input_placeholder(app).to_string()
}

pub(super) fn input_placeholder(app: &App) -> &'static str {
    if app.setup_required() {
        "Edit config.yaml, save, then press Enter to enable chat."
    } else if !app.daemon_running {
        "Waiting for a healthy local daemon. Press Enter to retry."
    } else if app.choice_input.is_some() || app.chat.sending {
        ""
    } else if !app.input_focused {
        "Press Enter or start typing to focus the input."
    } else {
        ""
    }
}

pub(super) fn build_prompt_lines(app: &App, placeholder: &str, width: usize) -> Vec<Line<'static>> {
    let content_width = width
        .saturating_sub(input_prompt_prefix_width() as usize)
        .max(1);

    if !app.chat_enabled() || app.input_buf.is_empty() {
        if let Some(draft) = &app.choice_input
            && app.chat_enabled()
            && app.input_buf.is_empty()
        {
            return vec![Line::from(vec![
                Span::styled(INPUT_PROMPT_PREFIX.to_string(), theme::accent_style()),
                Span::styled(draft.placeholder(), theme::dim_style()),
            ])];
        }
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

pub(super) struct InputViewport {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) total_lines: usize,
    pub(super) scroll_top: usize,
    pub(super) cursor_col: u16,
    pub(super) cursor_row: usize,
    pub(super) content_width: u16,
}

pub(super) fn input_viewport(
    app: &App,
    placeholder: &str,
    prompt_area: Rect,
) -> (Rect, Option<Rect>, InputViewport) {
    let viewport = build_input_viewport(app, placeholder, prompt_area.width, prompt_area.height);
    if viewport.total_lines <= prompt_area.height as usize
        || prompt_area.width <= input_prompt_prefix_width() + INPUT_SCROLLBAR_WIDTH
    {
        return (prompt_area, None, viewport);
    }

    let text_area = Rect {
        x: prompt_area.x,
        y: prompt_area.y,
        width: prompt_area.width.saturating_sub(INPUT_SCROLLBAR_WIDTH),
        height: prompt_area.height,
    };
    let scrollbar_area = Rect {
        x: text_area.x + text_area.width,
        y: text_area.y,
        width: INPUT_SCROLLBAR_WIDTH,
        height: text_area.height,
    };
    let viewport = build_input_viewport(app, placeholder, text_area.width, text_area.height);

    (text_area, Some(scrollbar_area), viewport)
}

pub(super) fn build_input_viewport(
    app: &App,
    placeholder: &str,
    width: u16,
    height: u16,
) -> InputViewport {
    let content_width = width.saturating_sub(input_prompt_prefix_width()).max(1);
    let mut lines = build_prompt_lines(app, placeholder, width as usize);
    let (cursor_col, cursor_row) = if input_cursor_visible(app) {
        wrapped_cursor_position(&app.input_buf, app.input_cursor, content_width)
    } else {
        (0, 0)
    };
    let cursor_row = cursor_row as usize;

    if input_cursor_visible(app) {
        while cursor_row >= lines.len() {
            lines.push(Line::from(vec![Span::styled(
                INPUT_CONTINUATION_PREFIX.to_string(),
                theme::accent_style(),
            )]));
        }
    }

    let total_lines = lines.len().max(1);
    let scroll_top = input_scroll_top(cursor_row, height as usize, total_lines);
    let lines = lines
        .into_iter()
        .skip(scroll_top)
        .take(height as usize)
        .collect();

    InputViewport {
        lines,
        total_lines,
        scroll_top,
        cursor_col,
        cursor_row,
        content_width,
    }
}

fn input_cursor_visible(app: &App) -> bool {
    app.chat_enabled() && app.input_focused && !app.chat.sending
}

pub(super) fn input_scroll_top(
    cursor_row: usize,
    viewport_height: usize,
    total_lines: usize,
) -> usize {
    if viewport_height == 0 || total_lines <= viewport_height {
        return 0;
    }

    let max_scroll = total_lines - viewport_height;
    cursor_row
        .min(total_lines.saturating_sub(1))
        .saturating_add(1)
        .saturating_sub(viewport_height)
        .min(max_scroll)
}

pub(super) fn input_prompt_prefix_width() -> u16 {
    display_width(INPUT_PROMPT_PREFIX) as u16
}

pub(super) fn split_input_area(area: Rect) -> (Option<Rect>, Rect) {
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
pub(super) fn input_separator_block_height(area_height: u16) -> u16 {
    if area_height <= 1 {
        0
    } else if area_height > INPUT_DIVIDER_PADDED_HEIGHT {
        INPUT_DIVIDER_PADDED_HEIGHT
    } else {
        1
    }
}

pub(super) fn input_separator_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
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

pub(super) fn input_separator_line(app: &App, width: usize) -> Line<'static> {
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

pub(super) fn input_separator_label(app: &App) -> Cow<'static, str> {
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
struct InputCursorPoint {
    cursor: usize,
    row: usize,
    col: u16,
}

pub(super) fn move_cursor_vertically(
    text: &str,
    cursor_chars: usize,
    width: u16,
    direction: InputCursorDirection,
    preferred_col: Option<u16>,
) -> (usize, u16) {
    let width = width.max(1);
    let cursor_chars = cursor_chars.min(text.chars().count());
    let points = input_cursor_points(text, width);
    let (_, current_row) = wrapped_cursor_position_usize(text, cursor_chars, width);
    let current_col = input_cursor_col(text, cursor_chars, width);
    let desired_col = preferred_col.unwrap_or(current_col);
    let Some(last_row) = points.last().map(|point| point.row) else {
        return (cursor_chars, desired_col);
    };

    let target_row = match direction {
        InputCursorDirection::Up => current_row.saturating_sub(1),
        InputCursorDirection::Down => (current_row + 1).min(last_row),
    };

    if target_row == current_row {
        return (cursor_chars, desired_col);
    }

    let target_cursor = input_cursor_for_visual_position(&points, target_row, desired_col);
    (target_cursor, desired_col)
}

fn input_cursor_points(text: &str, width: u16) -> Vec<InputCursorPoint> {
    let width = width.max(1) as usize;
    let mut points = vec![InputCursorPoint {
        cursor: 0,
        row: 0,
        col: 0,
    }];
    let mut row = 0usize;
    let mut col = 0usize;

    for (idx, ch) in text.chars().enumerate() {
        if ch == '\n' {
            row += 1;
            col = 0;
            points.push(InputCursorPoint {
                cursor: idx + 1,
                row,
                col: 0,
            });
            continue;
        }

        let ch_width = char_display_width(ch);
        if col + ch_width > width && col != 0 {
            row += 1;
            col = 0;
        }
        col += ch_width;
        if col >= width {
            row += col / width;
            col %= width;
        }

        points.push(InputCursorPoint {
            cursor: idx + 1,
            row,
            col: col.min(u16::MAX as usize) as u16,
        });
    }

    points
}

fn input_cursor_for_visual_position(
    points: &[InputCursorPoint],
    target_row: usize,
    target_col: u16,
) -> usize {
    points
        .iter()
        .filter(|point| point.row == target_row)
        .min_by_key(|point| {
            (
                point.col.abs_diff(target_col),
                point.col > target_col,
                point.cursor,
            )
        })
        .map(|point| point.cursor)
        .unwrap_or_else(|| points.last().map(|point| point.cursor).unwrap_or_default())
}

fn input_cursor_col(text: &str, cursor_chars: usize, width: u16) -> u16 {
    wrapped_cursor_position_usize(text, cursor_chars, width).0
}

fn wrapped_cursor_position(text: &str, cursor_chars: usize, width: u16) -> (u16, u16) {
    let (col, row) = wrapped_cursor_position_usize(text, cursor_chars, width);
    (col, row.min(u16::MAX as usize) as u16)
}

fn wrapped_cursor_position_usize(text: &str, cursor_chars: usize, width: u16) -> (u16, usize) {
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

    (col.min(u16::MAX as usize) as u16, row)
}
