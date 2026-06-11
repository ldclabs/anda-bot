use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::Style,
    text::Line,
    widgets::Widget,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme;

pub struct Banner {}

const BANNER_ART: [&str; 5] = [
    r#"      _     _   _   ____      _      "#,
    r#"     / \   | \ | | |  _ \    / \     "#,
    r#"    / _ \  |  \| | | | | |  / _ \    "#,
    r#"   / ___ \ | |\  | | |_| | / ___ \   "#,
    r#"  /_/   \_\|_| \_| |____/ /_/   \_\  "#,
];

impl Banner {
    pub fn height() -> u16 {
        BANNER_ART.len() as u16 + 1
    }
}

impl Widget for Banner {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let art_width = BANNER_ART
            .iter()
            .map(|line| line.width())
            .max()
            .unwrap_or(0) as u16;
        let art_x = area.x + area.width.saturating_sub(art_width) / 2;

        for (index, line) in BANNER_ART.iter().enumerate() {
            let y = area.y + index as u16;
            if y >= area.bottom() {
                return;
            }
            buf.set_stringn(
                art_x,
                y,
                *line,
                area.width as usize,
                theme::banner_line_style(index),
            );
        }

        let text_area = Rect {
            x: area.x,
            y: area.y + BANNER_ART.len() as u16,
            width: area.width,
            height: area.height.saturating_sub(BANNER_ART.len() as u16),
        };

        PackedLines::new(vec![])
            .alignment(Alignment::Center)
            .render(text_area, buf);
    }
}

/// A line-oriented text widget that pre-wraps content (we already do this
/// upstream via `wrap_visual`) and writes each grapheme to its own buffer
/// cell — the same shape the standard `Paragraph` widget produces, but
/// without the `LineComposer` machinery so we can opt out of `Wrap` and keep
/// the rendering deterministic.
///
/// Note on East-Asian Width and broken-font terminals: Unicode classifies CJK
/// glyphs as Wide (2 columns) and ratatui's diff emits an absolute `MoveTo`
/// to the start of every 2-column cell. On terminals/fonts that *paint* CJK
/// in only 1 column (e.g. Terminal.app + Sarasa Mono SC Nerd Font), this
/// leaves a visible gap. Earlier revisions of this widget tried to pack a
/// whole span into a single cell to make ratatui emit a single `Print`; that
/// triggered ratatui's `invalidated` clear logic and caused trailing
/// characters to be overwritten. The reliable fix is to use a font whose CJK
/// glyphs occupy the full 2 columns ratatui reserves (e.g. Sarasa Term SC
/// instead of the Mono Nerd Font variant).
pub struct PackedLines<'a> {
    lines: Vec<Line<'a>>,
    base_style: Style,
    alignment: Alignment,
}

impl<'a> PackedLines<'a> {
    pub fn new(lines: Vec<Line<'a>>) -> Self {
        Self {
            lines,
            base_style: Style::default(),
            alignment: Alignment::Left,
        }
    }

