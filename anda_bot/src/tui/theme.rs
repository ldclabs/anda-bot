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

pub fn badge_style() -> Style {
    Style::default()
        .fg(PANDA_INK)
        .bg(LEAF_MINT)
        .add_modifier(Modifier::BOLD)
}

pub fn panel_glow_style() -> Style {
    Style::default().fg(PANDA_WHITE)
}
