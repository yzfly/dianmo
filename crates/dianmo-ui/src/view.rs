//! How the platform window drives the keyboard UI.
//!
//! The host (window) owns a `View`. It forwards pointer input and timer ticks, paints when asked,
//! and executes the returned [`UiAction`]s: `Input` goes to `dianmo_core::InputController`, after
//! which the host calls [`View::set_input_state`] with the controller's new state.

use dianmo_core::{Action, Schema, Snapshot};

use crate::canvas::Canvas;
use crate::theme::ThemeKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    /// The system took the pointer away (e.g. palm rejection); drop it without acting.
    Cancel,
}

/// One touch contact (or the mouse/pen). Several ids can be down at once (two thumbs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub id: u32,
    pub phase: PointerPhase,
    /// DIPs, window coordinates.
    pub x: f32,
    pub y: f32,
    /// Monotonic milliseconds.
    pub time_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    /// Feed to the input controller.
    Input(Action),
    /// Start Windows voice typing (Win+H).
    Voice,
    /// Hide the keyboard.
    Hide,
    /// The user expanded the candidate list: host fetches more via `Engine::candidates`
    /// and passes them to [`View::set_more_candidates`].
    WantMoreCandidates { start: usize, count: usize },
    /// T9: host fetches `Engine::t9_spellings` and passes them to [`View::set_t9_spellings`].
    WantT9Spellings,
    /// The user switched the colour theme from the layout menu (already applied to the view).
    /// The host only needs to remember it (settings).
    ThemeChanged(ThemeKind),
    /// The 电脑键盘 layout was turned on or off (already applied). The host remembers it.
    PcKeyboard(bool),
    /// Paste this clipboard entry (card tapped). Short text is typed (`Action::Text`); long text
    /// goes through the system clipboard and Ctrl+V.
    Paste(String),
    /// Pin or unpin a clipboard entry (pinned entries are kept on disk).
    PinClip { id: u64, pinned: bool },
    DeleteClip(u64),
    /// Remove every unpinned clipboard entry.
    ClearClips,
    /// A 复制 button was pressed (Ctrl+C sent). If the clipboard does not change shortly, nothing
    /// was selected: the host calls `KeyboardView::enter_select_mode`.
    CheckCopied,
}

/// One clipboard history entry, as the host passes it to the view (most recent first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipItem {
    pub id: u64,
    pub text: String,
    pub pinned: bool,
}

/// What the host should do after an event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    pub repaint: bool,
    pub actions: Vec<UiAction>,
    /// Call [`View::timer`] once after this many ms (long-press, key repeat, animations).
    /// A later response replaces an earlier pending request; `None` leaves it unchanged.
    pub timer_ms: Option<u64>,
}

impl Response {
    pub fn repaint() -> Self {
        Self { repaint: true, ..Self::default() }
    }

    pub fn none() -> Self {
        Self::default()
    }
}

/// Input state the UI renders (candidate bar, mode indicators).
#[derive(Clone, Debug, PartialEq)]
pub struct InputState {
    pub snapshot: Snapshot,
    pub chinese: bool,
    pub schema: Schema,
}

pub trait View {
    /// Window client size in DIPs. Called before the first paint and on every resize.
    fn resize(&mut self, width: f32, height: f32);
    /// Preferred height in DIPs for a given width (the host sizes the docked window with it).
    fn preferred_height(&self, width: f32) -> f32;
    fn paint(&mut self, canvas: &mut dyn Canvas);
    fn pointer(&mut self, event: PointerEvent) -> Response;
    fn timer(&mut self, now_ms: u64) -> Response;
    fn set_input_state(&mut self, state: InputState) -> Response;
    fn set_more_candidates(&mut self, start: usize, candidates: Vec<dianmo_core::Candidate>) -> Response;
    fn set_t9_spellings(&mut self, spellings: Vec<String>) -> Response;
    /// Lets the app reach the concrete view behind `&mut dyn View` (e.g. `KeyboardView::set_theme`
    /// from a tray command). Views that don't need it keep the default.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}
