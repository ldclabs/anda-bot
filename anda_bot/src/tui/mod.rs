mod action;
mod app;
mod input;
mod layout;
mod markdown;
mod render;
mod status;
mod terminal;
mod text;
mod theme;
mod transcript;
mod widgets;

#[cfg(test)]
mod tests;

use std::time::Duration;

use app::App;

const STATUS_MAX_WIDTH: u16 = 92;
const MAX_INPUT_LINES: u16 = 4;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const INPUT_PROMPT_PREFIX: &str = "❯ ";
const INPUT_CONTINUATION_PREFIX: &str = "  ";
const INPUT_DIVIDER_PREFIX: &str = "── ";
const INPUT_DIVIDER_LABEL: &str = "compose";
const THINKING_LABEL: &str = "thinking";
const INPUT_DIVIDER_PADDED_HEIGHT: u16 = 3;
const INPUT_DIVIDER_FLOW_SPEED: f32 = 1.25;
const INPUT_DIVIDER_GLOW_RADIUS: f32 = 15.0;
const INPUT_DIVIDER_TRAIL_OFFSET: f32 = 9.0;
const INPUT_SCROLLBAR_WIDTH: u16 = 1;
const STATUS_FOOTER_MAX_LINES: usize = 3;
const SECONDARY_PART_MAX_LINES: usize = 3;
const THINKING_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub use terminal::run;
