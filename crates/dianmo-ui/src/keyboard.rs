//! [`KeyboardView`]: the phone-style keyboard (candidate bar, layouts, gestures).
//!
//! Gesture rules (per pointer, so two thumbs work independently):
//! - Character keys fire on release. If another finger presses a character key while one is
//!   still held, the held key fires first (keeps the order of fast two-thumb typing).
//! - Long-press (350 ms) or swipe-up on a key enters its secondary (shown in the top-right
//!   corner); keys with several alternates open a popup where sliding picks one.
//! - Backspace fires on press and repeats with acceleration; swipe left clears the composition.
//! - Dragging along the space bar moves the cursor; holding it turns the keyboard into a trackpad
//!   (drag = move the caret, a second finger's tap = start selecting).
//! - Modifiers (Shift, Ctrl, Alt, Win, Fn): tap = next key only, double tap = locked, held
//!   while another finger taps = a real chord. A lone Win tap presses Win; long-press latches it.
//! - Edit keys (arrows, Tab, Del) fire on press and repeat; keys with a hold action (Tab → Esc,
//!   toolbar ← → line start/end) fire on release instead.
//! - The candidate strip, the T9 column and the grids scroll by dragging, with a short fling.
//! - 电脑键盘 (`pc.rs`): every key is a real key press, down on press and up on release, repeating
//!   while held; held modifiers are really held.

use dianmo_core::{Action, Candidate, EditKey, KeyChord, KeyCode, Schema, Snapshot};

use crate::canvas::{Canvas, Rect, TextStyle};
use crate::clip::{extends_selection, horizontal};
use crate::layout::{
    self, BuildCtx, ColumnKind, Key, KeyAction, Latch, Layout, Metrics, Modifier, Mods, SelAct, SymTab, Tone,
};
use crate::scroll::{FRAME_MS, Scroller, VelocityTracker};
use crate::theme::{Theme, ThemeKind};
use crate::view::{ClipItem, InputState, PointerEvent, PointerPhase, Response, UiAction, View};

pub(crate) const LONG_PRESS_MS: u64 = 350;
/// Holding the space bar this long turns the keyboard into a trackpad.
pub(crate) const TRACKPAD_PRESS_MS: u64 = 450;
pub(crate) const REPEAT_DELAY_MS: u64 = 400;
/// 电脑键盘 auto-repeat interval (like a physical keyboard's typematic rate).
pub(crate) const PC_REPEAT_MS: u64 = 40;
/// How long the 「已复制」 toast stays.
pub(crate) const TOAST_MS: u64 = 1000;
const DOUBLE_TAP_MS: u64 = 350;
/// Candidates requested per `WantMoreCandidates`.
pub const MORE_BATCH: usize = 60;
/// Movement (DIPs) before a press on a scrollable area becomes a drag.
const SLOP: f32 = 8.0;
/// Movement (DIPs) before a press on the space bar becomes cursor movement.
const SPACE_SLOP: f32 = 12.0;

/// Construction options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyboardConfig {
    pub theme: ThemeKind,
    /// Layout shown before the first [`View::set_input_state`]. The host should start its
    /// `InputController` with the same schema (`Action::SetSchema`).
    pub schema: Schema,
    pub chinese: bool,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self { theme: ThemeKind::Light, schema: Schema::Pinyin, chinese: true }
    }
}

/// What occupies the area under the candidate bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Keys,
    Numbers,
    Symbols(SymTab),
    /// Expanded candidate grid.
    Candidates,
    /// Layout / theme picker.
    Menu,
    /// 电脑键: Esc, Tab, modifiers, arrows, F1–F12.
    PcKeys,
    /// Clipboard history, full-screen cards.
    Clipboard,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Key(Key),
    /// Toolbar icon or candidate chevron.
    Bar(Key),
    /// Candidate strip; the candidate under the finger.
    Strip(Option<usize>),
    Column(Option<usize>),
    /// Expanded candidate grid.
    Grid(Option<usize>),
    SymGrid(Option<usize>),
    /// Outside the menu tiles: closes the menu.
    CloseMenu,
    /// Clipboard card in the bar (index into `clips`).
    ClipCard(Option<usize>),
    /// Clipboard panel card (index into `clip_panel_order()`).
    ClipGrid(Option<usize>),
    /// 固定 / 删除 button of the panel card whose menu is open.
    ClipBtn { id: u64, delete: bool },
    /// Another finger while the keyboard is a trackpad: a tap starts selecting.
    PadAux,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mode {
    Press,
    /// Swiped up: release enters the secondary.
    SwipeUp,
    /// Long-press popup.
    /// `first`: pre-selected index; `anchor_x`: finger x when the popup opened (sliding one
    /// cell width moves the selection by one).
    Long { alts: Vec<String>, sel: usize, first: usize, anchor_x: f32, popup: Rect, cell_w: f32 },
    Scroll,
    /// Space-bar drag: `steps` cursor moves emitted so far (negative = left).
    Cursor { steps: i32 },
    /// Space held: the keyboard is a trackpad. Accumulated movement (DIPs, scaled by speed) not
    /// yet turned into caret steps, and the last finger sample.
    Trackpad { acc_x: f32, acc_y: f32, lx: f32, ly: f32, lt: u64 },
    /// Backspace swiped left: release clears the composition.
    ClearArmed,
}

#[derive(Clone, Debug)]
pub(crate) struct Touch {
    pub id: u32,
    pub target: Target,
    pub mode: Mode,
    pub x0: f32,
    pub y0: f32,
    pub x: f32,
    pub y: f32,
    /// Already fired (rollover) or otherwise spent: release does nothing.
    pub consumed: bool,
    pub long_at: Option<u64>,
    pub repeat_at: Option<u64>,
    pub repeats: u32,
    pub vel: VelocityTracker,
    /// The press stopped a fling: release must not select.
    pub stopped_fling: bool,
    /// A held modifier was used for a chord (or latched by a long press): release does not
    /// toggle it.
    pub used: bool,
    /// What a press-fired key (backspace, arrows) repeats.
    pub repeat_action: Option<Action>,
    /// 电脑键盘: this held modifier is really pressed (sent down): another key was pressed while
    /// it was held, or it was long-pressed (Shift / Ctrl / Alt).
    pub engaged: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CandLayout {
    /// (x, width) per candidate in strip content coordinates.
    pub strip: Vec<(f32, f32)>,
    /// Cell per candidate in grid content coordinates (origin at the grid's top-left).
    pub grid: Vec<Rect>,
    /// (candidate version, width, height, grid shown, measured with a real canvas).
    key: (u64, u32, u32, bool, bool),
}

pub struct KeyboardView {
    pub(crate) theme_kind: ThemeKind,
    pub(crate) theme: Theme,
    pub(crate) m: Metrics,
    pub(crate) chinese: bool,
    pub(crate) schema: Schema,
    pub(crate) snapshot: Snapshot,
    pub(crate) panel: Panel,
    /// Latched modifiers, indexed by [`Modifier::index`].
    pub(crate) latch: [Latch; 5],
    last_mod_tap: [Option<u64>; 5],
    /// Wide 26-key layout shows the edit area (user setting).
    pub(crate) edit_area: bool,
    pub(crate) keys: Vec<Key>,
    pub(crate) bar_keys: Vec<Key>,
    pub(crate) column: Option<(Rect, ColumnKind)>,
    pub(crate) grid: Option<Rect>,
    /// All loaded candidates: the snapshot's first batch plus `set_more_candidates`.
    pub(crate) cands: Vec<Candidate>,
    cand_ver: u64,
    more_pending: bool,
    more_done: bool,
    pub(crate) layout_cache: CandLayout,
    pub(crate) strip: Scroller,
    pub(crate) grid_scroll: Scroller,
    pub(crate) column_scroll: Scroller,
    pub(crate) sym_scroll: Scroller,
    pub(crate) spellings: Vec<String>,
    pub(crate) touches: Vec<Touch>,
    pub(crate) now: u64,
    /// Multiplier on the preferred height (user setting).
    height_scale: f32,
    /// 电脑键盘 (pass-through) layout.
    pub(crate) pc: bool,
    /// 电脑键盘: Caps Lock as toggled by our Caps key.
    pub(crate) pc_caps: bool,
    /// 电脑键盘: modifiers we sent down and not yet up (by [`Modifier::index`]; Fn is never sent).
    pub(crate) pc_down: [bool; 5],
    /// Selection mode: the selection bar replaces the toolbar; arrows extend the selection.
    pub(crate) selecting: bool,
    /// Trackpad: a second finger's tap started selecting (moves carry Shift).
    pub(crate) pad_select: bool,
    /// Trackpad: something was selected in this trackpad session.
    pub(crate) pad_selected: bool,
    /// Clipboard history from the host, most recent first.
    pub(crate) clips: Vec<ClipItem>,
    /// The idle bar shows the clipboard cards (after a copy) instead of the toolbar.
    pub(crate) clip_bar: bool,
    /// Preview of the system clipboard's text, shown small on the paste key.
    pub(crate) paste_preview: Option<String>,
    pub(crate) clip_strip: Scroller,
    pub(crate) clip_scroll: Scroller,
    /// Clipboard panel: the card showing its 固定 / 删除 buttons.
    pub(crate) clip_menu: Option<u64>,
    /// A short message (「已复制」) and when it goes away.
    pub(crate) toast: Option<(String, u64)>,
}

impl Default for KeyboardView {
    fn default() -> Self {
        Self::new(KeyboardConfig::default())
    }
}

impl KeyboardView {
    pub fn new(config: KeyboardConfig) -> Self {
        let w = 1440.0;
        let mut v = Self {
            theme_kind: config.theme,
            theme: Theme::of(config.theme),
            m: Metrics::new(w, layout::preferred_height(w)),
            chinese: config.chinese,
            schema: config.schema,
            snapshot: Snapshot::default(),
            panel: Panel::Keys,
            latch: [Latch::Off; 5],
            last_mod_tap: [None; 5],
            edit_area: true,
            keys: Vec::new(),
            bar_keys: Vec::new(),
            column: None,
            grid: None,
            cands: Vec::new(),
            cand_ver: 0,
            more_pending: false,
            more_done: false,
            layout_cache: CandLayout::default(),
            strip: Scroller::default(),
            grid_scroll: Scroller::default(),
            column_scroll: Scroller::default(),
            sym_scroll: Scroller::default(),
            spellings: Vec::new(),
            touches: Vec::new(),
            now: 0,
            height_scale: 1.0,
            pc: false,
            pc_caps: false,
            pc_down: [false; 5],
            selecting: false,
            pad_select: false,
            pad_selected: false,
            clips: Vec::new(),
            clip_bar: false,
            paste_preview: None,
            clip_strip: Scroller::default(),
            clip_scroll: Scroller::default(),
            clip_menu: None,
            toast: None,
        };
        v.rebuild();
        v
    }

