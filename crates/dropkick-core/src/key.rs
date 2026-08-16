//! Platform-independent key events.
//!
//! The Wayland adapter translates keysyms into these; the tools never see a
//! keysym, a keycode, or anything else that differs between Linux and Windows.
//! It is also what makes the interaction grammar unit-testable without a
//! compositor.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { alt: false, ctrl: false, shift: false };
    pub const ALT: Mods = Mods { alt: true, ctrl: false, shift: false };

    /// True when no modifier that changes a key's meaning is held. Shift is
    /// excluded deliberately: Shift+A is just `A`, which is why Shift could not
    /// be used for tab switching.
    pub fn is_plain(self) -> bool {
        !self.alt && !self.ctrl
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Mods,
}

impl KeyPress {
    pub fn plain(key: Key) -> Self {
        KeyPress { key, mods: Mods::NONE }
    }

    pub fn alt(c: char) -> Self {
        KeyPress { key: Key::Char(c), mods: Mods::ALT }
    }

    pub fn ch(c: char) -> Self {
        KeyPress::plain(Key::Char(c))
    }

    pub fn ctrl(c: char) -> Self {
        KeyPress { key: Key::Char(c), mods: Mods { ctrl: true, alt: false, shift: false } }
    }
}
