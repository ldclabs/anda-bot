use std::borrow::Cow;

use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn compact_cjk_spacing<'a>(text: &'a str) -> Cow<'a, str> {
    if !text.as_bytes().contains(&b' ') {
        return Cow::Borrowed(text);
    }

    let chars: Vec<char> = text.chars().collect();
    let mut compacted = String::with_capacity(text.len());
    let mut changed = false;

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch == ' '
            && should_compact_cjk_space(
                idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied(),
                chars.get(idx + 1).copied(),
            )
        {
            changed = true;
            continue;
        }

        compacted.push(ch);
    }

    if changed {
        Cow::Owned(compacted)
    } else {
        Cow::Borrowed(text)
    }
}

pub(super) fn compact_cjk_spacing_with_cursor(text: &str, cursor_chars: usize) -> (String, usize) {
    if !text.as_bytes().contains(&b' ') {
        return (text.to_string(), cursor_chars.min(text.chars().count()));
    }

    let chars: Vec<char> = text.chars().collect();
    let cursor_chars = cursor_chars.min(chars.len());
    let mut compacted = String::with_capacity(text.len());
    let mut normalized_cursor = 0;

    for (idx, ch) in chars.iter().copied().enumerate() {
        let keep = ch != ' '
            || !should_compact_cjk_space(
                idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied(),
                chars.get(idx + 1).copied(),
            );

        if keep {
            compacted.push(ch);
            if idx < cursor_chars {
                normalized_cursor += 1;
            }
        }
    }

    (compacted, normalized_cursor)
}

fn should_compact_cjk_space(prev: Option<char>, next: Option<char>) -> bool {
    matches!((prev, next), (Some(prev), Some(next)) if is_cjk_display_char(prev) && is_cjk_display_char(next))
}

pub(super) fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
}

fn is_cjk_display_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x31C0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE6F
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
    )
}

pub(super) fn wrapped_line_count(text: &str, width: usize) -> u16 {
    let width = width.max(1);
    let mut lines = 0u16;

    for line in text.split('\n') {
        lines = lines.saturating_add(wrap_visual(line, width).len() as u16);
    }

    lines.max(1)
}
pub(super) fn char_display_width(ch: char) -> usize {
    ch.width().unwrap_or(0)
}

pub(super) fn display_width(text: &str) -> usize {
    text.width()
}

pub(super) fn truncate_visual(text: &str, width: usize) -> String {
    let width = width.max(1);
    if display_width(text) <= width {
        return text.to_string();
    }

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

pub(super) fn wrap_visual(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let grapheme_width = display_width(grapheme);
        if current_width + grapheme_width > width && !current.is_empty() {
            wrapped.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push_str(grapheme);
        current_width += grapheme_width;
    }

    if !current.is_empty() || wrapped.is_empty() {
        wrapped.push(current);
    }

    wrapped
}
