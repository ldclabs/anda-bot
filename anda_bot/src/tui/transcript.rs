use anda_core::{ContentPart, Message};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    SECONDARY_PART_MAX_LINES,
    action::{action_from_payload, action_transcript_text},
    markdown,
    text::{compact_cjk_spacing, display_width, line_is_blank, normalize_newlines},
    theme,
};

#[cfg(test)]
use super::{App, input::input_separator_label};

#[cfg(test)]
pub(super) fn chat_message_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    chat_message_lines_for_messages(&app.chat.messages, width)
}

pub(super) fn chat_message_lines_for_messages(
    messages: &[Message],
    width: usize,
) -> Vec<Line<'static>> {
    let mut rendered_lines = Vec::new();
    for msg in messages {
        rendered_lines.extend(chat_message_lines_for_message(msg, width));
    }
    rendered_lines
}

pub(super) fn chat_message_lines_for_message(msg: &Message, width: usize) -> Vec<Line<'static>> {
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
                push_markdown_block(
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
            ContentPart::Action { name, payload, .. } => {
                let text = action_from_payload(name, payload)
                    .map(|action| action_transcript_text(&action))
                    .unwrap_or_else(|| format!("⚡ {name}"));
                push_limited_block(
                    &mut rendered_lines,
                    &mut first,
                    &text,
                    prefix,
                    &continuation_prefix,
                    prefix_style,
                    theme::dim_style(),
                    theme::dim_style(),
                    content_width,
                    action_part_max_lines(&text),
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

fn action_part_max_lines(text: &str) -> usize {
    text.lines().count().clamp(SECONDARY_PART_MAX_LINES, 12)
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
pub(super) fn thinking_lines(app: &App) -> Vec<Line<'static>> {
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

#[allow(clippy::too_many_arguments)]
fn push_markdown_block(
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
    for line in markdown::render(text) {
        for body_line in wrap_styled_body_line(line, body_style, content_width) {
            push_prefixed_body_line(
                rendered_lines,
                first,
                prefix,
                continuation_prefix,
                first_prefix_style,
                continuation_prefix_style,
                body_line,
            );
        }
    }
}

fn wrap_styled_body_line(
    line: Line<'static>,
    base_style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let line_style = base_style.patch(line.style);
    let alignment = line.alignment;
    let mut wrapped = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for span in line.spans {
        let style = line_style.patch(span.style);
        for grapheme in UnicodeSegmentation::graphemes(span.content.as_ref(), true) {
            if grapheme.chars().any(char::is_control) {
                continue;
            }

            let grapheme_width = display_width(grapheme);
            if grapheme_width == 0 {
                continue;
            }
            if current_width + grapheme_width > width && !current.is_empty() {
                wrapped.push(body_line_from_spans(
                    std::mem::take(&mut current),
                    alignment,
                ));
                current_width = 0;
            }

            push_styled_grapheme(&mut current, grapheme, style);
            current_width += grapheme_width;
        }
    }

    if !current.is_empty() || wrapped.is_empty() {
        wrapped.push(body_line_from_spans(current, alignment));
    }

    wrapped
}

fn body_line_from_spans(
    spans: Vec<Span<'static>>,
    alignment: Option<ratatui::layout::Alignment>,
) -> Line<'static> {
    let mut line = Line::from(spans);
    line.alignment = alignment;
    line
}

fn push_styled_grapheme(spans: &mut Vec<Span<'static>>, grapheme: &str, style: Style) {
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(grapheme);
        return;
    }

    spans.push(Span::styled(grapheme.to_string(), style));
}

#[allow(clippy::too_many_arguments)]
fn push_prefixed_body_line(
    rendered_lines: &mut Vec<Line<'static>>,
    first: &mut bool,
    prefix: &str,
    continuation_prefix: &str,
    first_prefix_style: Style,
    continuation_prefix_style: Style,
    body_line: Line<'static>,
) {
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

    let mut spans = Vec::with_capacity(body_line.spans.len() + 1);
    spans.push(Span::styled(marker, marker_style));
    spans.extend(body_line.spans);
    rendered_lines.push(Line::from(spans));
    *first = false;
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
