use ratatui::style::{Color, Modifier, Style};

pub const FROST_BLUE: Color = Color::Rgb(102, 184, 255);
pub const FROST_CYAN: Color = Color::Rgb(138, 220, 255);
pub const FROST_WHITE: Color = Color::Rgb(226, 240, 255);
pub const FROST_DIM: Color = Color::Rgb(112, 136, 168);
pub const FROST_BG: Color = Color::Rgb(10, 18, 34);
pub const FROST_PANEL: Color = Color::Rgb(22, 34, 56);
pub const SUCCESS_GREEN: Color = Color::Rgb(92, 214, 126);
pub const WARN_AMBER: Color = Color::Rgb(255, 208, 92);
pub const ERROR_RED: Color = Color::Rgb(255, 110, 110);

pub fn title_style() -> Style {
    Style::default().fg(FROST_BLUE).add_modifier(Modifier::BOLD)
}

pub fn heading_style() -> Style {
    Style::default().fg(FROST_CYAN).add_modifier(Modifier::BOLD)
}

pub fn body_style() -> Style {
    Style::default().fg(FROST_WHITE)
}

pub fn dim_style() -> Style {
    Style::default().fg(FROST_DIM)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(FROST_WHITE)
        .bg(FROST_PANEL)
        .add_modifier(Modifier::BOLD)
}

pub fn accent_style() -> Style {
    Style::default().fg(FROST_BLUE).add_modifier(Modifier::BOLD)
}

pub fn success_style() -> Style {
    Style::default()
        .fg(SUCCESS_GREEN)
        .add_modifier(Modifier::BOLD)
}

pub fn warn_style() -> Style {
    Style::default().fg(WARN_AMBER).add_modifier(Modifier::BOLD)
}

pub fn danger_style() -> Style {
    Style::default().fg(ERROR_RED).add_modifier(Modifier::BOLD)
}

pub fn border_style() -> Style {
    Style::default().fg(FROST_BLUE).bg(FROST_BG)
}

pub fn input_style() -> Style {
    Style::default().fg(FROST_WHITE)
}
