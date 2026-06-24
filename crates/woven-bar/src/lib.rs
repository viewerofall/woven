pub mod config;
pub mod bar;

mod wayland;
mod draw;
mod text;
mod icons;
mod sway;
mod widgets;

pub use bar::run;
pub use config::BarConfig;