    pub fn theme(&self) -> ThemeKind {
        self.theme_kind
    }

    /// Switches the colour theme. The caller repaints.
    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme_kind = kind;
        self.theme = Theme::of(kind);
        if self.panel == Panel::Menu {
            self.rebuild();
        }
    }

    /// Scales the preferred height (user setting, clamped to 0.7..=1.5). The host re-docks the
    /// window after changing it.
    pub fn set_height_scale(&mut self, scale: f32) {
        self.height_scale = if scale.is_finite() { scale.clamp(0.7, 1.5) } else { 1.0 };
    }

    pub fn height_scale(&self) -> f32 {
        self.height_scale
    }

    /// Shows or hides the edit area of the wide layout (user setting). Returns true if it
    /// changed; the caller repaints.
    pub fn set_edit_area(&mut self, on: bool) -> bool {
        let changed = self.edit_area != on;
        self.edit_area = on;
        if changed {
            self.rebuild();
        }
        changed
    }

    pub fn edit_area(&self) -> bool {
        self.edit_area
    }

    /// Whether the current size uses the wide (Surface landscape) layout.
    pub fn is_wide(&self) -> bool {
        self.m.wide
    }

    /// Opens the number pad (e.g. when a numeric field gets focus). Returns true if it changed.
    pub fn show_numbers(&mut self) -> bool {
        let changed = self.panel != Panel::Numbers;
        self.set_panel(Panel::Numbers);
        changed
    }

    /// Back to the letter keys from any panel (number pad, symbols, menu, candidate grid).
    /// Returns true if it changed.
    pub fn show_letters(&mut self) -> bool {
        let changed = self.panel != Panel::Keys;
        self.set_panel(Panel::Keys);
        changed
    }

    /// The letter layout currently shown (or that the keyboard returns to).
    pub fn layout(&self) -> Layout {
        if self.pc { Layout::Pc } else { Layout::of(self.chinese, self.schema) }
    }

    /// Centre of a visible key, for automated GUI tests. `name` is a key label ("a", "，",
    /// "符号", "123", "F5") or one of the names below (`clip0`, `clip1` … are clipboard cards in
    /// the bar, or in the clipboard panel when it is open).
    pub fn key_center(&self, name: &str) -> Option<(f32, f32)> {
        if let Some(i) = name.strip_prefix("clip").and_then(|n| n.parse::<usize>().ok()) {
            return self.clip_card_center(i);
        }
        let edit = |e: EditKey| move |k: &Key| k.action == KeyAction::Edit(e) || k.action == KeyAction::Raw(KeyCode::Edit(e));
        let chord = |c: KeyChord| move |k: &Key| k.action == KeyAction::Chord(c);
        let press = |e: EditKey, ctrl: bool| {
            move |k: &Key| k.action == KeyAction::Press(KeyChord { ctrl, shift: true, ..KeyChord::key(KeyCode::Edit(e)) })
        };
        let named = |k: &Key| match name {
            "shift" => k.action == KeyAction::Mod(Modifier::Shift),
            "ctrl" => k.action == KeyAction::Mod(Modifier::Ctrl),
            "alt" => k.action == KeyAction::Mod(Modifier::Alt),
            "win" => k.action == KeyAction::Mod(Modifier::Win),
            "fn" => k.action == KeyAction::Mod(Modifier::Fn),
            "caps" => k.action == KeyAction::CapsLock || k.action == KeyAction::Raw(KeyCode::CapsLock),
            "esc" => edit(EditKey::Escape)(k),
            "tab" => edit(EditKey::Tab)(k),
            "del" => edit(EditKey::Delete)(k),
            "left" => edit(EditKey::Left)(k),
            "right" => edit(EditKey::Right)(k),
            "up" => edit(EditKey::Up)(k),
            "down" => edit(EditKey::Down)(k),
            "home" => edit(EditKey::Home)(k),
            "end" => edit(EditKey::End)(k),
            "undo" => chord(KeyChord::UNDO)(k),
            "redo" => chord(KeyChord::REDO)(k),
            "selectall" => chord(KeyChord::SELECT_ALL)(k),
            "copy" => chord(KeyChord::COPY)(k),
            "paste" => chord(KeyChord::PASTE)(k),
            "cut" => chord(KeyChord::CUT)(k),
            "delword" => chord(KeyChord::DELETE_WORD)(k),
            "clear" => k.action == KeyAction::ClearAll,
            "pc" => k.action == KeyAction::PcKeys,
            "pageup" => k.action == KeyAction::PageUp,
            "pagedown" => k.action == KeyAction::PageDown,
            "backspace" => k.action == KeyAction::Backspace || k.action == KeyAction::Raw(KeyCode::Edit(EditKey::Backspace)),
            "enter" => k.action == KeyAction::Enter || k.action == KeyAction::Raw(KeyCode::Edit(EditKey::Enter)),
            "space" => k.action == KeyAction::Space || k.action == KeyAction::Raw(KeyCode::Char(' ')),
            "layout" => k.action == KeyAction::LayoutMenu,
            "toggle" => k.action == KeyAction::ToggleChinese,
            "expand" => k.action == KeyAction::ExpandCandidates,
            "voice" => k.action == KeyAction::Voice,
            "hide" => k.action == KeyAction::Hide,
            "symbols" => k.action == KeyAction::Symbols,
            "numbers" => k.action == KeyAction::Numbers,
            "back" => k.action == KeyAction::Back,
            "pcmode" => k.action == KeyAction::SetLayout(Layout::Pc),
            "pcback" => k.action == KeyAction::PcBack,
            "prtsc" => k.action == KeyAction::Raw(KeyCode::PrintScreen),
            "select" => k.action == KeyAction::SelectMode,
            "clipboard" => k.action == KeyAction::ClipPanel,
            "clipclose" => k.action == KeyAction::CloseClipBar,
            "clearclips" => k.action == KeyAction::ClearClips,
            "sel_left" => press(EditKey::Left, false)(k),
            "sel_right" => press(EditKey::Right, false)(k),
            "sel_wordleft" => press(EditKey::Left, true)(k),
            "sel_wordright" => press(EditKey::Right, true)(k),
            "sel_up" => press(EditKey::Up, false)(k),
            "sel_down" => press(EditKey::Down, false)(k),
            "sel_home" => press(EditKey::Home, false)(k),
            "sel_end" => press(EditKey::End, false)(k),
            "sel_copy" => k.action == KeyAction::Sel(SelAct::Copy),
            "sel_cut" => k.action == KeyAction::Sel(SelAct::Cut),
            "sel_paste" => k.action == KeyAction::Sel(SelAct::Paste),
            "sel_delete" => k.action == KeyAction::Sel(SelAct::Delete),
            "sel_done" => k.action == KeyAction::Sel(SelAct::Done),
            _ => !k.icon && (k.label == name || k.label.eq_ignore_ascii_case(name) && k.label.len() == 1),
        };
        self.keys.iter().chain(&self.bar_keys).find(|k| named(k)).map(|k| (k.cell.x + k.cell.w / 2.0, k.cell.y + k.cell.h / 2.0))
    }

