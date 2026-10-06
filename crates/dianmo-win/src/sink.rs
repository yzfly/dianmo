//! Typing into the focused app with `SendInput` (DESIGN.md §3 输入架构).

use std::sync::atomic::{AtomicU32, Ordering};

use dianmo_core::{EditKey, KeyChord, KeyCode, TextSink};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_F1, VK_H, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT,
    VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD,
    VK_OEM_PLUS, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SNAPSHOT, VK_SPACE, VK_APPS, VK_CAPITAL, VK_TAB, VK_UP,
};

/// Commits text and editing keys to whatever window has keyboard focus. The keyboard window never
/// takes focus, so that is the app the user is typing into.
///
/// Limits (by design of `SendInput`): elevated windows ignore it (UIPI), and apps that read raw
/// scan codes (games, remote desktop) don't see `KEYEVENTF_UNICODE` text.
#[derive(Debug, Default)]
pub struct SendInputSink;

impl SendInputSink {
    pub fn new() -> Self {
        Self
    }
}

impl TextSink for SendInputSink {
    fn commit_text(&mut self, text: &str) {
        send_text(text);
    }

    fn send_key(&mut self, key: EditKey) {
        send_edit_key(key);
    }

    fn send_chord(&mut self, chord: KeyChord) {
        send_chord(chord);
    }

    fn key_event(&mut self, key: KeyCode, down: bool) {
        send_key_event(key, down);
    }
}

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn vk_events(out: &mut Vec<INPUT>, vk: VIRTUAL_KEY, extended: bool, up: bool) {
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    out.push(key_input(vk, scan, flags));
}

fn tap(out: &mut Vec<INPUT>, vk: VIRTUAL_KEY, extended: bool) {
    vk_events(out, vk, extended, false);
    vk_events(out, vk, extended, true);
}

