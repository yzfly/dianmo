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
}

pub trait TextSink {
    fn commit_text(&mut self, text: &str);
    fn send_key(&mut self, key: EditKey);
}
