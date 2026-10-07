//! 电脑键盘 (TODO #31, DESIGN.md §2「电脑键盘布局」): a touch PC keyboard whose keys are passed
//! through as real key presses. Nothing is composed by 点墨; the focused app's own IME (微信、
//! 豆包、搜狗、微软拼音) sees the keys exactly as from a physical keyboard.
//!
//! - A key sends `KeyDown` when pressed and `KeyUp` when released; held, it repeats `KeyDown`
//!   after [`REPEAT_DELAY_MS`] every [`PC_REPEAT_MS`] (typematic).
//! - Modifiers keep the rules of the other layouts (tap = next key, double tap = locked, held =
//!   chord) but are real keys here: a modifier goes down right before the first key it applies to
//!   and up when it no longer applies (one-shot: after that key; locked Ctrl / Alt / Win: when
//!   unlocked, like on the other layouts; held: when the finger lifts). A
//!   long-pressed Shift / Ctrl / Alt is really held down even without another key (Shift alone
//!   switches 中/英 in most Chinese IMEs; Ctrl + touching the app zooms).

use dianmo_core::{Action, KeyCode};

use crate::canvas::Rect;
use crate::keyboard::{KeyboardView, Mode, Panel, REPEAT_DELAY_MS, Target};
use crate::layout::{self, Key, KeyAction, Latch, Metrics, Modifier, Tone};
use crate::view::{Response, UiAction};

/// Modifiers that are real keys (Fn only changes our key faces).
const SENT: [(Modifier, KeyCode); 4] =
    [(Modifier::Ctrl, KeyCode::Ctrl), (Modifier::Shift, KeyCode::Shift), (Modifier::Alt, KeyCode::Alt), (Modifier::Win, KeyCode::Win)];

/// Modifiers whose lock really holds the key down (DESIGN.md §2「锁定 = 真按住」). Shift's lock is
/// caps lock for typing: held down it would also shift committed text and taps in the app.
pub(crate) const LOCK_HELD: [(Modifier, KeyCode); 3] =
    [(Modifier::Ctrl, KeyCode::Ctrl), (Modifier::Alt, KeyCode::Alt), (Modifier::Win, KeyCode::Win)];

impl KeyboardView {
    /// Turns the 电脑键盘 on or off. Returns true if it changed. Turning it off releases every
    /// key we hold down (the actions go into `r`).
    pub(crate) fn set_pc(&mut self, on: bool, r: &mut Response) -> bool {
        if self.pc == on {
            return false;
        }
        // Leaving: the keys we hold down; entering: modifiers held down for a lock.
        self.pc_release_all(r);
        self.pc = on;
        self.latch = [Latch::Off; 5];
        self.selecting = false;
        self.clip_bar = false;
        self.m = if on { Metrics::pc(self.m.w, self.m.h) } else { Metrics::new(self.m.w, self.m.h) };
        self.panel = Panel::Keys;
        self.rebuild();
        r.repaint = true;
        true
    }

    /// Switches the 电脑键盘 on or off from the host (tray menu, saved setting). Returns true if it
    /// changed; the caller repaints.
    pub fn set_pc_keyboard(&mut self, on: bool) -> bool {
        // The tray menu has the pointer, so no key of ours is held down at this point.
        self.set_pc(on, &mut Response::none())
    }

    pub fn pc_keyboard(&self) -> bool {
        self.pc
    }

    /// A pass-through key went down (its touch is already in `touches`).
    pub(crate) fn pc_key_down(&mut self, id: u32, code: KeyCode, now: u64, r: &mut Response) {
        if code == KeyCode::CapsLock {
            self.pc_caps = !self.pc_caps;
        }
        // Held modifiers now apply to a key: they are really pressed from here on.
        for t in &mut self.touches {
            if let (false, Target::Key(Key { action: KeyAction::Mod(m), .. })) = (t.consumed, &t.target) {
                t.used = true;
                t.long_at = None;
                if *m != Modifier::Fn {
                    t.engaged = true;
                }
            }
        }
        self.pc_sync(r);
        let a = Action::KeyDown(code);
        r.actions.push(UiAction::Input(a.clone()));
        if let Some(t) = self.touches.iter_mut().find(|t| t.id == id) {
            t.repeat_action = Some(a);
            t.repeat_at = Some(now + REPEAT_DELAY_MS);
        }
        if code == KeyCode::CapsLock {
            self.rebuild();
        }
    }

