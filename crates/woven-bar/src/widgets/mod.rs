pub mod activities;
pub mod workspaces;
pub mod window_title;
pub mod clock;
pub mod date;
pub mod cava;
pub mod network;
pub mod audio;
pub mod battery;
pub mod systray;
pub mod cpu;
pub mod memory;
pub mod disk;
pub mod temp;
pub mod media;
pub mod notifications;
pub mod control_center;
pub mod weather;

use tiny_skia::Pixmap;
use crate::config::Theme;
use crate::text::TextRenderer;
use crate::icons::IconCache;
use crate::sway::BarState as SwayState;

/// Context passed to every widget's render call.
pub struct RenderCtx<'a> {
    pub pixmap:  &'a mut Pixmap,
    pub text:    &'a mut TextRenderer,
    pub icons:   &'a mut IconCache,
    pub theme:   &'a Theme,
    pub state:   &'a SwayState,
    pub height:  u32,
}

/// A bar widget knows how wide it wants to be and how to draw itself.
pub trait Widget: Send {
    /// Preferred width in logical pixels. Return 0 to fill remaining space (center slot).
    /// Takes `&mut RenderCtx` because text measurement writes to the glyph cache.
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32;
    /// Draw into `ctx.pixmap` at the given x offset.
    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32);
    /// Called when the bar is clicked at (click_x, click_y) — optional.
    fn on_click(&mut self, _x: f64, _y: f64) {}
}
