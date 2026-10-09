mod action;
mod app;
mod backend;
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

use app::App;

pub use terminal::run;
