use ratatui::style::{Color, Modifier, Style};

// Brighter, more vibrant palette — still bamboo-inspired but with higher luma
// so terminals with dark backgrounds render text crisply.
pub const BAMBOO_GREEN: Color = Color::Rgb(150, 224, 104);
pub const BAMBOO_LIGHT: Color = Color::Rgb(198, 244, 170);
pub const LEAF_MINT: Color = Color::Rgb(226, 252, 208);
pub const PANDA_WHITE: Color = Color::Rgb(248, 253, 243);
pub const BAMBOO_DIM: Color = Color::Rgb(168, 192, 150);
pub const PANDA_INK: Color = Color::Rgb(14, 20, 12);
pub const WARN_AMBER: Color = Color::Rgb(255, 222, 120);
pub const ERROR_RED: Color = Color::Rgb(255, 132, 132);
pub const ACCENT_TEAL: Color = Color::Rgb(130, 232, 214);
pub const FOOTER_BG: Color = Color::Rgb(10, 28, 24);
pub const FOOTER_BORDER: Color = Color::Rgb(74, 150, 140);

#[allow(unused)]
pub fn title_style() -> Style {
    Style::default()
        .fg(BAMBOO_LIGHT)
        .add_modifier(Modifier::BOLD)
}

#[allow(unused)]
pub fn heading_style() -> Style {
    Style::default().fg(LEAF_MINT).add_modifier(Modifier::BOLD)
}

pub fn body_style() -> Style {
    Style::default().fg(PANDA_WHITE)
}

pub fn dim_style() -> Style {
    Style::default().fg(BAMBOO_DIM)
}

pub fn accent_style() -> Style {
    Style::default()
        .fg(ACCENT_TEAL)
        .add_modifier(Modifier::BOLD)
}

pub fn success_style() -> Style {
    Style::default()
        .fg(BAMBOO_GREEN)
        .add_modifier(Modifier::BOLD)
}

pub fn warn_style() -> Style {
    Style::default().fg(WARN_AMBER).add_modifier(Modifier::BOLD)
}

pub fn danger_style() -> Style {
    Style::default().fg(ERROR_RED).add_modifier(Modifier::BOLD)
}

pub fn subtle_style() -> Style {
    Style::default().fg(BAMBOO_DIM)
}

pub fn footer_panel_style() -> Style {
    Style::default().bg(FOOTER_BG)
}

pub fn footer_border_style() -> Style {
    Style::default().fg(FOOTER_BORDER).bg(FOOTER_BG)
}

pub fn footer_text_style() -> Style {
    Style::default().fg(BAMBOO_LIGHT).bg(FOOTER_BG)
}

pub fn banner_line_style(index: usize) -> Style {
    match index {
        0 => Style::default().fg(LEAF_MINT).add_modifier(Modifier::BOLD),
        1 | 2 => Style::default()
            .fg(BAMBOO_LIGHT)
            .add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(BAMBOO_GREEN)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(BAMBOO_DIM).add_modifier(Modifier::BOLD),
    }
}

#[allow(unused)]
pub fn badge_style() -> Style {
    Style::default()
        .fg(PANDA_INK)
        .bg(LEAF_MINT)
        .add_modifier(Modifier::BOLD)
}

pub fn panel_glow_style() -> Style {
    Style::default().fg(PANDA_WHITE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_use_palette_colors() {
        assert_eq!(title_style().fg, Some(BAMBOO_LIGHT));
        assert!(title_style().add_modifier.contains(Modifier::BOLD));
        assert_eq!(heading_style().fg, Some(LEAF_MINT));
        assert_eq!(body_style().fg, Some(PANDA_WHITE));
        assert_eq!(dim_style().fg, Some(BAMBOO_DIM));
        assert_eq!(accent_style().fg, Some(ACCENT_TEAL));
        assert_eq!(success_style().fg, Some(BAMBOO_GREEN));
        assert_eq!(warn_style().fg, Some(WARN_AMBER));
        assert_eq!(danger_style().fg, Some(ERROR_RED));
        assert_eq!(subtle_style().fg, Some(BAMBOO_DIM));
        assert_eq!(panel_glow_style().fg, Some(PANDA_WHITE));
    }

    #[test]
    fn footer_styles_share_footer_background() {
        assert_eq!(footer_panel_style().bg, Some(FOOTER_BG));
        assert_eq!(footer_border_style().fg, Some(FOOTER_BORDER));
        assert_eq!(footer_border_style().bg, Some(FOOTER_BG));
        assert_eq!(footer_text_style().fg, Some(BAMBOO_LIGHT));
        assert_eq!(footer_text_style().bg, Some(FOOTER_BG));
    }

    #[test]
    fn badge_style_inverts_ink_on_mint() {
        assert_eq!(badge_style().fg, Some(PANDA_INK));
        assert_eq!(badge_style().bg, Some(LEAF_MINT));
    }

    #[test]
    fn banner_line_style_varies_by_row() {
        assert_eq!(banner_line_style(0).fg, Some(LEAF_MINT));
        assert_eq!(banner_line_style(1).fg, Some(BAMBOO_LIGHT));
        assert_eq!(banner_line_style(2).fg, Some(BAMBOO_LIGHT));
        assert_eq!(banner_line_style(3).fg, Some(BAMBOO_GREEN));
        assert_eq!(banner_line_style(4).fg, Some(BAMBOO_DIM));
        assert_eq!(banner_line_style(99).fg, Some(BAMBOO_DIM));
    }
}