    pub(crate) fn composing(&self) -> bool {
        self.snapshot.is_composing()
    }

    /// Latched modifiers plus the ones a finger is holding down.
    pub(crate) fn mods(&self) -> Mods {
        let mut held = [false; 5];
        for t in &self.touches {
            if let (false, Target::Key(Key { action: KeyAction::Mod(m), .. })) = (t.consumed, &t.target) {
                held[m.index()] = true;
            }
        }
        Mods { latch: self.latch, held }
    }

    /// `key` with the active modifiers.
    fn chord(&self, key: KeyCode) -> KeyChord {
        let m = self.mods();
        KeyChord {
            ctrl: m.on(Modifier::Ctrl),
            shift: m.on(Modifier::Shift),
            alt: m.on(Modifier::Alt),
            win: m.on(Modifier::Win),
            key: Some(key),
        }
    }

    /// After a key was sent: one-shot modifiers turn off; held ones count as used.
    fn after_key(&mut self) {
        let mut changed = false;
        for l in &mut self.latch {
            if *l == Latch::Once {
                *l = Latch::Off;
                changed = true;
            }
        }
        for t in &mut self.touches {
            if let Target::Key(Key { action: KeyAction::Mod(_), .. }) = &t.target {
                t.used = true;
                t.long_at = None;
            }
        }
        if changed {
            self.rebuild();
        }
    }

    /// What a press-fired key (backspace, arrows, Del) sends.
    fn press_input(&self, k: &Key) -> Action {
        let m = self.mods();
        match k.action {
            KeyAction::Press(c) => Action::Key(c),
            KeyAction::Edit(e) if self.selecting && extends_selection(e) => {
                Action::Key(KeyChord { shift: true, ..self.chord(KeyCode::Edit(e)) })
            }
            KeyAction::Edit(e) if m.chording() || m.on(Modifier::Shift) => Action::Key(self.chord(KeyCode::Edit(e))),
            KeyAction::Edit(e) => Action::Edit(e),
            _ if m.chording() => Action::Key(self.chord(KeyCode::Edit(EditKey::Backspace))),
            _ => Action::Backspace,
        }
    }

    /// A modifier key was released after a tap (not used for a chord).
    fn mod_tapped(&mut self, m: Modifier, r: &mut Response) {
        let i = m.index();
        let others = Modifier::ALL.iter().any(|&o| o != m && self.latch[o.index()] != Latch::Off);
        if m == Modifier::Win && self.latch[i] == Latch::Off && !others {
            // A lone Win tap presses Win (Start menu); long-press latches it instead.
            r.actions.push(UiAction::Input(Action::Key(KeyChord::win_alone())));
            return;
        }
        let quick = self.last_mod_tap[i].is_some_and(|t| self.now.saturating_sub(t) <= DOUBLE_TAP_MS);
        self.latch[i] = match self.latch[i] {
            Latch::Off => Latch::Once,
            Latch::Once if quick => Latch::Locked,
            Latch::Once | Latch::Locked => Latch::Off,
        };
        self.last_mod_tap[i] = Some(self.now);
    }

    // -----------------------------------------------------------------------------------------
    // Building
    // -----------------------------------------------------------------------------------------

    pub(crate) fn rebuild(&mut self) {
        let ctx = BuildCtx {
            layout: self.layout(),
            chinese: self.chinese,
            composing: self.composing(),
            mods: self.mods(),
            edit_area: self.edit_area,
            caps: self.pc_caps,
        };
        let built = match self.panel {
            Panel::Keys if self.pc => layout::build_pc_keyboard(&self.m, &ctx),
            Panel::Keys if ctx.layout == Layout::T9 => layout::build_t9(&self.m, &ctx),
            Panel::Keys => layout::build_letters(&self.m, &ctx),
            Panel::Numbers => layout::build_numbers(&self.m, &ctx),
            Panel::Symbols(tab) => layout::build_symbols(&self.m, tab),
            Panel::Candidates => layout::build_candidate_grid(&self.m),
            Panel::Menu => layout::build_menu(&self.m, ctx.layout, self.theme_kind == ThemeKind::Dark),
            Panel::PcKeys => layout::build_pc_keys(&self.m, &ctx),
            Panel::Clipboard => layout::build_clipboard_panel(&self.m),
        };
        self.keys = built.keys;
        for k in &mut self.keys {
            match k.action {
                KeyAction::SelectMode if self.selecting => k.tone = Tone::Active,
                // The paste key previews what it will paste.
                KeyAction::Chord(c) if c == KeyChord::PASTE && !k.icon => k.sub = self.paste_preview.clone(),
                _ => {}
            }
        }
        self.column = built.column;
        self.grid = built.grid;
        self.bar_keys = self.build_bar();
        if let Some((rect, _)) = self.column {
            let n = self.column_items().len() as f32;
            self.column_scroll.set_max(n * self.column_item_h() - rect.h);
        }
        if let (Panel::Symbols(tab), Some(grid)) = (self.panel, self.grid) {
            let (cols, cell_h) = self.sym_cell(grid);
            let rows = layout::symbols(tab).len().div_ceil(cols) as f32;
            self.sym_scroll.set_max(rows * cell_h - grid.h);
        }
        self.update_clip_scroll();
    }

    fn build_bar(&self) -> Vec<Key> {
        let m = &self.m;
        let cw = self.chevron_w();
        let right = Rect::new(m.w - cw - m.pad_x, 0.0, cw, m.bar_h);
        if self.pc {
            return self.build_pc_bar(right);
        }
        if self.panel == Panel::Candidates {
            let mut k = Key::icon(KeyAction::CollapseCandidates, layout::icon::CHEVRON_UP, Tone::Flat);
            k.cell = right;
            return vec![k];
        }
        if self.composing() {
            let mut k = Key::icon(KeyAction::ExpandCandidates, layout::icon::CHEVRON_DOWN, Tone::Flat);
            k.cell = right;
            return vec![k];
        }
        if self.selecting {
            return self.build_select_bar();
        }
        if self.clip_bar_shown() {
            return self.build_clip_bar(right);
        }
        self.build_toolbar(right)
    }

