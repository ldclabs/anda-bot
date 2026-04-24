use ratatui::style::{Color, Modifier, Style};

pub const BAMBOO_GREEN: Color = Color::Rgb(126, 186, 88);
pub const BAMBOO_LIGHT: Color = Color::Rgb(175, 224, 142);
pub const LEAF_MINT: Color = Color::Rgb(205, 238, 184);
pub const PANDA_WHITE: Color = Color::Rgb(236, 243, 229);
pub const BAMBOO_DIM: Color = Color::Rgb(120, 144, 104);
pub const PANDA_INK: Color = Color::Rgb(19, 24, 18);
pub const WARN_AMBER: Color = Color::Rgb(255, 208, 92);
pub const ERROR_RED: Color = Color::Rgb(255, 110, 110);

pub fn title_style() -> Style {
    Style::default()
        .fg(BAMBOO_LIGHT)
        .add_modifier(Modifier::BOLD)
}

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
        .fg(BAMBOO_LIGHT)
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
