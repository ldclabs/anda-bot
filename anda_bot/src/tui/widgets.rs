use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::theme;

pub struct Banner<'a> {
    pub headline: &'a str,
    pub subtitle: &'a str,
}

const BANNER_ART: [&str; 5] = [
    r#"      _     _   _   ____      _      "#,
    r#"     / \   | \ | | |  _ \    / \     "#,
    r#"    / _ \  |  \| | | | | |  / _ \    "#,
    r#"   / ___ \ | |\  | | |_| | / ___ \   "#,
    r#"  /_/   \_\|_| \_| |____/ /_/   \_\  "#,
];

impl Banner<'_> {
    pub fn height() -> u16 {
        BANNER_ART.len() as u16 + 2
    }
}

impl Widget for Banner<'_> {
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

        Paragraph::new(vec![
            Line::from(Span::styled(self.headline, theme::success_style())),
            Line::from(Span::styled(self.subtitle, theme::subtle_style())),
        ])
        .alignment(Alignment::Center)
        .render(text_area, buf);
    }
}
