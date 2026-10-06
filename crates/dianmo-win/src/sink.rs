//! Typing into the focused app with `SendInput` (DESIGN.md §3 输入架构).

use dianmo_core::{EditKey, TextSink};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_H, VK_HOME, VK_LEFT, VK_LWIN, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
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
    n as usize == events.len()
}

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
    }
}

/// Presses and releases a real virtual key (navigation keys carry `KEYEVENTF_EXTENDEDKEY`).
pub fn send_edit_key(key: EditKey) -> bool {
    let (vk, extended) = edit_vk(key);
    let mut events = Vec::with_capacity(2);
    tap(&mut events, vk, extended);
    send(&events)
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
    fn newline_is_enter() {
        let ev = text_events("a\r\n");
        assert_eq!(ev.len(), 4);
        assert_eq!(unsafe { ev[2].Anonymous.ki.wVk }, VK_RETURN);
    }
}
