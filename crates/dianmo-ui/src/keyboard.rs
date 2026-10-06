//! [`KeyboardView`]: the phone-style keyboard (candidate bar, layouts, gestures).
//!
//! Gesture rules (per pointer, so two thumbs work independently):
//! - Character keys fire on release. If another finger presses a character key while one is
//!   still held, the held key fires first (keeps the order of fast two-thumb typing).
//! - Long-press (350 ms) or swipe-up on a key enters its secondary (shown in the top-right
//!   corner); keys with several alternates open a popup where sliding picks one.
//! - Backspace fires on press and repeats with acceleration; swipe left clears the composition.
//! - Dragging along the space bar moves the cursor; holding it starts voice typing.
//! - The candidate strip, the T9 column and the grids scroll by dragging, with a short fling.

use dianmo_core::{Action, Candidate, EditKey, Schema, Snapshot};

use crate::canvas::{Canvas, Rect, TextStyle};
use crate::layout::{self, BuildCtx, ColumnKind, Key, KeyAction, Layout, Metrics, Shift, SymTab, Tone};
use crate::scroll::{FRAME_MS, Scroller, VelocityTracker};
use crate::theme::{Theme, ThemeKind};
use crate::view::{InputState, PointerEvent, PointerPhase, Response, UiAction, View};