    /// The idle toolbar. Edit tools (undo … paste, ← →) on the left unless the wide edit area
    /// already has them; voice, layout, 电脑键, emoji next; hide on the far right.
    fn build_toolbar(&self, right: Rect) -> Vec<Key> {
        let m = &self.m;
        let wide26 = m.wide && self.layout() != Layout::T9;
        let icon_w = m.bar_h * 1.2;
        let tool_w = m.bar_h * 1.6;
        let arrow_w = m.bar_h * 1.1;
        let flat = |action: KeyAction, glyph: &str| Key::icon(action, glyph, Tone::Flat);
        let mut apps = vec![flat(KeyAction::Voice, layout::icon::MIC), flat(KeyAction::LayoutMenu, layout::icon::KEYBOARD)];
        if !wide26 {
            apps.push(flat(KeyAction::PcKeys, layout::icon::KEYBOARD).with_sub("电脑键"));
        }
        apps.push(flat(KeyAction::ClipPanel, layout::icon::CLIPBOARD));
        apps.push(flat(KeyAction::Tab(SymTab::Emoji), layout::icon::EMOJI));
        let app_w = |k: &Key| if k.sub.is_some() { tool_w * 1.15 } else { icon_w };
        let apps_w: f32 = apps.iter().map(app_w).sum();
        let mut tools = Vec::new();
        if !(wide26 && self.edit_area) {
            let t = |c: KeyChord, glyph: &str, cap: &str| flat(KeyAction::Chord(c), glyph).with_sub(cap);
            tools = vec![
                (tool_w, t(KeyChord::UNDO, layout::icon::UNDO, "撤销")),
                (tool_w, t(KeyChord::REDO, layout::icon::REDO, "重做")),
                (tool_w, flat(KeyAction::SelectMode, layout::icon::SELECT).with_sub("选择")),
                (tool_w, t(KeyChord::SELECT_ALL, layout::icon::SELECT_ALL, "全选")),
                (tool_w, t(KeyChord::CUT, layout::icon::CUT, "剪切")),
                (tool_w, t(KeyChord::COPY, layout::icon::COPY, "复制")),
                (tool_w, t(KeyChord::PASTE, layout::icon::PASTE, "粘贴")),
            ];
            if !wide26 {
                let arrow = |e: EditKey, glyph: &str, hold: EditKey| flat(KeyAction::Edit(e), glyph).with_hold(Action::Edit(hold), "");
                tools.push((arrow_w, arrow(EditKey::Left, layout::icon::CHEVRON_LEFT, EditKey::Home)));
                tools.push((arrow_w, arrow(EditKey::Right, layout::icon::CHEVRON_RIGHT, EditKey::End)));
            }
            // Drop what does not fit (narrow windows): arrows first, then the tools.
            let room = right.x - m.pad_x - apps_w - 16.0 * m.s;
            while !tools.is_empty() && tools.iter().map(|(w, _)| w).sum::<f32>() > room {
                tools.pop();
            }
        }
        let mut keys = Vec::new();
        let mut x = m.pad_x + 6.0 * m.s;
        for (w, mut k) in tools.iter().cloned() {
            k.cell = Rect::new(x, 0.0, w, m.bar_h);
            x += w;
            keys.push(k);
        }
        // Apps go right-aligned before the hide chevron when there are tools, else left.
        let mut ax = if tools.is_empty() { m.pad_x + 6.0 * m.s } else { right.x - apps_w };
        for mut k in apps {
            let w = app_w(&k);
            k.cell = Rect::new(ax, 0.0, w, m.bar_h);
            ax += w;
            keys.push(k);
        }
        let mut hide = Key::icon(KeyAction::Hide, layout::icon::CHEVRON_DOWN, Tone::Flat);
        hide.cell = right;
        keys.push(hide);
        keys
    }

    pub(crate) fn chevron_w(&self) -> f32 {
        self.m.bar_h * 1.15
    }

