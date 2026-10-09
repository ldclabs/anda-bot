use std::borrow::Cow;

use anda_core::{ContentPart, Message};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    action::{action_from_payload, action_transcript_text},
    markdown,
    text::{display_width, line_is_blank, normalize_newlines, truncate_visual},
    theme,
};

pub(super) const SECONDARY_PART_MAX_LINES: usize = 3;
/// Role of the entries the TUI writes itself (command output, action
/// receipts). They are informational, unlike `system` errors.
pub(super) const NOTICE_ROLE: &str = "notice";
const INDENT: &str = "        ";

pub(super) fn chat_message_lines_for_messages(
    messages: &[Message],
    width: usize,
) -> Vec<Line<'static>> {
    messages
        .iter()
        .flat_map(|msg| chat_message_lines_for_message(msg, width))
        .collect()
}

pub(super) fn chat_message_lines_for_message(msg: &Message, width: usize) -> Vec<Line<'static>> {
    let (marker, marker_style, body_style) = match msg.role.as_str() {
        "user" => ("❯ ", theme::accent_style(), theme::body_style()),
        "assistant" => ("🐼 ❯ ", theme::success_style(), theme::body_style()),
        NOTICE_ROLE => ("ℹ️ ❯ ", theme::accent_style(), theme::body_style()),
        "system" => ("⚠️ ❯ ", theme::danger_style(), theme::danger_style()),
        "tool" => ("🔧 ❯ ", theme::dim_style(), theme::dim_style()),
        _ => ("  ", theme::dim_style(), theme::body_style()),
    };
    let marker_width = display_width(marker);
    let mut out = MessageLines {
        lines: Vec::new(),
        marker,
        marker_style,
        indent: &INDENT[..marker_width.min(INDENT.len())],
        width: width.saturating_sub(marker_width).max(1),
    };

    // Text parts render as markdown; every other part is a dim excerpt, set
    // apart from text by a blank line.
    let mut prev_is_text = None;
    for part in &msg.content {
        let is_text = matches!(part, ContentPart::Text { .. });
        if prev_is_text.is_some_and(|prev| prev != is_text)
            && !out.lines.last().is_some_and(line_is_blank)
        {
            out.lines.push(Line::from(""));
        }
        match part {
            ContentPart::Text { text } => out.push_markdown(text, body_style),
            part => {
                let (text, max_lines) = secondary_part_text(part);
                out.push_limited(&text, max_lines);
            }
        }
        prev_is_text = Some(is_text);
    }

    if prev_is_text.is_some() {
        out.lines.push(Line::from(""));
    }
    out.lines
}

/// The excerpt shown for a non-text part and how many lines it may wrap to.
fn secondary_part_text(part: &ContentPart) -> (Cow<'_, str>, usize) {
    let text = match part {
        ContentPart::Text { text } => Cow::Borrowed(text.as_str()),
        ContentPart::Reasoning { text } => format!("thinking: {text}").into(),
        ContentPart::ToolCall { name, args, .. } => format!("→ {name}({args})").into(),
        ContentPart::ToolOutput { name, output, .. } => format!("← {name}: {output}").into(),
        ContentPart::FileData {
            file_uri,
            mime_type,
        } => format!("📎 [{}] {file_uri}", mime_type.as_deref().unwrap_or("file")).into(),
        ContentPart::InlineData { mime_type, .. } => format!("[inline {mime_type}]").into(),
        // Action cards stay complete: their details and choices are the
        // only way to answer them from the terminal.
        ContentPart::Action { name, payload, .. } => {
            let text = action_from_payload(name, payload)
                .map(|action| action_transcript_text(&action))
                .unwrap_or_else(|| format!("⚡ {name}"));
            return (text.into(), usize::MAX);
        }
        ContentPart::Any(json) => json.to_string().into(),
    };
    (text, SECONDARY_PART_MAX_LINES)
}

/// Wrapped lines of one message: the role marker leads the first line and
/// blank indentation of the same width leads the rest.
struct MessageLines {
    lines: Vec<Line<'static>>,
    marker: &'static str,
    marker_style: Style,
    indent: &'static str,
    width: usize,
}

impl MessageLines {
    fn push(&mut self, body: Vec<Span<'static>>) {
        let marker = if self.lines.is_empty() {
            Span::styled(self.marker, self.marker_style)
        } else {
            Span::styled(self.indent, theme::dim_style())
        };
        let mut spans = Vec::with_capacity(body.len() + 1);
        spans.push(marker);
        spans.extend(body);
        self.lines.push(Line::from(spans));
    }

    fn push_markdown(&mut self, text: &str, body_style: Style) {
        for line in markdown::render(text) {
            for body in wrap_styled_body_line(line, body_style, self.width) {
                self.push(body);
            }
        }
    }

    fn push_limited(&mut self, text: &str, max_lines: usize) {
        for chunk in limited_visual_lines(text, self.width, max_lines) {
            self.push(vec![Span::styled(chunk, theme::dim_style())]);
        }
    }
}

fn wrap_styled_body_line(
    line: Line<'static>,
    base_style: Style,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let line_style = base_style.patch(line.style);
    let mut wrapped = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for span in line.spans {
        let style = line_style.patch(span.style);
        for grapheme in UnicodeSegmentation::graphemes(span.content.as_ref(), true) {
            let expanded;
            let grapheme = if grapheme == "\t" {
                expanded = " ".repeat(4 - current_width % 4);
                expanded.as_str()
            } else {
                grapheme
            };
            if grapheme.chars().any(char::is_control) {
                continue;
            }

            let grapheme_width = display_width(grapheme);
            if grapheme_width == 0 {
                continue;
            }
            if current_width + grapheme_width > width && !current.is_empty() {
                wrapped.push(std::mem::take(&mut current));
                current_width = 0;
            }

            push_styled_grapheme(&mut current, grapheme, style);
            current_width += grapheme_width;
        }
    }

    if !current.is_empty() || wrapped.is_empty() {
        wrapped.push(current);
    }

    wrapped
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

fn limited_visual_lines(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(1);
    let max_lines = max_lines.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let mut truncated = false;
    let normalized = normalize_newlines(text);

    for grapheme in UnicodeSegmentation::graphemes(normalized.as_str(), true) {
        if grapheme == "\n" {
            if !push_limited_line(&mut lines, &mut current, max_lines) {
                truncated = true;
                break;
            }
            current_width = 0;
            continue;
        }

        let expanded;
        let grapheme = if grapheme == "\t" {
            expanded = " ".repeat(4 - current_width % 4);
            expanded.as_str()
        } else {
            grapheme
        };
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

    if truncated && let Some(last) = lines.last_mut() {
        *last = truncate_visual(&format!("{last}..."), width);
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