    pub fn style(mut self, style: Style) -> Self {
        self.base_style = style;
        self
    }

    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl Widget for PackedLines<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let max_width = area.width as usize;
        for (row_idx, line) in self.lines.iter().enumerate() {
            let row = row_idx as u16;
            if row >= area.height {
                break;
            }
            let y = area.y + row;
            let line_style = self.base_style.patch(line.style);

            let line_width: usize = line
                .spans
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
                .min(max_width);

            let offset = match self.alignment {
                Alignment::Left => 0,
                Alignment::Center => max_width.saturating_sub(line_width) / 2,
                Alignment::Right => max_width.saturating_sub(line_width),
            };

            let mut x: u16 = area.x + offset as u16;
            let max_x = area.x + area.width;

            for span in &line.spans {
                if x >= max_x {
                    break;
                }
                let style = line_style.patch(span.style);
                for grapheme in UnicodeSegmentation::graphemes(span.content.as_ref(), true) {
                    let gw = grapheme
                        .chars()
                        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                        .sum::<usize>();
                    if gw == 0 {
                        // Zero-width modifier — drop it; ratatui's Cell does
                        // not expose `append_symbol` publicly. The few code
                        // paths in this app that produce zero-width content
                        // (combining marks) tolerate the loss.
                        continue;
                    }
                    let gw_u16 = gw as u16;
                    if x + gw_u16 > max_x {
                        break;
                    }
                    buf[(x, y)].set_symbol(grapheme).set_style(style);
                    x = x.saturating_add(gw_u16);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{style::Color, text::Span};

    fn rendered_row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn banner_height_covers_art_plus_padding() {
        assert_eq!(Banner::height(), BANNER_ART.len() as u16 + 1);
    }

    #[test]
    fn banner_renders_centered_art() {
        let area = Rect::new(0, 0, 45, 8);
        let mut buf = Buffer::empty(area);

        Banner {}.render(area, &mut buf);

        let row = rendered_row(&buf, 4);
        assert!(row.contains(r#"/_/   \_\|_| \_| |____/ /_/   \_\"#));
    }

    #[test]
    fn banner_render_tolerates_zero_and_short_areas() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        Banner {}.render(Rect::new(0, 0, 0, 0), &mut buf);
        // Shorter than the art: rendering stops at the bottom edge.
        Banner {}.render(Rect::new(0, 0, 10, 2), &mut buf);
    }

    #[test]
    fn packed_lines_alignment_offsets_content() {
        let area = Rect::new(0, 0, 8, 3);
        let mut buf = Buffer::empty(area);

        PackedLines::new(vec![
            Line::from("ab"),
            Line::from("cd"),
            Line::from("ef"),
        ])
        .render(area, &mut buf);
        assert_eq!(rendered_row(&buf, 0), "ab      ");

        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![Line::from("ab")])
            .alignment(Alignment::Center)
            .render(area, &mut buf);
        assert_eq!(rendered_row(&buf, 0), "   ab   ");

        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![Line::from("ab")])
            .alignment(Alignment::Right)
            .render(area, &mut buf);
        assert_eq!(rendered_row(&buf, 0), "      ab");
    }

    #[test]
    fn packed_lines_truncates_wide_graphemes_at_edge() {
        let area = Rect::new(0, 0, 3, 1);
        let mut buf = Buffer::empty(area);

        PackedLines::new(vec![Line::from("中文")]).render(area, &mut buf);

        // "中" takes two columns; "文" would overflow the third column and is
        // dropped instead of being clipped mid-glyph.
        assert_eq!(buf[(0, 0)].symbol(), "中");
        assert_eq!(buf[(2, 0)].symbol(), " ");
    }

    #[test]
    fn packed_lines_skips_zero_width_graphemes() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buf = Buffer::empty(area);

        // A lone combining acute accent has zero display width.
        PackedLines::new(vec![Line::from(vec![
            Span::raw("\u{0301}"),
            Span::raw("ok"),
        ])])
        .render(area, &mut buf);

        assert_eq!(rendered_row(&buf, 0), "ok  ");
    }

    #[test]
    fn packed_lines_patches_span_styles_over_base() {
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);

        PackedLines::new(vec![Line::from(vec![
            Span::styled("a", Style::default().fg(Color::Red)),
            Span::raw("b"),
        ])])
        .style(Style::default().fg(Color::Blue))
        .render(area, &mut buf);

        assert_eq!(buf[(0, 0)].fg, Color::Red);
        assert_eq!(buf[(1, 0)].fg, Color::Blue);
    }

    #[test]
    fn packed_lines_stops_at_area_bounds() {
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);

        PackedLines::new(vec![Line::from("abcdef"), Line::from("never")])
            .render(area, &mut buf);

        assert_eq!(rendered_row(&buf, 0), "ab");

        // Zero-sized areas render nothing and must not panic.
        PackedLines::new(vec![Line::from("x")]).render(Rect::new(0, 0, 0, 0), &mut buf);
    }
}