    /// Candidate strip area (left of the chevron).
    pub(crate) fn strip_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.m.w - self.chevron_w() - self.m.pad_x, self.m.bar_h)
    }

    /// Left padding of the first candidate inside the strip.
    pub(crate) fn strip_lead(&self) -> f32 {
        self.m.pad_x + 8.0 * self.m.s
    }

    /// Font scale of the candidate bar: wide bars use fixed sizes (24-DIP candidates at the
    /// 58-DIP bar of a 1440-wide screen), phone bars follow the key rows.
    pub(crate) fn bar_s(&self) -> f32 {
        if self.pc {
            // The 电脑键盘's thin bar: text as big as on the keys.
            self.m.s
        } else if self.m.wide {
            self.m.bar_h / 57.6
        } else {
            self.m.s
        }
    }

    pub(crate) fn cand_style(&self) -> TextStyle {
        let size = if self.m.wide { 24.0 * self.bar_s() } else { 21.0 * self.m.s };
        TextStyle { size, color: self.theme.text, align: crate::Align::Start, bold: false, font: crate::Font::Ui }
    }

    pub(crate) fn comment_style(&self) -> TextStyle {
        let size = if self.m.wide { 14.0 * self.bar_s() } else { 12.5 * self.m.s };
        TextStyle { size, color: self.theme.text_faint, ..self.cand_style() }
    }

    pub(crate) fn grid_cell_h(&self) -> f32 {
        if self.m.wide { self.m.row_h } else { self.m.row_h * 0.92 }
    }

    /// Scroll distance of one candidate-grid page: the whole rows that fit.
    fn grid_page(&self) -> f32 {
        let h = self.grid.map_or(0.0, |g| g.h);
        let cell = self.grid_cell_h();
        ((h / cell).floor() * cell).max(cell)
    }

    pub(crate) fn column_item_h(&self) -> f32 {
        self.m.row_h * 0.72
    }

    pub(crate) fn column_items(&self) -> Vec<String> {
        match self.column {
            Some((_, ColumnKind::T9)) if self.composing() && !self.spellings.is_empty() => self.spellings.clone(),
            Some((_, ColumnKind::T9)) => layout::T9_PUNCT.iter().map(|s| s.to_string()).collect(),
            Some((_, ColumnKind::Numbers)) => layout::NUM_COLUMN.iter().map(|s| s.to_string()).collect(),
            None => Vec::new(),
        }
    }

    /// Columns and cell height of the symbol grid.
    pub(crate) fn sym_cell(&self, grid: Rect) -> (usize, f32) {
        let cols = ((grid.w / (self.m.row_h * 1.3)).floor() as usize).clamp(6, 10);
        (cols, self.m.row_h)
    }

    // -----------------------------------------------------------------------------------------
    // Candidate layout
    // -----------------------------------------------------------------------------------------

    /// Lays out the strip and grid cells. `measured`: `measure` is a real text measurer.
    pub(crate) fn ensure_cand_layout(&mut self, measure: &mut dyn FnMut(&str, TextStyle) -> f32, measured: bool) {
        let grid_on = self.panel == Panel::Candidates;
        let key = (self.cand_ver, self.m.w as u32, self.m.h as u32, grid_on, measured);
        let cur = self.layout_cache.key;
        if (cur.0, cur.1, cur.2, cur.3) == (key.0, key.1, key.2, key.3) && (cur.4 || !measured) {
            return;
        }
        let s = self.bar_s();
        let style = self.cand_style();
        let cstyle = self.comment_style();
        let pad = if self.m.wide { 17.0 * s } else { 15.0 * s };
        let min_w = if self.m.wide { 58.0 * s } else { 50.0 * s };
        let mut strip = Vec::with_capacity(self.cands.len());
        let mut x = self.strip_lead();
        let mut widths = Vec::with_capacity(self.cands.len());
        for c in &self.cands {
            let mut tw = measure(&c.text, style);
            if let Some(comment) = &c.comment {
                tw += 4.0 * s + measure(comment, cstyle);
            }
            widths.push(tw);
            let w = (tw + 2.0 * pad).max(min_w);
            strip.push((x, w));
            x += w;
        }
        let strip_w = self.strip_rect().w;
        self.strip.set_max(x + 8.0 * s - strip_w);

        let mut grid = Vec::with_capacity(self.cands.len());
        if let Some(area) = self.grid.filter(|_| self.panel == Panel::Candidates) {
            let target = self.m.row_h * if self.m.wide { 2.0 } else { 1.7 };
            let cols = ((area.w / target).floor() as usize).max(3);
            let unit = area.w / cols as f32;
            let cell_h = self.grid_cell_h();
            let (mut col, mut row) = (0usize, 0usize);
            for tw in widths {
                let span = (((tw + 2.0 * pad) / unit).ceil() as usize).clamp(1, cols);
                if col + span > cols {
                    col = 0;
                    row += 1;
                }
                grid.push(Rect::new(col as f32 * unit, row as f32 * cell_h, span as f32 * unit, cell_h));
                col += span;
            }
            let rows = if grid.is_empty() { 0 } else { row + 1 };
            self.grid_scroll.set_max(rows as f32 * cell_h - area.h);
        }
        self.layout_cache = CandLayout { strip, grid, key };
    }

    fn ensure_cand_layout_estimated(&mut self) {
        self.ensure_cand_layout(&mut |t, st| estimate_width(t, st.size), false);
    }

    fn strip_item_at(&self, x: f32) -> Option<usize> {
        let cx = x - self.strip_rect().x + self.strip.offset;
        self.layout_cache.strip.iter().position(|&(ix, w)| cx >= ix && cx < ix + w)
    }

    fn grid_item_at(&self, x: f32, y: f32) -> Option<usize> {
        let area = self.grid?;
        let (cx, cy) = (x - area.x, y - area.y + self.grid_scroll.offset);
        self.layout_cache.grid.iter().position(|r| r.contains(cx, cy))
    }

    fn column_item_at(&self, y: f32) -> Option<usize> {
        let (rect, _) = self.column?;
        let i = ((y - rect.y + self.column_scroll.offset) / self.column_item_h()).floor();
        (i >= 0.0 && (i as usize) < self.column_items().len()).then_some(i as usize)
    }

    fn sym_item_at(&self, x: f32, y: f32) -> Option<usize> {
        let (Panel::Symbols(tab), Some(grid)) = (self.panel, self.grid) else { return None };
        let (cols, cell_h) = self.sym_cell(grid);
        let col = ((x - grid.x) / (grid.w / cols as f32)).floor().clamp(0.0, cols as f32 - 1.0) as usize;
        let row = ((y - grid.y + self.sym_scroll.offset) / cell_h).floor();
        if row < 0.0 {
            return None;
        }
        let i = row as usize * cols + col;
        (i < layout::symbols(tab).len()).then_some(i)
    }

    // -----------------------------------------------------------------------------------------
    // Hit testing
    // -----------------------------------------------------------------------------------------

    pub(crate) fn hit(&mut self, x: f32, y: f32) -> Target {
        if self.trackpad_active() {
            return Target::PadAux;
        }
        if y < self.m.bar_h {
            if let Some(k) = self.bar_keys.iter().find(|k| k.cell.contains(x, y)) {
                return Target::Bar(k.clone());
            }
            if self.pc {
                return Target::None;
            }
            if self.composing() && self.panel != Panel::Candidates {
                self.ensure_cand_layout_estimated();
                return Target::Strip(self.strip_item_at(x));
            }
            if self.clip_bar_shown() && self.clip_bar_rect().contains(x, y) {
                return Target::ClipCard(self.clip_card_at(x));
            }
            return Target::None;
        }
        if self.panel == Panel::Clipboard {
            if let Some(k) = self.keys.iter().find(|k| k.cell.contains(x, y)) {
                return Target::Key(k.clone());
            }
            return self.clip_grid_hit(x, y);
        }
        if self.panel == Panel::Menu {
            return match self.keys.iter().find(|k| k.cell.contains(x, y)) {
                Some(k) => Target::Key(k.clone()),
                None => Target::CloseMenu,
            };
        }
        if let Some((rect, _)) = self.column {
            if rect.contains(x, y) {
                return Target::Column(self.column_item_at(y));
            }
        }
        if let Some(grid) = self.grid {
            if grid.contains(x, y) {
                return match self.panel {
                    Panel::Candidates => {
                        self.ensure_cand_layout_estimated();
                        Target::Grid(self.grid_item_at(x, y))
                    }
                    _ => Target::SymGrid(self.sym_item_at(x, y)),
                };
            }
        }
        self.nearest_key(x, y).map(|k| Target::Key(k.clone())).unwrap_or(Target::None)
    }

    /// The key whose cell is nearest to the point (generous hit-testing between keys). Keys in
    /// the point's row win over closer keys in other rows.
    fn nearest_key(&self, x: f32, y: f32) -> Option<&Key> {
        let dist = |r: &Rect| {
            let dx = (r.x - x).max(0.0).max(x - (r.x + r.w));
            let dy = (r.y - y).max(0.0).max(y - (r.y + r.h));
            dx * dx + dy * dy
        };
        let in_row = |k: &&Key| y >= k.cell.y && y < k.cell.y + k.cell.h;
        let cmp = |a: &&Key, b: &&Key| dist(&a.cell).total_cmp(&dist(&b.cell));
        self.keys.iter().filter(in_row).min_by(cmp).or_else(|| self.keys.iter().min_by(cmp))
    }

    // -----------------------------------------------------------------------------------------
    // Pointer handling
    // -----------------------------------------------------------------------------------------

    fn on_down(&mut self, e: PointerEvent, r: &mut Response) {
        if self.touches.iter().any(|t| t.id == e.id) {
            // A stale contact with the same id (lost up): drop it cleanly.
            self.cancel_touch(e.id, r);
        }
        let target = self.hit(e.x, e.y);
        r.repaint = true;
        if self.clip_menu.is_some() && !matches!(target, Target::ClipBtn { .. } | Target::ClipGrid(_)) {
            self.clip_menu = None;
        }

        // Rollover: a new character press commits the characters other fingers still hold.
        if matches!(&target, Target::Key(k) if k.bubble) {
            let mut held = Vec::new();
            for t in &mut self.touches {
                if let (false, Mode::Press, Target::Key(k)) = (t.consumed, &t.mode, &t.target) {
                    if k.bubble {
                        t.consumed = true;
                        t.long_at = None;
                        held.push(k.clone());
                    }
                }
            }
            for k in held {
                self.tap_key(&k, r);
            }
        }

        let mut t = Touch {
            id: e.id,
            target,
            mode: Mode::Press,
            x0: e.x,
            y0: e.y,
            x: e.x,
            y: e.y,
            consumed: false,
            long_at: None,
            repeat_at: None,
            repeats: 0,
            vel: VelocityTracker::default(),
            stopped_fling: false,
            used: false,
            repeat_action: None,
            engaged: false,
        };
        let mut is_mod = false;
        match t.target.clone() {
            Target::Key(Key { action: KeyAction::Raw(code), .. }) => {
                self.touches.push(t);
                self.pc_key_down(e.id, code, e.time_ms, r);
                return;
            }
            Target::Key(k) | Target::Bar(k) if k.fires_on_press() => {
                if k.action == KeyAction::Backspace {
                    self.typed();
                }
                let a = self.press_input(&k);
                r.actions.push(UiAction::Input(a.clone()));
                t.repeat_action = Some(a);
                t.repeat_at = Some(e.time_ms + REPEAT_DELAY_MS);
                self.after_key();
            }
            Target::Key(k) => match k.action {
                KeyAction::Space => t.long_at = Some(e.time_ms + TRACKPAD_PRESS_MS),
                KeyAction::Mod(m) => {
                    is_mod = true;
                    // Win: long-press latches. 电脑键盘: a long-pressed Shift / Ctrl / Alt is
                    // really held down.
                    if m == Modifier::Win || self.pc && m != Modifier::Fn {
                        t.long_at = Some(e.time_ms + LONG_PRESS_MS);
                    }
                }
                _ if k.has_long_press() => t.long_at = Some(e.time_ms + LONG_PRESS_MS),
                _ => {}
            },
            Target::Bar(k) if k.hold.is_some() => t.long_at = Some(e.time_ms + LONG_PRESS_MS),
            Target::ClipGrid(Some(_)) => {
                t.long_at = Some(e.time_ms + LONG_PRESS_MS);
                t.stopped_fling = self.clip_scroll.stop();
                t.vel.push(e.y, e.time_ms);
            }
            Target::Strip(_) | Target::Column(_) | Target::Grid(_) | Target::SymGrid(_) | Target::ClipCard(_) | Target::ClipGrid(_) => {
                let horizontal = horizontal(&t.target);
                if let Some(s) = self.scroller_mut(&t.target) {
                    t.stopped_fling = s.stop();
                }
                t.vel.push(if horizontal { e.x } else { e.y }, e.time_ms);
            }
            _ => {}
        }
        self.touches.push(t);
        if is_mod {
            // Held modifiers change labels (uppercase, F keys, Ctrl hints).
            self.rebuild();
        }
    }

    fn on_move(&mut self, e: PointerEvent, r: &mut Response) {
        let Some(i) = self.touches.iter().position(|t| t.id == e.id) else { return };
        let mut t = self.touches[i].clone();
        if t.consumed {
            return;
        }
        t.x = e.x;
        t.y = e.y;
        let (dx, dy) = (e.x - t.x0, e.y - t.y0);
        let row_h = self.m.row_h;
        match (&t.mode, &t.target) {
            (Mode::Press, Target::Key(k)) => match k.action {
                KeyAction::Backspace if t.repeat_action == Some(Action::Backspace) => {
                    if dx < -self.clear_threshold(k) {
                        t.mode = Mode::ClearArmed;
                        t.repeat_at = None;
                        r.repaint = true;
                    }
                }
                KeyAction::Space => {
                    if dx.abs() > SPACE_SLOP {
                        t.mode = Mode::Cursor { steps: 0 };
                        t.long_at = None;
                        r.repaint = true;
                        self.cursor_steps(&mut t, r);
                    }
                }
                _ => {
                    if k.secondary.is_some() && -dy > 0.35 * row_h && -dy > dx.abs() {
                        t.mode = Mode::SwipeUp;
                        t.long_at = None;
                        r.repaint = true;
                    } else if k.bubble && !expand(k.cell, k.cell.w * 0.15, row_h * 0.2).contains(e.x, e.y) {
                        // Slid onto another key: the key under the finger fires on release.
                        if let Some(nk) = self.nearest_key(e.x, e.y).filter(|nk| nk.bubble && nk.cell != k.cell) {
                            t.long_at = nk.has_long_press().then_some(e.time_ms + LONG_PRESS_MS);
                            t.target = Target::Key(nk.clone());
                            t.x0 = e.x;
                            t.y0 = e.y;
                            r.repaint = true;
                        }
                    }
                }
            },
            (Mode::SwipeUp, _) => {
                if -dy < 0.2 * row_h {
                    t.mode = Mode::Press;
                    r.repaint = true;
                }
            }
            (Mode::ClearArmed, Target::Key(k)) => {
                if dx > -self.clear_threshold(k) * 0.5 {
                    t.mode = Mode::Press;
                    r.repaint = true;
                }
            }
            (Mode::Trackpad { .. }, _) => self.trackpad_move(&mut t, e, r),
            (Mode::Cursor { .. }, _) => self.cursor_steps(&mut t, r),
            (Mode::Long { alts, sel, first, anchor_x, popup, cell_w }, _) => {
                let n = alts.len() as i32;
                let shift = ((e.x - anchor_x) / cell_w + 0.5).floor() as i32;
                let new = (*first as i32 + shift).clamp(0, n - 1) as usize;
                if new != *sel {
                    let (first, anchor_x, popup, cell_w) = (*first, *anchor_x, *popup, *cell_w);
                    t.mode = Mode::Long { alts: alts.clone(), sel: new, first, anchor_x, popup, cell_w };
                    r.repaint = true;
                }
            }
            (
                Mode::Press,
                Target::Strip(_) | Target::Column(_) | Target::Grid(_) | Target::SymGrid(_) | Target::ClipCard(_) | Target::ClipGrid(_),
            ) => {
                let horizontal = horizontal(&t.target);
                t.vel.push(if horizontal { e.x } else { e.y }, e.time_ms);
                let d = if horizontal { dx } else { dy };
                if d.abs() > SLOP {
                    // Start the drag from the slop boundary so the content does not jump.
                    t.mode = Mode::Scroll;
                    t.long_at = None;
                    let origin = d.signum() * SLOP;
                    if let Some(s) = self.scroller_mut(&t.target) {
                        s.begin_drag();
                        s.drag_to(d - origin);
                    }
                    if horizontal {
                        t.x0 += origin;
                    } else {
                        t.y0 += origin;
                    }
                    r.repaint = true;
                    self.maybe_request_more(r);
                }
            }
            (Mode::Scroll, _) => {
                let horizontal = horizontal(&t.target);
                t.vel.push(if horizontal { e.x } else { e.y }, e.time_ms);
                let d = if horizontal { dx } else { dy };
                if let Some(s) = self.scroller_mut(&t.target) {
                    if s.drag_to(d) {
                        r.repaint = true;
                    }
                }
                self.maybe_request_more(r);
            }
            _ => {}
        }
        self.touches[i] = t;
    }

    fn on_up(&mut self, e: PointerEvent, r: &mut Response) {
        let Some(i) = self.touches.iter().position(|t| t.id == e.id) else { return };
        let t = self.touches.remove(i);
        r.repaint = true;
        if let Target::Key(Key { action: KeyAction::Raw(code), .. }) = &t.target {
            self.pc_key_up(*code, r);
            return;
        }
        if let Target::Key(Key { action: KeyAction::Mod(m), .. }) = &t.target {
            if !t.consumed && !t.used {
                self.mod_tapped(*m, r);
            }
            if self.pc {
                self.pc_sync(r);
            }
            self.rebuild();
            return;
        }
        if let Mode::Trackpad { .. } = t.mode {
            self.trackpad_end(r);
            return;
        }
        if t.consumed {
            return;
        }
        match (t.mode, t.target) {
            (Mode::Press, Target::Key(k)) => {
                if !k.fires_on_press() {
                    self.tap_key(&k, r);
                }
            }
            (Mode::SwipeUp, Target::Key(k)) => {
                if let Some(s) = k.secondary {
                    r.actions.push(UiAction::Input(Action::Text(s)));
                    self.after_key();
                    self.typed();
                }
            }
            (Mode::Long { alts, sel, .. }, _) => {
                r.actions.push(UiAction::Input(Action::Text(alts[sel].clone())));
                self.after_key();
                self.typed();
            }
            (Mode::ClearArmed, _) => r.actions.push(UiAction::Input(Action::ClearComposition)),
            (Mode::Press, Target::Bar(k)) => {
                if !k.fires_on_press() {
                    self.tap_key(&k, r);
                }
            }
            (Mode::Press, Target::Strip(Some(idx))) if !t.stopped_fling => {
                r.actions.push(UiAction::Input(Action::Select(idx)));
            }
            (Mode::Press, Target::Grid(Some(idx))) if !t.stopped_fling => {
                r.actions.push(UiAction::Input(Action::Select(idx)));
                self.set_panel(Panel::Keys);
            }
            (Mode::Press, Target::Column(Some(idx))) if !t.stopped_fling => {
                if let Some(item) = self.column_items().get(idx).cloned() {
                    let spelling = matches!(self.column, Some((_, ColumnKind::T9))) && self.composing() && !self.spellings.is_empty();
                    let action = if spelling { Action::PickSpelling(item) } else { Action::Text(item) };
                    r.actions.push(UiAction::Input(action));
                }
            }
            (Mode::Press, Target::SymGrid(Some(idx))) if !t.stopped_fling => {
                if let Panel::Symbols(tab) = self.panel {
                    if let Some(s) = layout::symbols(tab).get(idx) {
                        r.actions.push(UiAction::Input(Action::Text(s.to_string())));
                    }
                }
            }
            (Mode::Press, Target::ClipCard(Some(idx))) if !t.stopped_fling => {
                if let Some(c) = self.clips.get(idx) {
                    r.actions.push(UiAction::Paste(c.text.clone()));
                }
            }
            (Mode::Press, Target::ClipGrid(Some(pos))) if !t.stopped_fling => {
                if self.clip_menu.take().is_none() {
                    if let Some(c) = self.clip_panel_order().get(pos).and_then(|&i| self.clips.get(i)) {
                        r.actions.push(UiAction::Paste(c.text.clone()));
                        self.set_panel(Panel::Keys);
                    }
                }
            }
            (Mode::Press, Target::ClipGrid(None)) => self.clip_menu = None,
            (Mode::Press, Target::ClipBtn { id, delete }) => {
                self.clip_menu = None;
                if delete {
                    r.actions.push(UiAction::DeleteClip(id));
                } else if let Some(c) = self.clips.iter().find(|c| c.id == id) {
                    r.actions.push(UiAction::PinClip { id, pinned: !c.pinned });
                }
            }
            (Mode::Press, Target::PadAux) => self.trackpad_aux_tap(r),
            (Mode::Press, Target::CloseMenu) => self.set_panel(Panel::Keys),
            (Mode::Scroll, target) => {
                let v = t.vel.velocity(e.time_ms);
                if let Some(s) = self.scroller_mut(&target) {
                    s.fling(v, e.time_ms);
                }
            }
            _ => {}
        }
    }

    /// Drops a touch without acting on it (system cancel, stale id). Pass-through keys and
    /// modifiers that were sent down are released.
    fn cancel_touch(&mut self, id: u32, r: &mut Response) {
        let Some(i) = self.touches.iter().position(|t| t.id == id) else { return };
        let t = self.touches.remove(i);
        r.repaint = true;
        match &t.target {
            Target::Key(Key { action: KeyAction::Raw(code), .. }) => self.pc_key_up(*code, r),
            Target::Key(Key { action: KeyAction::Mod(_), .. }) => {
                if self.pc {
                    self.pc_sync(r);
                }
                self.rebuild();
            }
            _ => {
                if let Mode::Trackpad { .. } = t.mode {
                    self.trackpad_end(r);
                }
            }
        }
    }

    fn clear_threshold(&self, k: &Key) -> f32 {
        (k.cell.w * 0.6).clamp(36.0, 90.0)
    }

    fn cursor_step(&self) -> f32 {
        (self.m.keys_area().w / 10.0 * 0.45).clamp(22.0, 64.0)
    }

    fn cursor_steps(&self, t: &mut Touch, r: &mut Response) {
        let Mode::Cursor { steps } = t.mode else { return };
        let target = ((t.x - t.x0) / self.cursor_step()).trunc() as i32;
        let key = if target > steps { EditKey::Right } else { EditKey::Left };
        for _ in 0..(target - steps).abs() {
            r.actions.push(UiAction::Input(Action::Edit(key)));
        }
        t.mode = Mode::Cursor { steps: target };
    }

    fn scroller_mut(&mut self, target: &Target) -> Option<&mut Scroller> {
        match target {
            Target::Strip(_) => Some(&mut self.strip),
            Target::Column(_) => Some(&mut self.column_scroll),
            Target::Grid(_) => Some(&mut self.grid_scroll),
            Target::SymGrid(_) => Some(&mut self.sym_scroll),
            Target::ClipCard(_) => Some(&mut self.clip_strip),
            Target::ClipGrid(_) => Some(&mut self.clip_scroll),
            _ => None,
        }
    }

    /// The popup rect and cell width for `n` alternates over key `k`.
    /// The pre-selected cell `sel` sits over the key when there is room.
    pub(crate) fn popup_geometry(&self, k: &Key, n: usize, sel: usize) -> (Rect, f32) {
        let face = self.m.face(k.cell);
        let row_h = self.m.row_h;
        let cell_w = (face.w * 0.8).clamp(row_h * 0.8, row_h * 0.95);
        let h = row_h * 1.05;
        let w = cell_w * n as f32;
        let x = (face.x + face.w / 2.0 - (sel as f32 + 0.5) * cell_w)
            .clamp(4.0, (self.m.w - w - 4.0).max(4.0));
        let y = (face.y + face.h * 0.35 - h).max(1.0);
        (Rect::new(x, y, w, h), cell_w)
    }

    /// Fires a key's tap action.
    fn tap_key(&mut self, k: &Key, r: &mut Response) {
        let input = |r: &mut Response, a: Action| r.actions.push(UiAction::Input(a));
        let mods = self.mods();
        let chording = mods.chording();
        match &k.action {
            KeyAction::Letter(c) => {
                let a = if chording {
                    Action::Key(self.chord(KeyCode::Char(*c)))
                } else {
                    self.typed();
                    Action::Char(if mods.on(Modifier::Shift) { c.to_ascii_uppercase() } else { *c })
                };
                input(r, a);
                self.after_key();
            }
            KeyAction::Char(c) => {
                self.typed();
                input(r, Action::Char(*c));
            }
            KeyAction::Text(s) => {
                let a = match k.code {
                    Some(code) if chording => Action::Key(self.chord(KeyCode::Char(code))),
                    _ => {
                        self.typed();
                        Action::Text(s.clone())
                    }
                };
                input(r, a);
                self.after_key();
            }
            KeyAction::Backspace => input(r, Action::Backspace),
            KeyAction::Edit(_) => {
                let a = self.press_input(k);
                input(r, a);
                self.after_key();
            }
            KeyAction::Chord(c) => {
                let m = self.chord(c.key.unwrap_or(KeyCode::Char(' ')));
                let merged = KeyChord {
                    ctrl: c.ctrl || m.ctrl,
                    shift: c.shift || m.shift,
                    alt: c.alt || m.alt,
                    win: c.win || m.win,
                    key: c.key,
                };
                input(r, Action::Key(merged));
                if *c == KeyChord::COPY {
                    // Nothing selected? The host checks the clipboard and opens selection mode.
                    r.actions.push(UiAction::CheckCopied);
                }
                self.after_key();
            }
            KeyAction::ClearAll => {
                input(r, Action::Key(KeyChord::SELECT_ALL));
                input(r, Action::Key(KeyChord::key(KeyCode::Edit(EditKey::Delete))));
                self.after_key();
            }
            KeyAction::Mod(_) => {}
            KeyAction::CapsLock => {
                let i = Modifier::Shift.index();
                self.latch[i] = if self.latch[i] == Latch::Locked { Latch::Off } else { Latch::Locked };
                self.rebuild();
            }
            KeyAction::PcKeys => self.set_panel(if self.panel == Panel::PcKeys { Panel::Keys } else { Panel::PcKeys }),
            KeyAction::PageUp | KeyAction::PageDown => {
                let step = if k.action == KeyAction::PageUp { -self.grid_page() } else { self.grid_page() };
                let g = &mut self.grid_scroll;
                g.stop();
                g.offset = (g.offset + step).clamp(0.0, g.max);
                self.maybe_request_more(r);
            }
            KeyAction::Space if chording => {
                input(r, Action::Key(self.chord(KeyCode::Char(' '))));
                self.after_key();
            }
            KeyAction::Enter if chording || mods.on(Modifier::Shift) => {
                input(r, Action::Key(self.chord(KeyCode::Edit(EditKey::Enter))));
                self.after_key();
            }
            KeyAction::Space => {
                self.typed();
                input(r, Action::Space);
                self.after_key();
            }
            KeyAction::Enter => {
                self.typed();
                input(r, Action::Enter);
                self.after_key();
            }
            KeyAction::ToggleChinese => input(r, Action::ToggleChinese),
            KeyAction::LayoutMenu => {
                self.set_panel(if self.panel == Panel::Menu { Panel::Keys } else { Panel::Menu });
            }
            KeyAction::Symbols => {
                self.set_panel(Panel::Symbols(if self.chinese { SymTab::Chinese } else { SymTab::English }));
            }
            KeyAction::Numbers => self.set_panel(Panel::Numbers),
            KeyAction::Back | KeyAction::CollapseCandidates => self.set_panel(Panel::Keys),
            KeyAction::ClearComposition => input(r, Action::ClearComposition),
            KeyAction::Tab(tab) => self.set_panel(Panel::Symbols(*tab)),
            KeyAction::Voice => r.actions.push(UiAction::Voice),
            KeyAction::Hide => r.actions.push(UiAction::Hide),
            KeyAction::ExpandCandidates => {
                self.set_panel(Panel::Candidates);
                self.request_more(r);
            }
            KeyAction::SetLayout(Layout::Pc) => {
                if self.composing() {
                    // Like any other key that leaves the composition: commit the first candidate.
                    input(r, Action::Space);
                }
                if self.set_pc(true, r) {
                    r.actions.push(UiAction::PcKeyboard(true));
                }
            }
            KeyAction::SetLayout(l) => {
                if self.set_pc(false, r) {
                    r.actions.push(UiAction::PcKeyboard(false));
                }
                let schema = match l {
                    Layout::Pinyin => Some(Schema::Pinyin),
                    Layout::Shuangpin => Some(Schema::Shuangpin),
                    Layout::T9 => Some(Schema::T9),
                    Layout::English | Layout::Pc => None,
                };
                match schema {
                    Some(s) => {
                        if s != self.schema {
                            input(r, Action::SetSchema(s));
                        }
                        if !self.chinese {
                            input(r, Action::ToggleChinese);
                        }
                    }
                    None if self.chinese => input(r, Action::ToggleChinese),
                    None => {}
                }
                self.set_panel(Panel::Keys);
            }
            KeyAction::ToggleTheme => {
                let next = if self.theme_kind == ThemeKind::Dark { ThemeKind::Light } else { ThemeKind::Dark };
                self.set_theme(next);
                r.actions.push(UiAction::ThemeChanged(next));
            }
            KeyAction::T9One => {
                self.typed();
                input(r, Action::Char(if self.composing() { '\'' } else { '1' }));
            }
            // Fired on press (`fires_on_press`).
            KeyAction::Raw(_) | KeyAction::Press(_) => {}
            KeyAction::PcBack => {
                if self.set_pc(false, r) {
                    r.actions.push(UiAction::PcKeyboard(false));
                }
            }
            KeyAction::SelectMode => {
                let on = !self.selecting;
                self.set_selecting(on);
            }
            KeyAction::Sel(act) => {
                match act {
                    SelAct::Copy => {
                        input(r, Action::Key(KeyChord::COPY));
                    }
                    SelAct::Cut => input(r, Action::Key(KeyChord::CUT)),
                    SelAct::Paste => input(r, Action::Key(KeyChord::PASTE)),
                    SelAct::Delete => input(r, Action::Edit(EditKey::Delete)),
                    SelAct::Done => {}
                }
                self.set_selecting(false);
            }
            KeyAction::ClipPanel => {
                self.clip_menu = None;
                self.set_panel(if self.panel == Panel::Clipboard { Panel::Keys } else { Panel::Clipboard });
            }
            KeyAction::CloseClipBar => {
                self.clip_bar = false;
                self.rebuild();
            }
            KeyAction::ClearClips => {
                self.clip_menu = None;
                r.actions.push(UiAction::ClearClips);
            }
        }
    }

    pub(crate) fn set_panel(&mut self, panel: Panel) {
        if self.panel != panel {
            let tab_change = matches!((self.panel, panel), (Panel::Symbols(_), Panel::Symbols(_)));
            self.panel = panel;
            if tab_change || matches!(panel, Panel::Symbols(_)) {
                self.sym_scroll.reset();
            }
            self.grid_scroll.reset();
            self.column_scroll.reset();
            self.clip_scroll.reset();
            self.rebuild();
        }
    }

    fn request_more(&mut self, r: &mut Response) {
        if !self.more_pending && !self.more_done && self.composing() {
            self.more_pending = true;
            r.actions.push(UiAction::WantMoreCandidates { start: self.cands.len(), count: MORE_BATCH });
        }
    }

    fn maybe_request_more(&mut self, r: &mut Response) {
        if self.panel == Panel::Candidates {
            if let Some(g) = self.grid {
                if self.grid_scroll.offset >= self.grid_scroll.max - g.h * 0.5 {
                    self.request_more(r);
                }
            }
        } else if self.composing() && self.strip.offset >= self.strip.max - self.strip_rect().w * 0.5 {
            self.request_more(r);
        }
    }

    /// Sets `timer_ms` to the earliest pending deadline.
    pub(crate) fn finish(&self, mut r: Response) -> Response {
        let mut next: Option<u64> = None;
        let mut consider = |d: u64| next = Some(next.map_or(d, |n: u64| n.min(d)));
        for t in &self.touches {
            t.long_at.into_iter().chain(t.repeat_at).for_each(&mut consider);
        }
        if self.scrollers().iter().any(|s| s.animating()) {
            consider(self.now + FRAME_MS);
        }
        if let Some((_, until)) = &self.toast {
            consider(*until);
        }
        r.timer_ms = next.map(|d| d.saturating_sub(self.now).max(1));
        r
    }
}

