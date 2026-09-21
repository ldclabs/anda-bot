use std::io::{self, Write};

use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::{Cell, CellWidth},
    layout::{Position, Size},
};

/// Crossterm output with wide-cell handling for Ratatui's inline scrollback.
///
/// `Terminal::insert_before` can send every cell in a row directly to the
/// backend, bypassing `Buffer::diff`. Printing a reserved trailing cell then
/// overwrites half of the preceding wide glyph, which erases the whole glyph
/// on terminals such as Terminal.app. Keep the buffer's normal representation
/// and omit those covered cells at the output boundary instead.
pub(super) struct TuiBackend<W: Write> {
    inner: CrosstermBackend<W>,
}

impl<W: Write> TuiBackend<W> {
    pub(super) fn new(writer: W) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
        }
    }
}

impl<W: Write> Backend for TuiBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut covered: Option<(u16, std::ops::Range<u32>)> = None;
        self.inner.draw(content.filter(|(x, y, cell)| {
            if covered
                .as_ref()
                .is_some_and(|(row, columns)| row == y && columns.contains(&u32::from(*x)))
            {
                return false;
            }

            let width = cell.cell_width();
            // VS16 emoji have terminal-dependent width. Preserve Ratatui's
            // explicit trailing-cell updates for those sequences. Frame diffs
            // also clear stale wide-cell styles before repainting the leading
            // cell; the strictly following range below leaves that order intact.
            covered = (width > 1 && !cell.symbol().contains('\u{FE0F}'))
                .then(|| (*y, u32::from(*x) + 1..u32::from(*x) + u32::from(width)));
            true
        }))
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn save_cursor_position(&mut self) -> io::Result<bool> {
        self.inner.save_cursor_position()
    }

    fn restore_cursor_position(&mut self) -> io::Result<()> {
        self.inner.restore_cursor_position()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::cursor::MoveTo;
    use ratatui::{
        backend::Backend,
        buffer::{Buffer, Cell},
        layout::Rect,
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::Widget,
    };

    use crate::tui::widgets::PackedLines;

    fn draw<'a>(cells: impl Iterator<Item = (u16, u16, &'a Cell)>) -> String {
        let mut output = Vec::new();
        TuiBackend::new(&mut output).draw(cells).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn all_cells(buf: &Buffer) -> impl Iterator<Item = (u16, u16, &Cell)> {
        buf.content.iter().enumerate().map(|(i, cell)| {
            let width = usize::from(buf.area.width);
            (
                buf.area.x + (i % width) as u16,
                buf.area.y + (i / width) as u16,
                cell,
            )
        })
    }

    #[test]
    fn scrollback_keeps_chinese_and_mixed_text() {
        let area = Rect::new(0, 0, 24, 2);
        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![
            Line::from(vec![
                Span::styled("❯ ", Style::new().fg(Color::Cyan)),
                Span::styled("请分析 KIP v2", Style::new().fg(Color::White)),
            ]),
            Line::from("中文回复：你好！"),
        ])
        .render(area, &mut buf);

        // insert_before sends complete rows, including the reserved second
        // column of every CJK glyph. Check emitted terminal bytes: inspecting
        // Buffer symbols alone cannot detect a later space erasing a glyph.
        let output = draw(all_cells(&buf));
        assert!(output.contains("请分析 KIP v2"), "{output:?}");
        assert!(output.contains("中文回复：你好！"), "{output:?}");
        assert!(!output.contains(&MoveTo(3, 0).to_string()), "{output:?}");
    }

    #[test]
    fn wide_characters_at_row_end_preserve_the_next_row() {
        let area = Rect::new(0, 0, 6, 2);
        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![Line::from("abcd中"), Line::from("文ABCD")]).render(area, &mut buf);

        let output = draw(all_cells(&buf));
        assert!(output.contains("abcd中"), "{output:?}");
        assert!(output.contains("文ABCD"), "{output:?}");
        assert!(!output.contains(&MoveTo(5, 0).to_string()), "{output:?}");
        assert!(!output.contains(&MoveTo(1, 1).to_string()), "{output:?}");
        assert!(output.contains(&MoveTo(0, 1).to_string()), "{output:?}");
    }

    #[test]
    fn ascii_rows_and_padding_are_unchanged() {
        let area = Rect::new(0, 0, 16, 2);
        let mut buf = Buffer::empty(area);
        PackedLines::new(vec![Line::from("KIP v2  text"), Line::from("next row")])
            .render(area, &mut buf);

        let mut expected = Vec::new();
        CrosstermBackend::new(&mut expected)
            .draw(all_cells(&buf))
            .unwrap();
        assert_eq!(draw(all_cells(&buf)).as_bytes(), expected);
    }

    #[test]
    fn frame_diffs_preserve_style_clears_and_emoji() {
        for (before, after) in [("中文", "中文"), ("abcd", "❤️文"), ("中文", "abcd")] {
            let area = Rect::new(0, 0, 4, 1);
            let mut prev = Buffer::empty(area);
            prev.set_string(
                0,
                0,
                before,
                Style::new()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::UNDERLINED),
            );
            let mut next = Buffer::empty(area);
            next.set_string(0, 0, after, Style::default());

            // Ratatui deliberately emits trailing-cell clears BEFORE a
            // repainted wide glyph. The adapter must preserve their order.
            let mut expected = Vec::new();
            CrosstermBackend::new(&mut expected)
                .draw(prev.diff_iter(&next))
                .unwrap();
            assert_eq!(
                draw(prev.diff_iter(&next)).as_bytes(),
                expected,
                "{before:?} -> {after:?}"
            );
        }
    }
}
