//! Input handling placeholder — keyboard/pointer events from the Wayland seat.
//! Will be wired into the layer surface once basic rendering is confirmed working.

#[allow(dead_code)]
pub enum InputEvent {
    KeyPress { key: String, mods: Modifiers },
    ScrollUp,
    ScrollDown,
    Click { x: f32, y: f32 },
    Close,
}

#[allow(dead_code)]
#[derive(Default)]
pub struct Modifiers {
    pub ctrl:   bool,
    pub shift:  bool,
    pub alt:    bool,
    pub super_: bool,
}
