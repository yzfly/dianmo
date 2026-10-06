//! Where committed text goes: the focused app (implemented in the Windows platform crate).

/// Editing keys forwarded to the focused app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditKey {
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// A physical key, for key combinations (DESIGN.md §2「电脑按键」).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// An ASCII letter (either case: the case does not add Shift), digit, space or US-layout
    /// punctuation key (`` ` - = [ ] \ ; ' , . / ``). Sinks ignore other characters.
    Char(char),
    Edit(EditKey),
    /// F1–F24.
    F(u8),
    Insert,
    PrintScreen,
    /// The modifier keys themselves (left-hand ones), for raw key events (pass-through layouts).
    Ctrl,
    Shift,
    Alt,
    Win,
    CapsLock,
    /// The context-menu (Apps) key.
    Menu,
}

/// Modifiers plus a key, sent as one unit: modifiers down, key down/up, modifiers up.
/// `key: None` taps the modifiers alone (a lone Win press opens Start).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    pub key: Option<KeyCode>,
}

impl KeyChord {
    pub const fn key(key: KeyCode) -> Self {
        Self { ctrl: false, shift: false, alt: false, win: false, key: Some(key) }
    }

    pub const fn ctrl(c: char) -> Self {
        Self { ctrl: true, ..Self::key(KeyCode::Char(c)) }
    }

    pub const fn ctrl_key(key: EditKey) -> Self {
        Self { ctrl: true, ..Self::key(KeyCode::Edit(key)) }
    }

    /// A lone Win press (Start menu).
    pub const fn win_alone() -> Self {
        Self { ctrl: false, shift: false, alt: false, win: true, key: None }
    }

    pub const UNDO: Self = Self::ctrl('z');
    pub const REDO: Self = Self::ctrl('y');
    pub const CUT: Self = Self::ctrl('x');
    pub const COPY: Self = Self::ctrl('c');
    pub const PASTE: Self = Self::ctrl('v');
    pub const SELECT_ALL: Self = Self::ctrl('a');
    /// Deletes the word before the caret.
    pub const DELETE_WORD: Self = Self::ctrl_key(EditKey::Backspace);

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.win
    }
}

pub trait TextSink {
    fn commit_text(&mut self, text: &str);
    fn send_key(&mut self, key: EditKey);
    /// Sends a key combination to the focused app in one go (no other input interleaves).
    fn send_chord(&mut self, chord: KeyChord);
    /// Presses (`down`) or releases one physical key, as a real keyboard would (virtual key +
    /// scan code). For pass-through layouts where the app's own IME handles the keys.
    fn key_event(&mut self, key: KeyCode, down: bool);
}