fn expand(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.x - dx, r.y - dy, r.w + 2.0 * dx, r.h + 2.0 * dy)
}

/// Text width estimate used before the first paint has measured real widths.
pub(crate) fn estimate_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { if c.is_ascii_uppercase() { 0.62 } else { 0.52 } } else { 1.0 })
        .sum::<f32>()
        * size
}

/// Backspace auto-repeat interval: accelerates from 110 ms to 40 ms.
pub(crate) fn repeat_interval(repeats: u32) -> u64 {
    110u64.saturating_sub(repeats as u64 * 10).max(40)
}

impl View for KeyboardView {
    fn resize(&mut self, width: f32, height: f32) {
        let m = if self.pc { Metrics::pc(width, height) } else { Metrics::new(width, height) };
        if m != self.m {
            self.m = m;
            self.rebuild();
        }
    }

    fn preferred_height(&self, width: f32) -> f32 {
        if layout::is_wide(width) {
            layout::preferred_height_wide(width, self.height_scale)
        } else {
            layout::preferred_height(width) * self.height_scale
        }
    }

    fn paint(&mut self, canvas: &mut dyn Canvas) {
        self.ensure_cand_layout(&mut |t, st| canvas.measure_text(t, st), true);
        self.draw(canvas);
    }

    fn pointer(&mut self, e: PointerEvent) -> Response {
        self.now = self.now.max(e.time_ms);
        let mut r = Response::none();
        match e.phase {
            PointerPhase::Down => self.on_down(e, &mut r),
            PointerPhase::Move => self.on_move(e, &mut r),
            PointerPhase::Up => self.on_up(e, &mut r),
            PointerPhase::Cancel => self.cancel_touch(e.id, &mut r),
        }
        self.finish(r)
    }