/// Builds the input events for `text`: each UTF-16 unit is a `KEYEVENTF_UNICODE` down+up (so a
/// surrogate pair is two of them); `\n` and `\t` become real Enter / Tab presses.
fn text_events(text: &str) -> Vec<INPUT> {
    let mut out = Vec::with_capacity(text.len() * 2);
    let mut buf = [0u16; 2];
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => tap(&mut out, VK_RETURN, false),
            '\t' => tap(&mut out, VK_TAB, false),
            _ => {
                for &unit in c.encode_utf16(&mut buf).iter() {
                    out.push(key_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                    out.push(key_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
    }
    out
}

fn send(events: &[INPUT]) -> bool {
    if events.is_empty() {
        return true;
    }
    let n = unsafe { SendInput(events, size_of::<INPUT>() as i32) };
    LAST_SEND_TICK.store(unsafe { GetTickCount() }.max(1), Ordering::Relaxed);
    n as usize == events.len()
}

/// `GetTickCount()` right after our last `SendInput` (0 = never), so the focus watcher can tell
/// our own key presses from touch input (`GetLastInputInfo` sees both).
pub(crate) static LAST_SEND_TICK: AtomicU32 = AtomicU32::new(0);

/// Types `text` into the focused window in a single `SendInput` call. Returns false if the
/// system blocked some of the events (e.g. the target is elevated).
pub fn send_text(text: &str) -> bool {
    send(&text_events(text))
}

fn edit_vk(key: EditKey) -> (VIRTUAL_KEY, bool) {
    match key {
        EditKey::Backspace => (VK_BACK, false),
        EditKey::Delete => (VK_DELETE, true),
        EditKey::Enter => (VK_RETURN, false),
        EditKey::Tab => (VK_TAB, false),
        EditKey::Escape => (VK_ESCAPE, false),
        EditKey::Left => (VK_LEFT, true),
        EditKey::Right => (VK_RIGHT, true),
        EditKey::Up => (VK_UP, true),
        EditKey::Down => (VK_DOWN, true),
        EditKey::Home => (VK_HOME, true),
        EditKey::End => (VK_END, true),
        EditKey::PageUp => (VK_PRIOR, true),
        EditKey::PageDown => (VK_NEXT, true),
    }
}

/// Presses and releases a real virtual key (navigation keys carry `KEYEVENTF_EXTENDEDKEY`).
pub fn send_edit_key(key: EditKey) -> bool {
    let (vk, extended) = edit_vk(key);
    let mut events = Vec::with_capacity(2);
    tap(&mut events, vk, extended);
    send(&events)
}

/// The virtual key for a chord key, and whether it needs `KEYEVENTF_EXTENDEDKEY`.
///
/// Letters and digits use their virtual-key codes ('A'..'Z' = 0x41.., '0'..'9' = 0x30..) and
/// punctuation the US-layout OEM codes, which is what Windows' Chinese layouts use too. Virtual
/// keys reach the app's keyboard handling directly, so an IME in the target app (微软拼音、搜狗)
/// does not compose them; it only reacts to its own hotkeys (Ctrl+Space, Ctrl+Shift, Ctrl+.).
fn chord_vk(key: KeyCode) -> Option<(VIRTUAL_KEY, bool)> {
    Some(match key {
        KeyCode::Char(c) => {
            let vk: u16 = match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => c.to_ascii_uppercase() as u16,
                ' ' => VK_SPACE.0,
                '-' => VK_OEM_MINUS.0,
                '=' => VK_OEM_PLUS.0,
                '[' => VK_OEM_4.0,
                ']' => VK_OEM_6.0,
                '\\' => VK_OEM_5.0,
                ';' => VK_OEM_1.0,
                '\'' => VK_OEM_7.0,
                ',' => VK_OEM_COMMA.0,
                '.' => VK_OEM_PERIOD.0,
                '/' => VK_OEM_2.0,
                '`' => VK_OEM_3.0,
                _ => return None,
            };
            (VIRTUAL_KEY(vk), false)
        }
        KeyCode::Edit(k) => edit_vk(k),
        KeyCode::F(n @ 1..=24) => (VIRTUAL_KEY(VK_F1.0 + n as u16 - 1), false),
        KeyCode::F(_) => return None,
        KeyCode::Insert => (VK_INSERT, true),
        KeyCode::PrintScreen => (VK_SNAPSHOT, true),
        KeyCode::Ctrl => (VK_LCONTROL, false),
        KeyCode::Shift => (VK_LSHIFT, false),
        KeyCode::Alt => (VK_LMENU, false),
        KeyCode::Win => (VK_LWIN, true),
        KeyCode::CapsLock => (VK_CAPITAL, false),
        KeyCode::Menu => (VK_APPS, true),
    })
}

/// Modifiers down (Ctrl, Shift, Alt, Win), the key down/up, modifiers up in reverse order.
/// Empty if the key cannot be sent; a chord without a key taps the modifiers alone.
fn chord_events(chord: KeyChord) -> Vec<INPUT> {
    let key = match chord.key {
        Some(k) => match chord_vk(k) {
            Some(vk) => Some(vk),
            None => return Vec::new(),
        },
        None => None,
    };
    let mods: Vec<(VIRTUAL_KEY, bool)> = [
        (chord.ctrl, (VK_LCONTROL, false)),
        (chord.shift, (VK_LSHIFT, false)),
        (chord.alt, (VK_LMENU, false)),
        (chord.win, (VK_LWIN, true)),
    ]
    .into_iter()
    .filter_map(|(on, m)| on.then_some(m))
    .collect();
    if key.is_none() && mods.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(2 * mods.len() + 2);
    for &(vk, ext) in &mods {
        vk_events(&mut out, vk, ext, false);
    }
    if let Some((vk, ext)) = key {
        tap(&mut out, vk, ext);
    }
    for &(vk, ext) in mods.iter().rev() {
        vk_events(&mut out, vk, ext, true);
    }
    out
}

/// Sends a key combination (Ctrl+C, Ctrl+Shift+T, Win+V, Alt+F4, a lone Win, ...) in one
/// `SendInput`, so no other input can interleave with the held modifiers.
pub fn send_chord(chord: KeyChord) -> bool {
    send(&chord_events(chord))
}

/// Presses or releases one key (virtual key + scan code from `MapVirtualKeyW`, extended keys
/// flagged), like a physical keyboard. False if the key can't be sent or the system blocked it.
pub fn send_key_event(key: KeyCode, down: bool) -> bool {
    let Some((vk, ext)) = chord_vk(key) else { return false };
    let mut out = Vec::with_capacity(1);
    vk_events(&mut out, vk, ext, !down);
    send(&out)
}

/// Starts Windows voice typing: Win↓ H↓ H↑ Win↑ in one `SendInput`. The keyboard never takes
/// focus, so dictated text lands in the focused app.
pub fn start_voice_typing() -> bool {
    let mut events = Vec::with_capacity(4);
    vk_events(&mut events, VK_LWIN, true, false);
    vk_events(&mut events, VK_H, false, false);
    vk_events(&mut events, VK_H, false, true);
    vk_events(&mut events, VK_LWIN, true, true);
    send(&events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrogate_pairs_are_two_unicode_events_each_way() {
        let ev = text_events("你😀");
        assert_eq!(ev.len(), 2 + 4);
        let scans: Vec<u16> = ev.iter().map(|e| unsafe { e.Anonymous.ki.wScan }).collect();
        assert_eq!(scans, [0x4F60, 0x4F60, 0xD83D, 0xD83D, 0xDE00, 0xDE00]);
    }

    #[test]
    fn chords_hold_modifiers_around_the_key() {
        let vks = |c| chord_events(c).iter().map(|e| unsafe { (e.Anonymous.ki.wVk, e.Anonymous.ki.dwFlags) }).collect::<Vec<_>>();
        let up = KEYEVENTF_KEYUP;
        let none = KEYBD_EVENT_FLAGS(0);
        let c = VIRTUAL_KEY(b'C' as u16);
        assert_eq!(vks(KeyChord::COPY), [(VK_LCONTROL, none), (c, none), (c, up), (VK_LCONTROL, up)]);
        let t = vks(KeyChord { shift: true, ..KeyChord::ctrl('t') });
        assert_eq!(t.len(), 6);
        assert_eq!(t[1], (VK_LSHIFT, none));
        assert_eq!(t[4], (VK_LSHIFT, up), "released in reverse order");
        assert_eq!(vks(KeyChord::ctrl('1'))[1].0, VIRTUAL_KEY(b'1' as u16));
        assert_eq!(vks(KeyChord::ctrl('-'))[1].0, VK_OEM_MINUS);
        assert_eq!(vks(KeyChord::ctrl_key(EditKey::Left))[1], (VK_LEFT, KEYEVENTF_EXTENDEDKEY));
        assert_eq!(vks(KeyChord::win_alone()), [(VK_LWIN, KEYEVENTF_EXTENDEDKEY), (VK_LWIN, KEYEVENTF_EXTENDEDKEY | up)]);
        assert_eq!(vks(KeyChord { alt: true, ..KeyChord::key(KeyCode::F(4)) })[1].0, VIRTUAL_KEY(VK_F1.0 + 3));
        assert!(chord_events(KeyChord::ctrl('，')).is_empty());
        assert!(chord_events(KeyChord::default()).is_empty());
        assert_eq!(chord_vk(KeyCode::Win), Some((VK_LWIN, true)));
        assert_eq!(chord_vk(KeyCode::Shift), Some((VK_LSHIFT, false)));
    }

    #[test]
    fn newline_is_enter() {
        let ev = text_events("a\r\n");
        assert_eq!(ev.len(), 4);
        assert_eq!(unsafe { ev[2].Anonymous.ki.wVk }, VK_RETURN);
    }
}
