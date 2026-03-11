pub mod backend;
pub mod hyprland;
pub mod niri;
pub mod sway;
pub mod registry;
pub mod xwayland;

pub use self::registry::detect_backend;
