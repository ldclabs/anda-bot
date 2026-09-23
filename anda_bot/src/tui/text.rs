use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
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
