use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use super::theme;

pub struct InfoPanel<'a> {
    pub title: &'a str,
    pub lines: Vec<Line<'a>>,
}

impl Widget for InfoPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style())
            .title(Span::styled(
                format!(" {} ", self.title),
                theme::heading_style(),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        Paragraph::new(Text::from(self.lines))
            .wrap(Wrap { trim: false })
            .style(theme::body_style())
            .render(inner, buf);
    }
}

pub struct Banner<'a> {
    pub headline: &'a str,
    pub subtitle: &'a str,
}

const BANNER_ART: &str = r"
    _    _   _ ____    _
   / \  | \ | |  _ \  / \
  / _ \ |  \| | | | |/ _ \
 / ___ \| |\  | |_| / ___ \
/_/   \_\_| \_|____/_/   \_\
";

impl Widget for Banner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![Line::from("")];
        for line in BANNER_ART.lines() {
            if !line.is_empty() {
                lines.push(Line::from(Span::styled(line, theme::title_style())));
            }
        }
        lines.push(Line::from(Span::styled(
            self.headline,
            theme::accent_style(),
        )));
        lines.push(Line::from(Span::styled(self.subtitle, theme::dim_style())));

        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .render(area, buf);
    }
}