    /// A pass-through key was released (its touch is already removed): key up, then one-shot
    /// modifiers end and the modifiers that no longer apply go up.
    pub(crate) fn pc_key_up(&mut self, code: KeyCode, r: &mut Response) {
        r.actions.push(UiAction::Input(Action::KeyUp(code)));
        let mut changed = false;
        for l in &mut self.latch {
            if *l == Latch::Once {
                *l = Latch::Off;
                changed = true;
            }
        }
        self.pc_sync(r);
        if changed {
            self.rebuild();
        }
        r.repaint = true;
    }

    /// A finger holds modifier `m` down (engaged: it applied to a key or was long-pressed).
    fn finger_holds(&self, m: Modifier) -> bool {
        self.touches.iter().any(|t| {
            t.engaged && !t.consumed && matches!(&t.target, Target::Key(Key { action: KeyAction::Mod(tm), .. }) if *tm == m)
        })
    }

    /// Sends modifier downs / ups so that exactly the modifiers that apply are down: held ones
    /// that are engaged, latched ones while a pass-through key is pressed, and locked Ctrl / Alt /
    /// Win from their first key until unlocked.
    pub(crate) fn pc_sync(&mut self, r: &mut Response) {
        let key_down = self.touches.iter().any(|t| matches!(t.target, Target::Key(Key { action: KeyAction::Raw(_), .. })));
        for (m, code) in SENT {
            let i = m.index();
            let held = self.finger_holds(m);
            let kept = self.mods_down[i] && m != Modifier::Shift && self.latch[i] == Latch::Locked;
            let want = self.pc && (held || kept || key_down && self.latch[i] != Latch::Off);
            if want != self.mods_down[i] {
                self.mods_down[i] = want;
                r.actions.push(UiAction::Input(if want { Action::KeyDown(code) } else { Action::KeyUp(code) }));
            }
        }
    }

    /// Releases every pass-through key and modifier we hold down.
    pub(crate) fn pc_release_all(&mut self, r: &mut Response) {
        let mut ups = Vec::new();
        self.touches.retain(|t| match (&t.target, &t.mode) {
            (Target::Key(Key { action: KeyAction::Raw(code), .. }), Mode::Press) => {
                ups.push(*code);
                false
            }
            (Target::Key(Key { action: KeyAction::Raw(_) | KeyAction::Mod(_), .. }), _) => false,
            _ => true,
        });
        for code in ups {
            r.actions.push(UiAction::Input(Action::KeyUp(code)));
        }
        self.lift_mods(&mut r.actions);
    }

    /// Sends up every modifier we hold down (latches stay: a lock presses it again before the
    /// next key).
    pub(crate) fn lift_mods(&mut self, out: &mut Vec<UiAction>) {
        for (m, code) in SENT {
            if std::mem::take(&mut self.mods_down[m.index()]) {
                out.push(UiAction::Input(Action::KeyUp(code)));
            }
        }
    }

    /// For the host: sends up the modifiers held down for a lock (the lock stays lit and presses
    /// them again before the next key). Call it when the keyboard hides, when focus moves by
    /// touch, and before changing the input mode from outside (tray, settings), so no Ctrl / Alt
    /// / Win stays stuck. Modifiers a finger holds on the 电脑键盘, or that apply to a key being
    /// pressed, stay down.
    pub fn release_locked_mods(&mut self) -> Response {
        let mut r = Response::none();
        let key_down = self.touches.iter().any(|t| matches!(t.target, Target::Key(Key { action: KeyAction::Raw(_), .. })));
        for (m, code) in SENT {
            let i = m.index();
            if self.mods_down[i] && !self.finger_holds(m) && !(self.pc && key_down) {
                self.mods_down[i] = false;
                r.actions.push(UiAction::Input(Action::KeyUp(code)));
            }
        }
        self.lift_pending = false;
        r
    }

    /// The thin top bar of the 电脑键盘: 返回 on the left; voice and hide on the right.
    pub(crate) fn build_pc_bar(&self, right: Rect) -> Vec<Key> {
        let m = &self.m;
        let mut back = Key::icon(KeyAction::PcBack, layout::icon::BACK, Tone::Flat).with_sub("返回");
        back.cell = Rect::new(m.pad_x + 4.0, 0.0, m.bar_h * 3.2, m.bar_h);
        let w = m.bar_h * 1.4;
        let mut hide = Key::icon(KeyAction::Hide, layout::icon::CHEVRON_DOWN, Tone::Flat);
        hide.cell = Rect::new(right.x + right.w - w, 0.0, w, m.bar_h);
        let mut voice = Key::icon(KeyAction::Voice, layout::icon::MIC, Tone::Flat);
        voice.cell = Rect::new(hide.cell.x - w, 0.0, w, m.bar_h);
        vec![back, voice, hide]
    }
}