    fn timer(&mut self, now_ms: u64) -> Response {
        self.now = self.now.max(now_ms);
        let now = self.now;
        let mut r = Response::none();
        for i in 0..self.touches.len() {
            let mut t = self.touches[i].clone();
            // Some(true): a hold action was sent; Some(false): Win latched.
            let mut fired: Option<bool> = None;
            if t.long_at.is_some_and(|d| d <= now) {
                t.long_at = None;
                if let (Mode::Press, Target::ClipGrid(Some(pos))) = (&t.mode, &t.target) {
                    // Long-press a clipboard card: show its 固定 / 删除 buttons.
                    self.clip_menu = self.clip_panel_order().get(*pos).and_then(|&i| self.clips.get(i)).map(|c| c.id);
                    t.consumed = true;
                    r.repaint = true;
                } else if let (Mode::Press, Target::Key(k) | Target::Bar(k)) = (&t.mode, &t.target) {
                    if k.action == KeyAction::Space {
                        t.mode = Mode::Trackpad { acc_x: 0.0, acc_y: 0.0, lx: t.x, ly: t.y, lt: now };
                        self.pad_select = false;
                        self.pad_selected = false;
                        self.selecting = false;
                        r.repaint = true;
                    } else if self.pc && matches!(k.action, KeyAction::Mod(Modifier::Shift | Modifier::Ctrl | Modifier::Alt)) {
                        // 电脑键盘: a long-pressed modifier is really held down until released.
                        t.engaged = true;
                        t.used = true;
                        self.touches[i] = t.clone();
                        self.pc_sync(&mut r);
                        r.repaint = true;
                    } else if k.action == KeyAction::Mod(Modifier::Win) {
                        // Long-press latches Win instead of pressing it.
                        let i = Modifier::Win.index();
                        self.latch[i] = if self.latch[i] == Latch::Off { Latch::Once } else { Latch::Locked };
                        t.used = true;
                        r.repaint = true;
                        fired = Some(false);
                    } else if let Some(hold) = &k.hold {
                        r.actions.push(UiAction::Input(hold.clone()));
                        t.consumed = true;
                        r.repaint = true;
                        fired = Some(true);
                    } else if k.has_long_press() {
                        let (alts, sel) = k.popup();
                        let (popup, cell_w) = self.popup_geometry(k, alts.len(), sel);
                        t.mode = Mode::Long { alts, sel, first: sel, anchor_x: t.x, popup, cell_w };
                        r.repaint = true;
                    }
                }
            }
            if let Some(sent) = fired {
                self.touches[i] = t;
                if sent {
                    self.after_key();
                } else {
                    self.rebuild();
                }
                continue;
            }
            if t.repeat_at.is_some_and(|d| d <= now) {
                if let (Mode::Press, Some(a)) = (&t.mode, &t.repeat_action) {
                    r.actions.push(UiAction::Input(a.clone()));
                    t.repeats += 1;
                    let interval = if matches!(a, Action::KeyDown(_)) { PC_REPEAT_MS } else { repeat_interval(t.repeats) };
                    t.repeat_at = Some(now + interval);
                } else {
                    t.repeat_at = None;
                }
            }
            self.touches[i] = t;
        }
        let mut scrolled = false;
        for s in [
            &mut self.strip,
            &mut self.grid_scroll,
            &mut self.column_scroll,
            &mut self.sym_scroll,
            &mut self.clip_strip,
            &mut self.clip_scroll,
        ] {
            scrolled |= s.step(now);
        }
        if self.toast.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.toast = None;
            r.repaint = true;
        }
        if scrolled {
            r.repaint = true;
            self.maybe_request_more(&mut r);
        }
        self.finish(r)
    }

    fn set_input_state(&mut self, state: InputState) -> Response {
        let layout_changed = self.chinese != state.chinese || self.schema != state.schema;
        let snap_changed =
            self.snapshot.preedit != state.snapshot.preedit || self.snapshot.candidates != state.snapshot.candidates;
        if !layout_changed && !snap_changed {
            return Response::none();
        }
        let mut r = Response::repaint();
        self.chinese = state.chinese;
        self.schema = state.schema;
        self.snapshot = Snapshot { commit: None, ..state.snapshot };
        if layout_changed {
            self.latch[Modifier::Shift.index()] = Latch::Off;
            if self.panel == Panel::Menu {
                self.panel = Panel::Keys;
            }
        }
        if snap_changed {
            self.cands = self.snapshot.candidates.clone();
            self.cand_ver += 1;
            self.more_pending = false;
            self.more_done = false;
            self.strip.reset();
            self.grid_scroll.reset();
            if self.composing() {
                if self.layout() == Layout::T9 {
                    r.actions.push(UiAction::WantT9Spellings);
                }
            } else {
                self.spellings.clear();
                self.column_scroll.reset();
                if self.panel == Panel::Candidates {
                    self.panel = Panel::Keys;
                }
            }
        }
        self.rebuild();
        if snap_changed && self.panel == Panel::Candidates {
            self.request_more(&mut r);
        }
        self.finish(r)
    }

    fn set_more_candidates(&mut self, start: usize, candidates: Vec<Candidate>) -> Response {
        self.more_pending = false;
        if start > self.cands.len() || !self.composing() {
            return Response::none();
        }
        if candidates.len() < MORE_BATCH {
            self.more_done = true;
        }
        if candidates.is_empty() && start == self.cands.len() {
            return Response::none();
        }
        self.cands.truncate(start);
        self.cands.extend(candidates);
        self.cand_ver += 1;
        self.finish(Response::repaint())
    }

    fn set_t9_spellings(&mut self, spellings: Vec<String>) -> Response {
        if spellings == self.spellings {
            return Response::none();
        }
        self.spellings = spellings;
        self.column_scroll.reset();
        self.rebuild();
        self.finish(Response::repaint())
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
#[path = "keyboard_tests.rs"]
mod tests;