pub(crate) const LONG_PRESS_MS: u64 = 350;
pub(crate) const VOICE_PRESS_MS: u64 = 600;
pub(crate) const REPEAT_DELAY_MS: u64 = 400;
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
    /// Space held: release starts voice typing.
    Voice,
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
    pub(crate) shift: Shift,
    last_shift_tap: Option<u64>,
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
    now: u64,
    /// Multiplier on the preferred height (user setting).
    height_scale: f32,
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
            shift: Shift::Off,
            last_shift_tap: None,
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
        Layout::of(self.chinese, self.schema)
    }

    /// Centre of a visible key, for automated GUI tests. `name` is a key label ("a", "，",
    /// "符号", "123") or one of "shift", "backspace", "enter", "space", "layout", "toggle",
    /// "expand", "voice", "hide".
    pub fn key_center(&self, name: &str) -> Option<(f32, f32)> {
        let named = |k: &Key| match name {
            "shift" => k.action == KeyAction::Shift,
            "backspace" => k.action == KeyAction::Backspace,
            "enter" => k.action == KeyAction::Enter,
            "space" => k.action == KeyAction::Space,
            "layout" => k.action == KeyAction::LayoutMenu,
            "toggle" => k.action == KeyAction::ToggleChinese,
            "expand" => k.action == KeyAction::ExpandCandidates,
            "voice" => k.action == KeyAction::Voice,
            "hide" => k.action == KeyAction::Hide,
            _ => !k.icon && (k.label == name || k.label.eq_ignore_ascii_case(name) && k.label.len() == 1),
        };
        self.keys.iter().chain(&self.bar_keys).find(|k| named(k)).map(|k| (k.cell.x + k.cell.w / 2.0, k.cell.y + k.cell.h / 2.0))
    }

    pub(crate) fn composing(&self) -> bool {
        self.snapshot.is_composing()
    }

    // -----------------------------------------------------------------------------------------
    // Building
    // -----------------------------------------------------------------------------------------

    pub(crate) fn rebuild(&mut self) {
        let ctx = BuildCtx { layout: self.layout(), chinese: self.chinese, composing: self.composing(), shift: self.shift };
        let built = match self.panel {
            Panel::Keys if ctx.layout == Layout::T9 => layout::build_t9(&self.m, &ctx),
            Panel::Keys => layout::build_letters(&self.m, &ctx),
            Panel::Numbers => layout::build_numbers(&self.m, &ctx),
            Panel::Symbols(tab) => layout::build_symbols(&self.m, tab),
            Panel::Candidates => layout::build_candidate_grid(&self.m),
            Panel::Menu => layout::build_menu(&self.m, ctx.layout, self.theme_kind == ThemeKind::Dark),
        };
        self.keys = built.keys;
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
    }

    fn build_bar(&self) -> Vec<Key> {
        let m = &self.m;
        let cw = self.chevron_w();
        let right = Rect::new(m.w - cw - m.pad_x, 0.0, cw, m.bar_h);
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
        let w = m.bar_h * 1.25;
        let mut keys = Vec::new();
        let items = [
            (KeyAction::Voice, layout::icon::MIC),
            (KeyAction::LayoutMenu, layout::icon::KEYBOARD),
            (KeyAction::Tab(SymTab::Emoji), layout::icon::EMOJI),
        ];
        for (i, (action, glyph)) in items.into_iter().enumerate() {
            let mut k = Key::icon(action, glyph, Tone::Flat);
            k.cell = Rect::new(m.pad_x + 6.0 * m.s + i as f32 * w, 0.0, w, m.bar_h);
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

    pub(crate) fn cand_style(&self) -> TextStyle {
        TextStyle { size: 21.0 * self.m.s, color: self.theme.text, align: crate::Align::Start, bold: false, font: crate::Font::Ui }
    }

    pub(crate) fn comment_style(&self) -> TextStyle {
        TextStyle { size: 12.5 * self.m.s, color: self.theme.text_faint, ..self.cand_style() }
    }

    pub(crate) fn grid_cell_h(&self) -> f32 {
        self.m.row_h * 0.92
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
        let s = self.m.s;
        let style = self.cand_style();
        let cstyle = self.comment_style();
        let pad = 15.0 * s;
        let min_w = 50.0 * s;
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
            let target = self.m.row_h * 1.7;
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
        if y < self.m.bar_h {
            if let Some(k) = self.bar_keys.iter().find(|k| k.cell.contains(x, y)) {
                return Target::Bar(k.clone());
            }
            if self.composing() && self.panel != Panel::Candidates {
                self.ensure_cand_layout_estimated();
                return Target::Strip(self.strip_item_at(x));
            }
            return Target::None;
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
        self.touches.retain(|t| t.id != e.id);
        let target = self.hit(e.x, e.y);
        r.repaint = true;

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
        };
        match &t.target {
            Target::Key(k) => match k.action {
                KeyAction::Backspace => {
                    r.actions.push(UiAction::Input(Action::Backspace));
                    t.repeat_at = Some(e.time_ms + REPEAT_DELAY_MS);
                }
                KeyAction::Space => t.long_at = Some(e.time_ms + VOICE_PRESS_MS),
                _ if k.has_long_press() => t.long_at = Some(e.time_ms + LONG_PRESS_MS),
                _ => {}
            },
            Target::Strip(_) | Target::Column(_) | Target::Grid(_) | Target::SymGrid(_) => {
                let horizontal = matches!(t.target, Target::Strip(_));
                if let Some(s) = self.scroller_mut(&t.target) {
                    t.stopped_fling = s.stop();
                }
                t.vel.push(if horizontal { e.x } else { e.y }, e.time_ms);
            }
            _ => {}
        }
        self.touches.push(t);
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
                KeyAction::Backspace => {
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
            (Mode::Voice, _) => {
                if dx.abs() > SPACE_SLOP {
                    t.mode = Mode::Cursor { steps: 0 };
                    r.repaint = true;
                    self.cursor_steps(&mut t, r);
                }
            }
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
            (Mode::Press, Target::Strip(_) | Target::Column(_) | Target::Grid(_) | Target::SymGrid(_)) => {
                let horizontal = matches!(t.target, Target::Strip(_));
                t.vel.push(if horizontal { e.x } else { e.y }, e.time_ms);
                let d = if horizontal { dx } else { dy };
                if d.abs() > SLOP {
                    // Start the drag from the slop boundary so the content does not jump.
                    t.mode = Mode::Scroll;
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
                let horizontal = matches!(t.target, Target::Strip(_));
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
        if t.consumed {
            return;
        }
        match (t.mode, t.target) {
            (Mode::Press, Target::Key(k)) => {
                if k.action != KeyAction::Backspace {
                    self.tap_key(&k, r);
                }
            }
            (Mode::SwipeUp, Target::Key(k)) => {
                if let Some(s) = k.secondary {
                    r.actions.push(UiAction::Input(Action::Text(s)));
                }
            }
            (Mode::Long { alts, sel, .. }, _) => {
                r.actions.push(UiAction::Input(Action::Text(alts[sel].clone())));
            }
            (Mode::ClearArmed, _) => r.actions.push(UiAction::Input(Action::ClearComposition)),
            (Mode::Voice, _) => r.actions.push(UiAction::Voice),
            (Mode::Press, Target::Bar(k)) => self.tap_key(&k, r),
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
            (Mode::Press, Target::CloseMenu) => self.set_panel(Panel::Keys),
            (Mode::Scroll, target) => {
                let horizontal = matches!(target, Target::Strip(_));
                let v = t.vel.velocity(e.time_ms);
                let _ = horizontal;
                if let Some(s) = self.scroller_mut(&target) {
                    s.fling(v, e.time_ms);
                }
            }
            _ => {}
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
        match &k.action {
            KeyAction::Letter(c) => {
                let c = if self.shift != Shift::Off { c.to_ascii_uppercase() } else { *c };
                input(r, Action::Char(c));
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                    self.rebuild();
                }
            }
            KeyAction::Char(c) => input(r, Action::Char(*c)),
            KeyAction::Text(s) => input(r, Action::Text(s.clone())),
            KeyAction::Backspace => input(r, Action::Backspace),
            KeyAction::Shift => {
                let quick = self.last_shift_tap.is_some_and(|t| self.now.saturating_sub(t) <= DOUBLE_TAP_MS);
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once if quick => Shift::Locked,
                    Shift::Once | Shift::Locked => Shift::Off,
                };
                self.last_shift_tap = Some(self.now);
                self.rebuild();
            }
            KeyAction::Space => input(r, Action::Space),
            KeyAction::Enter => input(r, Action::Enter),
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
            KeyAction::SetLayout(l) => {
                let schema = match l {
                    Layout::Pinyin => Some(Schema::Pinyin),
                    Layout::Shuangpin => Some(Schema::Shuangpin),
                    Layout::T9 => Some(Schema::T9),
                    Layout::English => None,
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
            KeyAction::T9One => input(r, Action::Char(if self.composing() { '\'' } else { '1' })),
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
    fn finish(&self, mut r: Response) -> Response {
        let mut next: Option<u64> = None;
        let mut consider = |d: u64| next = Some(next.map_or(d, |n: u64| n.min(d)));
        for t in &self.touches {
            t.long_at.into_iter().chain(t.repeat_at).for_each(&mut consider);
        }
        if [&self.strip, &self.grid_scroll, &self.column_scroll, &self.sym_scroll].iter().any(|s| s.animating()) {
            consider(self.now + FRAME_MS);
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
        let m = Metrics::new(width, height);
        if m != self.m {
            self.m = m;
            self.rebuild();
        }
    }

    fn preferred_height(&self, width: f32) -> f32 {
        layout::preferred_height(width) * self.height_scale
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
            PointerPhase::Cancel => {
                let before = self.touches.len();
                self.touches.retain(|t| t.id != e.id);
                r.repaint = before != self.touches.len();
            }
        }
        self.finish(r)
    }

    fn timer(&mut self, now_ms: u64) -> Response {
        self.now = self.now.max(now_ms);
        let now = self.now;
        let mut r = Response::none();
        for i in 0..self.touches.len() {
            let mut t = self.touches[i].clone();
            if t.long_at.is_some_and(|d| d <= now) {
                t.long_at = None;
                if let (Mode::Press, Target::Key(k)) = (&t.mode, &t.target) {
                    if k.action == KeyAction::Space {
                        t.mode = Mode::Voice;
                        r.repaint = true;
                    } else if k.has_long_press() {
                        let (alts, sel) = k.popup();
                        let (popup, cell_w) = self.popup_geometry(k, alts.len(), sel);
                        t.mode = Mode::Long { alts, sel, first: sel, anchor_x: t.x, popup, cell_w };
                        r.repaint = true;
                    }
                }
            }
            if t.repeat_at.is_some_and(|d| d <= now) {
                if t.mode == Mode::Press {
                    r.actions.push(UiAction::Input(Action::Backspace));
                    t.repeats += 1;
                    t.repeat_at = Some(now + repeat_interval(t.repeats));
                } else {
                    t.repeat_at = None;
                }
            }
            self.touches[i] = t;
        }
        let mut scrolled = false;
        for s in [&mut self.strip, &mut self.grid_scroll, &mut self.column_scroll, &mut self.sym_scroll] {
            scrolled |= s.step(now);
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
            self.shift = Shift::Off;
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
