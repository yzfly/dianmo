use dianmo_core::{Action, Candidate, EditKey, Engine, InputController, KeyChord, KeyCode, Schema, Snapshot, TextSink};

use super::*;
use crate::canvas::{Canvas, Color, Rect, TextStyle};
use crate::view::ClipItem;

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

/// Records draw calls; measures text with the same estimate the view uses before painting.
#[derive(Default)]
struct RecCanvas {
    texts: Vec<(String, Rect)>,
    fills: usize,
    clip_depth: i32,
    max_clip_depth: i32,
}

impl Canvas for RecCanvas {
    fn clear(&mut self, _: Color) {}
    fn fill_rect(&mut self, _: Rect, _: f32, _: Color) {
        self.fills += 1;
    }
    fn stroke_rect(&mut self, _: Rect, _: f32, _: f32, _: Color) {}
    fn text(&mut self, text: &str, rect: Rect, _: TextStyle) {
        self.texts.push((text.to_string(), rect));
    }
    fn measure_text(&mut self, text: &str, style: TextStyle) -> f32 {
        estimate_width(text, style.size)
    }
    fn push_clip(&mut self, _: Rect) {
        self.clip_depth += 1;
        self.max_clip_depth = self.max_clip_depth.max(self.clip_depth);
    }
    fn pop_clip(&mut self) {
        self.clip_depth -= 1;
        assert!(self.clip_depth >= 0, "unbalanced clip");
    }
}

/// Phone layout (Surface portrait). Wide-layout tests use [`WIDE`].
const W: f32 = 960.0;
const WIDE: f32 = 1440.0;

struct H {
    v: KeyboardView,
    t: u64,
    last: Response,
}

fn st(chinese: bool, schema: Schema, preedit: &str, words: &[&str]) -> InputState {
    InputState {
        snapshot: Snapshot {
            preedit: preedit.into(),
            candidates: words.iter().map(|w| Candidate::new(*w)).collect(),
            commit: None,
        },
        chinese,
        schema,
    }
}

const NIHAO: &[&str] = &["你好", "拟好", "你", "尼", "泥", "呢", "妮", "倪", "腻", "逆", "匿", "霓", "昵", "拟", "溺", "旎", "睨"];

impl H {
    fn new() -> Self {
        Self::with(st(true, Schema::Pinyin, "", &[]))
    }

    fn with(state: InputState) -> Self {
        Self::sized(W, state)
    }

    /// Surface landscape: the wide layout.
    fn wide(state: InputState) -> Self {
        Self::sized(WIDE, state)
    }

    fn sized(w: f32, state: InputState) -> Self {
        let mut v = KeyboardView::new(KeyboardConfig::default());
        let h = v.preferred_height(w);
        v.resize(w, h);
        let last = v.set_input_state(state);
        Self { v, t: 1000, last }
    }

    fn ev(&mut self, id: u32, phase: PointerPhase, (x, y): (f32, f32)) -> Vec<UiAction> {
        self.last = self.v.pointer(PointerEvent { id, phase, x, y, time_ms: self.t });
        self.last.actions.clone()
    }
    fn down(&mut self, id: u32, p: (f32, f32)) -> Vec<UiAction> {
        self.ev(id, PointerPhase::Down, p)
    }
    fn mov(&mut self, id: u32, p: (f32, f32)) -> Vec<UiAction> {
        self.t += 10;
        self.ev(id, PointerPhase::Move, p)
    }
    fn up(&mut self, id: u32, p: (f32, f32)) -> Vec<UiAction> {
        self.t += 10;
        self.ev(id, PointerPhase::Up, p)
    }
    fn wait(&mut self, ms: u64) -> Vec<UiAction> {
        self.t += ms;
        self.last = self.v.timer(self.t);
        self.last.actions.clone()
    }
    fn at(&self, name: &str) -> (f32, f32) {
        self.v.key_center(name).unwrap_or_else(|| panic!("no key {name}"))
    }
    fn tap(&mut self, name: &str) -> Vec<UiAction> {
        let p = self.at(name);
        let mut a = self.down(1, p);
        a.extend(self.up(1, p));
        a
    }
    fn tap_at(&mut self, p: (f32, f32)) -> Vec<UiAction> {
        let mut a = self.down(1, p);
        a.extend(self.up(1, p));
        a
    }
    fn state(&mut self, s: InputState) -> Response {
        self.last = self.v.set_input_state(s);
        self.last.clone()
    }
    fn paint(&mut self) -> RecCanvas {
        let mut c = RecCanvas::default();
        self.v.paint(&mut c);
        assert_eq!(c.clip_depth, 0);
        c
    }
}

fn input(a: Action) -> UiAction {
    UiAction::Input(a)
}

fn chars(s: &str) -> Vec<UiAction> {
    s.chars().map(|c| input(Action::Char(c))).collect()
}

fn text(s: &str) -> UiAction {
    input(Action::Text(s.into()))
}

// ---------------------------------------------------------------------------------------------
// Keys and gestures
// ---------------------------------------------------------------------------------------------

#[test]
fn letter_fires_on_release() {
    let mut h = H::new();
    let p = h.at("n");
    assert!(h.down(1, p).is_empty());
    assert!(h.last.repaint, "pressed highlight");
    assert_eq!(h.last.timer_ms, Some(LONG_PRESS_MS));
    assert_eq!(h.up(1, p), chars("n"));
    assert_eq!(h.last.timer_ms, None, "no timers when idle");
}

#[test]
fn hit_between_keys_picks_nearest() {
    let mut h = H::new();
    let (qx, qy) = h.at("q");
    let (_, ay) = h.at("a");
    // Slightly above the middle between rows 1 and 2, under q.
    assert_eq!(h.tap_at((qx, qy + (ay - qy) * 0.45)), chars("q"));
    // Far left of row 2 (outside the half-key indent) still hits 'a'.
    assert_eq!(h.tap_at((2.0, ay)), chars("a"));
}

#[test]
fn long_press_enters_secondary() {
    let mut h = H::new();
    let p = h.at("q");
    h.down(1, p);
    h.t += LONG_PRESS_MS;
    assert!(h.wait(0).is_empty());
    assert!(h.last.repaint);
    assert!(matches!(h.v.touches[0].mode, Mode::Long { .. }));
    assert_eq!(h.up(1, p), vec![text("1")]);
}

#[test]
fn swipe_up_enters_secondary() {
    let mut h = H::new();
    let (x, y) = h.at("w");
    h.down(1, (x, y));
    h.mov(1, (x + 3.0, y - 40.0));
    assert_eq!(h.up(1, (x + 3.0, y - 40.0)), vec![text("2")]);
    // Swiping back down cancels it.
    h.down(1, (x, y));
    h.mov(1, (x, y - 40.0));
    h.mov(1, (x, y - 2.0));
    assert_eq!(h.up(1, (x, y - 2.0)), chars("w"));
}

#[test]
fn long_press_alternates_slide_to_pick() {
    let mut h = H::new();
    let (x, y) = h.at("，");
    h.down(1, (x, y));
    h.wait(LONG_PRESS_MS);
    let Mode::Long { alts, sel, cell_w, .. } = h.v.touches[0].mode.clone() else { panic!("no popup") };
    assert_eq!(alts[sel], "。", "secondary pre-selected");
    h.mov(1, (x + cell_w * 1.1, y));
    assert_eq!(h.up(1, (x + cell_w * 1.1, y)), vec![text("？")]);
    // Sliding far left clamps to the first alternate.
    h.down(1, (x, y));
    h.wait(LONG_PRESS_MS);
    h.mov(1, (x - 1000.0, y));
    assert_eq!(h.up(1, (x - 1000.0, y)), vec![text("，")]);
}

#[test]
fn punctuation_follows_language() {
    let mut h = H::new();
    assert_eq!(h.tap("，"), vec![text("，")]);
    h.state(st(false, Schema::Pinyin, "", &[]));
    assert_eq!(h.tap(","), vec![text(",")]);
}

#[test]
fn backspace_fires_on_press_and_repeats_faster() {
    let mut h = H::new();
    let p = h.at("backspace");
    assert_eq!(h.down(1, p), vec![input(Action::Backspace)]);
    assert_eq!(h.last.timer_ms, Some(REPEAT_DELAY_MS));
    let mut intervals = Vec::new();
    for _ in 0..12 {
        let next = h.last.timer_ms.expect("repeat timer");
        intervals.push(next);
        assert_eq!(h.wait(next), vec![input(Action::Backspace)]);
    }
    assert!(intervals.windows(2).all(|w| w[1] <= w[0]), "{intervals:?}");
    assert!(*intervals.last().unwrap() < 60);
    assert!(h.up(1, p).is_empty());
    assert_eq!(h.last.timer_ms, None);
    // A stale timer tick after release does nothing.
    assert!(h.wait(100).is_empty());
    assert!(!h.last.repaint);
}

#[test]
fn backspace_swipe_left_clears_composition() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", NIHAO));
    let (x, y) = h.at("backspace");
    assert_eq!(h.down(1, (x, y)), vec![input(Action::Backspace)]);
    h.mov(1, (x - 100.0, y));
    assert!(h.wait(1000).is_empty(), "no repeat once swiping");
    assert_eq!(h.up(1, (x - 100.0, y)), vec![input(Action::ClearComposition)]);
}

#[test]
fn space_tap_drag_and_hold() {
    let mut h = H::new();
    let (x, y) = h.at("space");
    assert_eq!(h.tap("space"), vec![input(Action::Space)]);

    h.down(1, (x, y));
    let step = h.v.cursor_step();
    let mut acts = h.mov(1, (x + step * 2.5, y));
    acts.extend(h.mov(1, (x + step * 1.5, y)));
    acts.extend(h.up(1, (x + step * 1.5, y)));
    let right = input(Action::Edit(EditKey::Right));
    let left = input(Action::Edit(EditKey::Left));
    assert_eq!(acts, vec![right.clone(), right, left]);

    // Holding the space bar no longer starts voice typing: it turns into a trackpad.
    h.down(1, (x, y));
    h.wait(TRACKPAD_PRESS_MS);
    assert!(h.v.trackpad_active());
    assert_eq!(h.up(1, (x, y)), vec![]);
    assert!(!h.v.trackpad_active());
}

#[test]
fn overlapping_thumbs_keep_order() {
    let mut h = H::new();
    let (n, i) = (h.at("n"), h.at("i"));
    // n down, i down, n up, i up.
    let mut acts = h.down(1, n);
    acts.extend(h.down(2, i));
    acts.extend(h.up(1, n));
    acts.extend(h.up(2, i));
    assert_eq!(acts, chars("ni"));
    // n down, i down, i up first, n up: still "ni" (n fired when i went down).
    let mut acts = h.down(1, n);
    acts.extend(h.down(2, i));
    acts.extend(h.up(2, i));
    acts.extend(h.up(1, n));
    assert_eq!(acts, chars("ni"));
    assert!(h.v.touches.is_empty());
}

#[test]
fn function_key_held_while_typing() {
    let mut h = H::new();
    // Holding backspace with one thumb does not swallow a letter typed by the other.
    let bs = h.at("backspace");
    let a = h.at("a");
    h.down(1, bs);
    assert_eq!(h.tap_at_id(2, a), chars("a"));
    h.up(1, bs);
}

impl H {
    fn tap_at_id(&mut self, id: u32, p: (f32, f32)) -> Vec<UiAction> {
        let mut a = self.down(id, p);
        a.extend(self.up(id, p));
        a
    }
}

#[test]
fn cancel_drops_the_touch() {
    let mut h = H::new();
    let p = h.at("k");
    h.down(1, p);
    assert!(h.ev(1, PointerPhase::Cancel, p).is_empty());
    assert!(h.up(1, p).is_empty());
    assert!(h.v.touches.is_empty());
}

#[test]
fn slide_to_neighbour_fires_key_under_finger() {
    let mut h = H::new();
    let (gx, gy) = h.at("g");
    let (hx, _) = h.at("h");
    h.down(1, (gx, gy));
    h.mov(1, (hx, gy + 4.0));
    assert_eq!(h.up(1, (hx, gy + 4.0)), chars("h"));
}

#[test]
fn shift_once_and_caps_lock() {
    let mut h = H::with(st(false, Schema::Pinyin, "", &[]));
    h.tap("shift");
    assert_eq!(h.tap("a"), chars("A"));
    assert_eq!(h.tap("a"), chars("a"));
    // Double tap locks.
    h.tap("shift");
    h.t += 100;
    h.tap("shift");
    assert_eq!(h.v.latch[Modifier::Shift.index()], Latch::Locked);
    let mut acts = h.tap("b");
    acts.extend(h.tap("c"));
    assert_eq!(acts, chars("BC"));
    h.t += 1000;
    h.tap("shift");
    assert_eq!(h.tap("d"), chars("d"));
    // Slow second tap just turns one-shot shift off.
    h.tap("shift");
    h.t += 1000;
    h.tap("shift");
    assert_eq!(h.v.latch[Modifier::Shift.index()], Latch::Off);
}

#[test]
fn separator_key_while_composing() {
    let mut h = H::with(st(true, Schema::Pinyin, "xian", &["先", "西安"]));
    assert_eq!(h.tap("分词"), vec![input(Action::Char('\''))]);
    h.state(st(true, Schema::Pinyin, "", &[]));
    assert!(h.v.key_center("分词").is_none());
    assert!(h.v.key_center("shift").is_some());
}

// ---------------------------------------------------------------------------------------------
// Candidate bar
// ---------------------------------------------------------------------------------------------

/// Centre of candidate `i` in the strip (after a paint has laid it out).
fn cand_pos(h: &mut H, i: usize) -> (f32, f32) {
    h.paint();
    let (x, w) = h.v.layout_cache.strip[i];
    (x + w / 2.0 - h.v.strip.offset, h.v.m.bar_h * 0.65)
}

#[test]
fn candidate_tap_selects_absolute_index() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", NIHAO));
    let p = cand_pos(&mut h, 2);
    assert_eq!(h.tap_at(p), vec![input(Action::Select(2))]);
}

#[test]
fn candidate_strip_scrolls_with_fling() {
    let many: Vec<&str> = NIHAO.iter().chain(NIHAO).chain(NIHAO).copied().collect();
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", &many));
    let (x, y) = cand_pos(&mut h, 3);
    assert!(h.v.strip.max > 0.0);
    h.down(1, (x, y));
    let mut acts = Vec::new();
    for i in 1..=6 {
        acts.extend(h.mov(1, (x - 40.0 * i as f32, y)));
    }
    acts.extend(h.up(1, (x - 240.0, y)));
    assert!(!acts.iter().any(|a| matches!(a, UiAction::Input(Action::Select(_)))), "{acts:?}");
    assert!(h.v.strip.offset > 200.0);
    assert_eq!(h.last.timer_ms, Some(FRAME_MS), "fling animates");
    let before = h.v.strip.offset;
    let mut frames = 0;
    while let Some(ms) = h.last.timer_ms {
        h.wait(ms);
        frames += 1;
        assert!(frames < 400, "fling never stops");
    }
    assert!(h.v.strip.offset >= before);
    assert!(!h.v.strip.animating());
    // Tapping during a fling stops it without selecting.
    h.v.strip.reset();
}

#[test]
fn strip_scrolled_to_end_requests_more() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", NIHAO));
    let (x, y) = cand_pos(&mut h, 1);
    h.down(1, (x, y));
    let mut acts = Vec::new();
    for i in 1..=30 {
        acts.extend(h.mov(1, (x - 60.0 * i as f32, y)));
    }
    let want = UiAction::WantMoreCandidates { start: NIHAO.len(), count: MORE_BATCH };
    assert_eq!(acts.iter().filter(|a| **a == want).count(), 1, "{acts:?}");
    h.up(1, (x - 1800.0, y));
}

#[test]
fn expand_grid_loads_more_and_selects() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", NIHAO));
    let acts = h.tap("expand");
    assert_eq!(acts, vec![UiAction::WantMoreCandidates { start: NIHAO.len(), count: MORE_BATCH }]);
    assert_eq!(h.v.panel, Panel::Candidates);
    let more: Vec<Candidate> = (0..MORE_BATCH).map(|i| Candidate::new(format!("词{i}"))).collect();
    let r = h.v.set_more_candidates(NIHAO.len(), more);
    assert!(r.repaint);
    assert_eq!(h.v.cands.len(), NIHAO.len() + MORE_BATCH);
    let c = h.paint();
    assert!(c.texts.iter().any(|(t, _)| t == "词0"));
    // Tap the first "more" candidate.
    let area = h.v.grid.unwrap();
    let cell = h.v.layout_cache.grid[NIHAO.len()];
    let p = (area.x + cell.x + cell.w / 2.0, area.y + cell.y + cell.h / 2.0);
    assert!(area.contains(p.0, p.1), "visible without scrolling");
    assert_eq!(h.tap_at(p), vec![input(Action::Select(NIHAO.len()))]);
    assert_eq!(h.v.panel, Panel::Keys);
}

#[test]
fn grid_collapse_and_idle_reset() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni hao", NIHAO));
    h.tap("expand");
    // Collapse via the bar chevron.
    let ch = h.v.bar_keys[0].cell;
    h.tap_at((ch.x + ch.w / 2.0, ch.y + ch.h / 2.0));
    assert_eq!(h.v.panel, Panel::Keys);
    h.tap("expand");
    h.state(st(true, Schema::Pinyin, "", &[]));
    assert_eq!(h.v.panel, Panel::Keys, "commit closes the grid");
}

#[test]
fn idle_toolbar_actions() {
    let mut h = H::new();
    assert_eq!(h.tap("voice"), vec![UiAction::Voice]);
    assert_eq!(h.tap("hide"), vec![UiAction::Hide]);
    assert!(h.v.key_center("expand").is_none());
}

#[test]
fn unchanged_state_does_not_repaint() {
    let mut h = H::with(st(true, Schema::Pinyin, "ni", &["你"]));
    let r = h.state(st(true, Schema::Pinyin, "ni", &["你"]));
    assert_eq!(r, Response::none());
    assert!(h.wait(5000).is_empty());
    assert!(!h.last.repaint);
    assert_eq!(h.last.timer_ms, None);
}

// ---------------------------------------------------------------------------------------------
// T9, panels, layout switching
// ---------------------------------------------------------------------------------------------

#[test]
fn t9_digits_and_column() {
    let mut h = H::with(st(true, Schema::T9, "", &[]));
    assert_eq!(h.tap("ABC"), chars("2"));
    assert_eq!(h.tap("WXYZ"), chars("9"));
    // Idle column: punctuation.
    let (col, _) = h.v.column.unwrap();
    let first = (col.x + col.w / 2.0, col.y + h.v.column_item_h() / 2.0);
    assert_eq!(h.tap_at(first), vec![text("，")]);
    // Composing asks for spellings, then the column picks them.
    let r = h.state(st(true, Schema::T9, "64", &["你", "米"]));
    assert_eq!(r.actions, vec![UiAction::WantT9Spellings]);
    h.v.set_t9_spellings(vec!["ni".into(), "mi".into(), "oh".into()]);
    let second = (first.0, first.1 + h.v.column_item_h());
    assert_eq!(h.tap_at(second), vec![input(Action::PickSpelling("mi".into()))]);
    assert_eq!(h.tap("分词"), vec![input(Action::Char('\''))]);
    assert_eq!(h.tap("重输"), vec![input(Action::ClearComposition)]);
}

#[test]
fn layout_menu_switches_schema_and_language() {
    let mut h = H::new();
    h.tap("layout");
    assert_eq!(h.v.panel, Panel::Menu);
    assert_eq!(h.tap("鹤"), vec![input(Action::SetSchema(Schema::Shuangpin))]);
    assert_eq!(h.v.panel, Panel::Keys);
    h.state(st(true, Schema::Shuangpin, "", &[]));
    assert_eq!(h.v.layout(), Layout::Shuangpin);
    assert!(h.v.keys.iter().any(|k| k.sub.as_deref() == Some("iang uang")));

    h.tap("layout");
    assert_eq!(h.tap("En"), vec![input(Action::ToggleChinese)]);
    h.state(st(false, Schema::Shuangpin, "", &[]));
    assert_eq!(h.v.layout(), Layout::English);
    assert!(h.v.keys.iter().all(|k| k.sub.is_none()));

    h.tap("layout");
    assert_eq!(h.tap("九"), vec![input(Action::SetSchema(Schema::T9)), input(Action::ToggleChinese)]);

    // Tapping outside the tiles closes the menu without acting.
    h.tap("layout");
    let area = h.v.m.keys_area();
    assert!(h.tap_at((area.x + 4.0, area.y + 4.0)).is_empty());
    assert_eq!(h.v.panel, Panel::Keys);
}

#[test]
fn external_schema_change_is_followed() {
    let mut h = H::new();
    h.state(st(true, Schema::T9, "", &[]));
    assert_eq!(h.v.layout(), Layout::T9);
    assert!(h.v.column.is_some());
    assert!(h.v.key_center("ABC").is_some());
}

#[test]
fn toggle_key_and_theme() {
    let mut h = H::new();
    assert_eq!(h.tap("toggle"), vec![input(Action::ToggleChinese)]);
    h.tap("layout");
    let tile = h.v.keys.iter().find(|k| k.action == KeyAction::ToggleTheme).unwrap().cell;
    let acts = h.tap_at((tile.x + tile.w / 2.0, tile.y + tile.h / 2.0));
    assert_eq!(h.v.theme(), ThemeKind::Dark);
    assert_eq!(acts, vec![UiAction::ThemeChanged(ThemeKind::Dark)]);
}

#[test]
fn host_helpers_panels_height_downcast() {
    let mut h = H::new();
    let base = h.v.preferred_height(W);
    h.v.set_height_scale(1.2);
    assert!((h.v.preferred_height(W) - base * 1.2).abs() < 0.01);
    h.v.set_height_scale(9.0);
    assert_eq!(h.v.height_scale(), 1.5);
    assert!(h.v.show_numbers());
    assert_eq!(h.v.panel, Panel::Numbers);
    assert!(!h.v.show_numbers());
    assert!(h.v.show_letters());
    assert_eq!(h.v.panel, Panel::Keys);
    let view: &mut dyn View = &mut h.v;
    let kv = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()).expect("downcast");
    kv.set_theme(ThemeKind::Dark);
    assert_eq!(h.v.theme(), ThemeKind::Dark);
}

#[test]
fn numbers_and_symbols_panels() {
    let mut h = H::new();
    h.tap("123");
    assert_eq!(h.v.panel, Panel::Numbers);
    assert_eq!(h.tap("5"), vec![text("5")]);
    assert_eq!(h.tap("0"), vec![text("0")]);
    let (col, _) = h.v.column.unwrap();
    assert_eq!(h.tap_at((col.x + col.w / 2.0, col.y + 5.0)), vec![text("+")]);
    h.tap("返回");
    assert_eq!(h.v.panel, Panel::Keys);

    h.tap("符号");
    assert_eq!(h.v.panel, Panel::Symbols(SymTab::Chinese));
    let g = h.v.grid.unwrap();
    assert_eq!(h.tap_at((g.x + 5.0, g.y + 5.0)), vec![text("，")]);
    h.tap("英文");
    assert_eq!(h.tap_at((g.x + 5.0, g.y + 5.0)), vec![text(",")]);
    h.tap("表情");
    assert_eq!(h.tap_at((g.x + 5.0, g.y + 5.0)), vec![text("😀")]);
    // The symbol grid scrolls vertically.
    let p = (g.x + g.w / 2.0, g.y + g.h - 5.0);
    h.down(1, p);
    h.mov(1, (p.0, p.1 - 60.0));
    let acts = h.up(1, (p.0, p.1 - 60.0));
    assert!(acts.is_empty());
    assert!(h.v.sym_scroll.offset > 0.0);
    h.tap("返回");
    assert_eq!(h.v.panel, Panel::Keys);
}

// ---------------------------------------------------------------------------------------------
// Painting and sizing
// ---------------------------------------------------------------------------------------------

#[test]
fn paints_every_panel_in_both_themes() {
    for theme in [ThemeKind::Light, ThemeKind::Dark] {
        for (schema, chinese) in [(Schema::Pinyin, true), (Schema::Shuangpin, true), (Schema::T9, true), (Schema::Pinyin, false)] {
            for preedit in ["", "ni"] {
                let mut h = H::with(st(chinese, schema, preedit, if preedit.is_empty() { &[] } else { NIHAO }));
                h.v.set_theme(theme);
                let c = h.paint();
                assert!(c.fills > 30);
                if !preedit.is_empty() {
                    assert!(c.texts.iter().any(|(t, _)| t == "你好"));
                    assert!(c.texts.iter().any(|(t, _)| t == "ni"));
                }
                for panel in [Panel::Numbers, Panel::Symbols(SymTab::Emoji), Panel::Menu, Panel::Candidates] {
                    h.v.set_panel(panel);
                    h.paint();
                }
            }
        }
    }
}

#[test]
fn pressed_key_draws_bubble() {
    let mut h = H::new();
    let base = h.paint().texts.iter().filter(|(t, _)| t == "g").count();
    h.down(1, h.at("g"));
    let c = h.paint();
    let gs: Vec<_> = c.texts.iter().filter(|(t, _)| t == "g").collect();
    assert_eq!(gs.len(), base + 1, "bubble repeats the letter");
    let (_, key) = gs[0];
    let (_, bubble) = gs.last().unwrap();
    assert!(bubble.y < key.y, "bubble above the key");
}

#[test]
fn sizes_are_sensible() {
    let v = KeyboardView::default();
    // Phone layout (narrow windows, portrait).
    let phone = v.preferred_height(1000.0);
    assert!(phone > 230.0 && phone <= 0.38 * 960.0, "{phone}");
    let port = v.preferred_height(960.0);
    assert!(port > 230.0 && port <= phone, "{port}");
    let m = Metrics::new(1000.0, phone);
    assert!(!m.wide && m.rows == 4);
    assert!((48.0..=70.0).contains(&m.row_h), "{}", m.row_h);
    assert!((42.0..=54.0).contains(&m.bar_h));
    // Wide layout (Surface landscape): five rows within 40% of a 960-DIP screen.
    let land = v.preferred_height(1440.0);
    assert!(land > 320.0 && land <= 0.4 * 960.0, "{land}");
    let m = Metrics::new(1440.0, land);
    assert!(m.wide && m.rows == 5);
    assert!((54.0..=60.0).contains(&m.row_h), "{}", m.row_h);
}

#[test]
fn flypy_table_is_complete() {
    for c in 'a'..='z' {
        assert!(layout::flypy_hint(c).is_some(), "{c}");
    }
    assert_eq!(layout::flypy_hint('l'), Some("iang uang"));
    assert_eq!(layout::flypy_hint('v'), Some("zh ui"));
}

// ---------------------------------------------------------------------------------------------
// Wide layout (Surface landscape): digit row, PC keys, modifiers, edit area
// ---------------------------------------------------------------------------------------------

fn key(c: KeyChord) -> UiAction {
    input(Action::Key(c))
}

fn idle() -> InputState {
    st(true, Schema::Pinyin, "", &[])
}

#[test]
fn wide_layout_has_five_rows_and_a_digit_row() {
    let h = H::wide(idle());
    assert!(h.v.is_wide());
    assert_eq!(h.v.m.rows, 5);
    // The digit row sits above q, and the digits are plain keys (no long-press needed).
    let (_, y1) = h.at("1");
    let (_, yq) = h.at("q");
    assert!(y1 < yq);
    let q = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('q')).unwrap();
    assert_ne!(q.corner.as_deref(), Some("1"), "letters no longer carry digits");
    // Letter keys stay at least ~80 DIPs wide with the edit area on.
    assert!(q.cell.w >= 80.0, "{}", q.cell.w);
    for name in ["esc", "tab", "ctrl", "win", "alt", "fn", "left", "right", "up", "down", "copy", "paste", "undo"] {
        assert!(h.v.key_center(name).is_some(), "{name}");
    }
    // The phone layout at the same scale has none of this.
    let p = H::new();
    assert!(!p.v.is_wide());
    assert!(p.v.key_center("ctrl").is_none());
    assert!(p.v.key_center("1").is_none());
}

#[test]
fn wide_digit_row_types_digits_and_shifted_symbols() {
    let mut h = H::wide(idle());
    let mut acts = h.tap("1");
    acts.extend(h.tap("0"));
    assert_eq!(acts, vec![text("1"), text("0")]);
    h.tap("shift");
    assert_eq!(h.tap("！"), vec![text("！")], "shift swaps to the shifted symbol");
    assert_eq!(h.tap("1"), vec![text("1")], "one-shot shift is used up");
    h.state(st(false, Schema::Pinyin, "", &[]));
    h.tap("shift");
    assert_eq!(h.tap("!"), vec![text("!")]);
    // Chinese punctuation is full-width, English half-width.
    assert!(h.v.key_center("，").is_none());
    h.state(idle());
    assert_eq!(h.tap("，"), vec![text("，")]);
    assert_eq!(h.tap("【"), vec![text("【")]);
}

#[test]
fn sticky_ctrl_sends_chords() {
    let mut h = H::wide(idle());
    h.tap("ctrl");
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Once);
    let c = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('c')).unwrap();
    assert_eq!(c.sub.as_deref(), Some("复制"), "shortcut hint");
    assert_eq!(h.tap("s"), vec![key(KeyChord::ctrl('s'))]);
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Off);
    assert_eq!(h.tap("s"), chars("s"));
    // Double tap locks: Ctrl is really held from the first key until unlocked, the keys go
    // without it (DESIGN「锁定 = 真按住」).
    h.tap("ctrl");
    h.t += 100;
    assert!(h.tap("ctrl").is_empty(), "nothing is sent before a key");
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Locked);
    let mut acts = h.tap("c");
    acts.extend(h.tap("v"));
    acts.extend(h.tap("1"));
    acts.extend(h.tap("left"));
    let plain = |c: char| key(KeyChord::key(KeyCode::Char(c)));
    assert_eq!(
        acts,
        vec![kd(KeyCode::Ctrl), plain('c'), plain('v'), plain('1'), key(KeyChord::key(KeyCode::Edit(EditKey::Left)))]
    );
    h.t += 1000;
    assert_eq!(h.tap("ctrl"), vec![ku(KeyCode::Ctrl)]);
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Off);
    // Ctrl + Shift stack; full-width punctuation sends its physical key.
    h.tap("ctrl");
    h.t += 1000;
    h.tap("shift");
    assert_eq!(h.tap("T"), vec![key(KeyChord { shift: true, ..KeyChord::ctrl('t') })]);
    h.tap("ctrl");
    assert_eq!(h.tap("，"), vec![key(KeyChord::ctrl(','))]);
}

#[test]
fn held_modifier_chords_with_another_finger() {
    let mut h = H::wide(idle());
    let ctrl = h.at("ctrl");
    h.down(1, ctrl);
    assert_eq!(h.tap_at_id(2, h.at("z")), vec![key(KeyChord::UNDO)]);
    assert_eq!(h.tap_at_id(2, h.at("y")), vec![key(KeyChord::REDO)]);
    assert!(h.up(1, ctrl).is_empty());
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Off, "a used hold does not latch");
    assert_eq!(h.tap("z"), chars("z"));
    // Holding shift uppercases while held.
    let sh = h.at("shift");
    h.down(1, sh);
    assert_eq!(h.tap_at_id(2, h.at("A")), chars("A"));
    h.up(1, sh);
    assert_eq!(h.tap("a"), chars("a"));
}

/// Double-taps `name` (a modifier) so that it is locked.
fn lock(h: &mut H, name: &str) -> Vec<UiAction> {
    h.t += 1000;
    let mut acts = h.tap(name);
    h.t += 100;
    acts.extend(h.tap(name));
    acts
}

#[test]
fn locked_alt_stays_down_for_alt_tab() {
    let mut h = H::wide(idle());
    let tab = key(KeyChord::key(KeyCode::Edit(EditKey::Tab)));
    assert!(lock(&mut h, "alt").is_empty(), "a lock alone sends nothing");
    assert_eq!(h.v.latch[Modifier::Alt.index()], Latch::Locked);
    // Alt goes down before the first Tab and stays down: the task switcher stays open.
    assert_eq!(h.tap("tab"), vec![kd(KeyCode::Alt), tab.clone()]);
    assert_eq!(h.tap("tab"), vec![tab.clone()]);
    assert_eq!(h.v.latch[Modifier::Alt.index()], Latch::Locked);
    // Unlocking releases Alt (the switch completes).
    h.t += 1000;
    assert_eq!(h.tap("alt"), vec![ku(KeyCode::Alt)]);
    assert_eq!(h.v.latch[Modifier::Alt.index()], Latch::Off);
    assert_eq!(h.tap("tab"), vec![input(Action::Edit(EditKey::Tab))]);
    // Lock and unlock without a key: nothing at all (no lone Alt = menu bar).
    assert!(lock(&mut h, "alt").is_empty());
    h.t += 1000;
    assert!(h.tap("alt").is_empty());
    // One-shot Alt is still a one-off chord.
    h.t += 1000;
    h.tap("alt");
    assert_eq!(h.tap("tab"), vec![key(KeyChord { alt: true, ..KeyChord::key(KeyCode::Edit(EditKey::Tab)) })]);
    // So is a held Alt with another finger.
    let alt = h.at("alt");
    h.down(2, alt);
    let mut acts = h.tap_at_id(1, h.at("tab"));
    acts.extend(h.tap_at_id(1, h.at("tab")));
    acts.extend(h.up(2, alt));
    let chord = key(KeyChord { alt: true, ..KeyChord::key(KeyCode::Edit(EditKey::Tab)) });
    assert_eq!(acts, vec![chord.clone(), chord]);
}

#[test]
fn locked_alt_end_to_end_keeps_alt_down_in_the_sink() {
    let mut h = H::wide(idle());
    let mut ctl = InputController::new(Fake { schema: Schema::Pinyin, raw: String::new() }, Sink::default());
    let mut run = |acts: Vec<UiAction>| {
        for a in acts {
            if let UiAction::Input(a) = a {
                ctl.handle(a);
            }
        }
        std::mem::take(&mut ctl.sink_mut().0)
    };
    run(lock(&mut h, "alt"));
    let tab = format!("<{:?}>", KeyChord::key(KeyCode::Edit(EditKey::Tab)));
    assert_eq!(run(h.tap("tab")), format!("<Alt true>{tab}"));
    assert_eq!(run(h.tap("tab")), tab, "no Alt up between the Tabs");
    h.t += 1000;
    assert_eq!(run(h.tap("alt")), "<Alt false>");
}

#[test]
fn locked_ctrl_alt_win_and_shift_combine() {
    let mut h = H::wide(idle());
    // Locked Ctrl + one-shot Shift: Ctrl stays down, Shift goes with the chord.
    lock(&mut h, "ctrl");
    h.tap("shift");
    assert_eq!(h.tap("T"), vec![kd(KeyCode::Ctrl), key(KeyChord { shift: true, ..KeyChord::key(KeyCode::Char('t')) })]);
    // A toolbar chord that has Ctrl anyway does not press it again (nor release it).
    let undo = h.tap("undo");
    assert_eq!(undo, vec![key(KeyChord::key(KeyCode::Char('z')))]);
    h.t += 1000;
    assert_eq!(h.tap("ctrl"), vec![ku(KeyCode::Ctrl)]);
    // Locked Win (long-press twice) is held the same way.
    let win = h.at("win");
    for _ in 0..2 {
        h.down(1, win);
        h.wait(LONG_PRESS_MS);
        h.up(1, win);
    }
    assert_eq!(h.v.latch[Modifier::Win.index()], Latch::Locked);
    let right = key(KeyChord::key(KeyCode::Edit(EditKey::Right)));
    let r = h.at("right");
    assert_eq!(h.down(1, r), vec![kd(KeyCode::Win), right.clone()]);
    h.up(1, r);
    assert_eq!(h.down(1, r), vec![right]);
    h.up(1, r);
    h.t += 1000;
    assert_eq!(h.tap("win"), vec![ku(KeyCode::Win)]);
    // Locked Shift stays caps lock: nothing is held, letters are typed in capitals.
    assert!(lock(&mut h, "shift").is_empty());
    assert_eq!(h.tap("A"), chars("A"));
    assert!(h.v.mods_down.iter().all(|d| !d));
}

#[test]
fn locked_modifier_goes_up_when_leaving() {
    let tab = || key(KeyChord::key(KeyCode::Edit(EditKey::Tab)));
    let mut h = H::wide(idle());
    lock(&mut h, "alt");
    assert_eq!(h.tap("tab"), vec![kd(KeyCode::Alt), tab()]);
    // Hiding the keyboard: Alt goes up before the hide; the lock stays lit and presses again.
    assert_eq!(h.tap("hide"), vec![ku(KeyCode::Alt), UiAction::Hide]);
    assert_eq!(h.v.latch[Modifier::Alt.index()], Latch::Locked);
    assert_eq!(h.tap("tab"), vec![kd(KeyCode::Alt), tab()]);
    // Another panel (symbols, layout menu).
    assert_eq!(h.tap("symbols"), vec![ku(KeyCode::Alt)]);
    h.tap("back");
    assert_eq!(h.tap("tab"), vec![kd(KeyCode::Alt), tab()]);
    assert_eq!(h.tap("layout"), vec![ku(KeyCode::Alt)]);
    h.tap("layout");
    // Another layout (中 / 英).
    h.tap("tab");
    let r = h.state(st(false, Schema::Pinyin, "", &[]));
    assert_eq!(r.actions, vec![ku(KeyCode::Alt)]);
    // Typed text: Alt goes up first.
    h.tap("tab");
    let (x, y) = h.at("w");
    h.down(1, (x, y));
    h.mov(1, (x, y - 40.0));
    let acts = h.up(1, (x, y - 40.0));
    assert!(matches!(&acts[..], [a, UiAction::Input(Action::Text(_))] if *a == ku(KeyCode::Alt)), "{acts:?}");
    // The host: keyboard hidden / focus moved by touch / input mode changed from the tray.
    h.tap("tab");
    assert_eq!(h.v.release_locked_mods().actions, vec![ku(KeyCode::Alt)]);
    assert!(h.v.release_locked_mods().actions.is_empty());
    // Voice releases the lock itself.
    h.tap("tab");
    assert_eq!(h.tap("voice"), vec![ku(KeyCode::Alt), UiAction::Voice]);
    assert_eq!(h.v.latch[Modifier::Alt.index()], Latch::Off);
    // Into the 电脑键盘 with Alt down (host switch after the release, or the view's own switch).
    lock(&mut h, "alt");
    h.tap("tab");
    let mut r = Response::none();
    assert!(h.v.set_pc(true, &mut r));
    assert_eq!(r.actions, vec![ku(KeyCode::Alt)]);
    assert!(h.v.mods_down.iter().all(|d| !d));
}

#[test]
fn win_tap_presses_win_and_long_press_latches() {
    let mut h = H::wide(idle());
    assert_eq!(h.tap("win"), vec![key(KeyChord::win_alone())]);
    assert_eq!(h.v.latch[Modifier::Win.index()], Latch::Off);
    let p = h.at("win");
    h.down(1, p);
    h.wait(LONG_PRESS_MS);
    assert!(h.up(1, p).is_empty());
    assert_eq!(h.v.latch[Modifier::Win.index()], Latch::Once);
    let v = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('v')).unwrap();
    assert_eq!(v.sub.as_deref(), Some("剪贴板"));
    assert_eq!(h.tap("v"), vec![key(KeyChord { win: true, ..KeyChord::key(KeyCode::Char('v')) })]);
    // With another modifier lit, Win joins the chord: Win+Shift+S.
    h.tap("shift");
    h.tap("win");
    assert_eq!(h.tap("S"), vec![key(KeyChord { win: true, shift: true, ..KeyChord::key(KeyCode::Char('s')) })]);
}

#[test]
fn fn_layer_function_keys() {
    let mut h = H::wide(idle());
    h.tap("fn");
    assert_eq!(h.tap("F5"), vec![key(KeyChord::key(KeyCode::F(5)))]);
    assert!(h.v.key_center("F5").is_none(), "one-shot Fn");
    h.tap("fn");
    assert_eq!(h.tap("F12"), vec![key(KeyChord::key(KeyCode::F(12)))]);
    h.tap("fn");
    let del = h.at("del");
    assert_eq!(h.down(1, del), vec![input(Action::Edit(EditKey::Delete))]);
    h.up(1, del);
    assert!(h.v.key_center("backspace").is_some(), "Fn released after one key");
    // Alt + F4.
    h.tap("alt");
    h.tap("fn");
    assert_eq!(h.tap("F4"), vec![key(KeyChord { alt: true, ..KeyChord::key(KeyCode::F(4)) })]);
}

#[test]
fn tab_and_arrows() {
    let mut h = H::wide(idle());
    assert_eq!(h.tap("tab"), vec![input(Action::Edit(EditKey::Tab))]);
    // Holding Tab sends Esc instead, once.
    let p = h.at("tab");
    h.down(1, p);
    assert_eq!(h.wait(LONG_PRESS_MS), vec![input(Action::Edit(EditKey::Escape))]);
    assert!(h.up(1, p).is_empty());
    assert_eq!(h.tap("esc"), vec![input(Action::Edit(EditKey::Escape))]);
    // Arrows fire on press and repeat.
    let l = h.at("left");
    assert_eq!(h.down(1, l), vec![input(Action::Edit(EditKey::Left))]);
    assert_eq!(h.wait(REPEAT_DELAY_MS), vec![input(Action::Edit(EditKey::Left))]);
    assert!(h.up(1, l).is_empty());
    // Shift + arrow selects.
    h.tap("shift");
    let r = h.at("right");
    assert_eq!(h.down(1, r), vec![key(KeyChord { shift: true, ..KeyChord::key(KeyCode::Edit(EditKey::Right)) })]);
    h.up(1, r);
}

#[test]
fn edit_area_and_toolbar() {
    let mut h = H::wide(idle());
    assert_eq!(h.tap("copy"), vec![key(KeyChord::COPY), UiAction::CheckCopied]);
    assert_eq!(h.tap("delword"), vec![key(KeyChord::DELETE_WORD)]);
    assert_eq!(h.tap("clear"), vec![key(KeyChord::SELECT_ALL), key(KeyChord::key(KeyCode::Edit(EditKey::Delete)))]);
    // 行首 / 行尾 moved to the Fn layer and the selection bar; 选择 and 剪贴板 took their place.
    assert!(h.v.key_center("home").is_none());
    assert!(h.v.key_center("select").is_some() && h.v.key_center("clipboard").is_some());
    // With the edit area on, the toolbar has no edit tools.
    assert!(h.v.bar_keys.iter().all(|k| !matches!(k.action, KeyAction::Chord(_))));
    let q_before = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('q')).unwrap().cell.w;
    assert!(h.v.set_edit_area(false));
    assert!(h.v.key_center("clear").is_none());
    let q_after = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('q')).unwrap().cell.w;
    assert!(q_after > q_before, "width goes back to the letters");
    // The toolbar now carries undo … paste.
    let undo = h.v.bar_keys.iter().find(|k| k.action == KeyAction::Chord(KeyChord::UNDO)).unwrap().cell;
    assert_eq!(h.tap_at((undo.x + undo.w / 2.0, undo.y + undo.h / 2.0)), vec![key(KeyChord::UNDO)]);
    // Composing: the edit keys commit through the controller (the chord follows the commit).
    h.state(st(true, Schema::Pinyin, "ni", NIHAO));
    assert!(h.v.bar_keys.iter().all(|k| !matches!(k.action, KeyAction::Chord(_))));
}

#[test]
fn toolbar_arrows_hold_to_line_ends_and_pc_keys_panel() {
    let mut h = H::new();
    let r = h.v.bar_keys.iter().find(|k| k.action == KeyAction::Edit(EditKey::Right)).unwrap().cell;
    let p = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(h.tap_at(p), vec![input(Action::Edit(EditKey::Right))]);
    h.down(1, p);
    assert_eq!(h.wait(LONG_PRESS_MS), vec![input(Action::Edit(EditKey::End))]);
    assert!(h.up(1, p).is_empty());
    // 电脑键 panel.
    h.tap("pc");
    assert_eq!(h.v.panel, Panel::PcKeys);
    assert_eq!(h.tap("F11"), vec![key(KeyChord::key(KeyCode::F(11)))]);
    h.tap("ctrl");
    h.tap("shift");
    assert_eq!(h.tap("esc"), vec![key(KeyChord { ctrl: true, shift: true, ..KeyChord::key(KeyCode::Edit(EditKey::Escape)) })]);
    h.tap("返回");
    assert_eq!(h.v.panel, Panel::Keys);
}

#[test]
fn wide_t9_and_numbers_have_direct_digits() {
    let mut h = H::wide(st(true, Schema::T9, "", &[]));
    assert_eq!(h.tap("ABC"), chars("2"), "T9 keys still compose");
    assert_eq!(h.tap("7"), vec![text("7")], "the digit pad types digits");
    assert_eq!(h.tap("0"), vec![text("0")]);
    assert!(h.v.column.is_some());
    assert!(h.v.key_center("pc").is_some(), "T9 reaches PC keys from the toolbar");
    let mut h = H::wide(idle());
    h.v.show_numbers();
    assert_eq!(h.tap("5"), vec![text("5")]);
    let up = h.at("up");
    assert_eq!(h.down(1, up), vec![input(Action::Edit(EditKey::Up))]);
    h.up(1, up);
    // Fixed width: digits don't stretch to the full keyboard width.
    let five = h.v.keys.iter().find(|k| k.label == "5").unwrap().cell;
    assert!(five.w <= 170.0, "{}", five.w);
    // The 123 panel is also a tab of the symbol panel.
    h.v.show_letters();
    h.tap("符号");
    h.tap("123");
    assert_eq!(h.v.panel, Panel::Numbers);
}

#[test]
fn wide_candidates_bigger_and_paged() {
    let many: Vec<&str> = NIHAO.iter().cycle().take(120).copied().collect();
    let mut h = H::wide(st(true, Schema::Pinyin, "ni hao", &many));
    assert!(h.v.cand_style().size >= 22.0);
    h.tap("expand");
    h.paint();
    assert!(h.v.grid_scroll.max > 0.0);
    h.tap("pagedown");
    let page = h.v.grid_scroll.offset;
    assert!(page > 0.0);
    h.tap("pagedown");
    let second = h.v.grid_scroll.offset;
    assert!(second > page);
    h.tap("pageup");
    assert!((h.v.grid_scroll.offset - (second - page).max(0.0)).abs() < 0.5);
}

#[test]
fn wide_height_respects_setting_and_minimum_row() {
    let mut v = KeyboardView::default();
    let base = v.preferred_height(WIDE);
    v.set_height_scale(1.3);
    assert!(v.preferred_height(WIDE) > base);
    v.set_height_scale(0.7);
    let low = v.preferred_height(WIDE);
    v.resize(WIDE, low);
    assert!(v.m.row_h >= layout::WIDE_MIN_ROW - 0.5, "{}", v.m.row_h);
}

#[test]
fn paints_wide_panels() {
    for theme in [ThemeKind::Light, ThemeKind::Dark] {
        for (schema, chinese) in [(Schema::Pinyin, true), (Schema::Shuangpin, true), (Schema::T9, true), (Schema::Pinyin, false)] {
            let mut h = H::wide(st(chinese, schema, "", &[]));
            h.v.set_theme(theme);
            assert!(h.paint().fills > 60);
            h.tap(if schema == Schema::T9 { "pc" } else { "ctrl" });
            h.paint();
            for panel in [Panel::Numbers, Panel::Symbols(SymTab::Emoji), Panel::Menu, Panel::Candidates, Panel::PcKeys] {
                h.v.set_panel(panel);
                h.paint();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// End to end with the real input controller
// ---------------------------------------------------------------------------------------------

struct Fake {
    schema: Schema,
    raw: String,
}

impl Fake {
    fn snap(&self, commit: Option<String>) -> Snapshot {
        let cands = match self.raw.as_str() {
            "" => vec![],
            "nihao" => vec![Candidate::new("你好"), Candidate::new("拟好")],
            _ => vec![Candidate::new(self.raw.clone())],
        };
        Snapshot { preedit: self.raw.clone(), candidates: cands, commit }
    }
}

impl Engine for Fake {
    fn schema(&self) -> Schema {
        self.schema
    }
    fn set_schema(&mut self, schema: Schema) {
        self.schema = schema;
        self.raw.clear();
    }
    fn input(&mut self, c: char) -> Snapshot {
        self.raw.push(c);
        self.snap(None)
    }
    fn backspace(&mut self) -> Snapshot {
        self.raw.pop();
        self.snap(None)
    }
    fn select(&mut self, index: usize) -> Snapshot {
        let word = self.snap(None).candidates.get(index).map(|c| c.text.clone());
        if word.is_some() {
            self.raw.clear();
        }
        self.snap(word)
    }
    fn commit_raw(&mut self) -> Snapshot {
        let raw = std::mem::take(&mut self.raw);
        self.snap(Some(raw))
    }
    fn clear(&mut self) -> Snapshot {
        self.raw.clear();
        self.snap(None)
    }
    fn candidates(&mut self, _: usize, _: usize) -> Vec<Candidate> {
        vec![]
    }
    fn snapshot(&mut self) -> Snapshot {
        self.snap(None)
    }
}

#[derive(Default)]
struct Sink(String);

impl TextSink for Sink {
    fn commit_text(&mut self, text: &str) {
        self.0.push_str(text);
    }
    fn send_key(&mut self, key: EditKey) {
        self.0.push_str(&format!("<{key:?}>"));
    }
    fn send_chord(&mut self, c: KeyChord) {
        self.0.push_str(&format!("<{c:?}>"));
    }
    fn key_event(&mut self, k: KeyCode, down: bool) {
        self.0.push_str(&format!("<{k:?} {down}>"));
    }
}

#[test]
fn end_to_end_typing() {
    let mut h = H::new();
    let mut ctl = InputController::new(Fake { schema: Schema::Pinyin, raw: String::new() }, Sink::default());
    let mut run = |h: &mut H, acts: Vec<UiAction>| {
        for a in acts {
            if let UiAction::Input(a) = a {
                ctl.handle(a);
                let s = InputState { snapshot: ctl.state().clone(), chinese: ctl.is_chinese(), schema: ctl.schema() };
                h.state(s);
            }
        }
        ctl.sink_mut().0.clone()
    };
    for c in ["n", "i", "h", "a", "o"] {
        let a = h.tap(c);
        run(&mut h, a);
    }
    assert_eq!(h.v.snapshot.preedit, "nihao");
    let p = cand_pos(&mut h, 1);
    let a = h.tap_at(p);
    assert_eq!(run(&mut h, a), "拟好");
    let a = h.tap("，");
    assert_eq!(run(&mut h, a), "拟好，");
    let a = h.tap("toggle");
    run(&mut h, a);
    assert_eq!(h.v.layout(), Layout::English);
    h.tap("shift");
    let a = h.tap("o");
    let a2 = h.tap("k");
    run(&mut h, a);
    assert_eq!(run(&mut h, a2), "拟好，Ok");
}

#[test]
fn fullwidth_punctuation_is_optically_centred() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    let moved = crate::draw::punct_centered("，", r, 20.0);
    assert!(moved.x > r.x && moved.y < r.y);
    assert_eq!(crate::draw::punct_centered("？", r, 20.0), r);
    assert_eq!(crate::draw::punct_centered("，，", r, 20.0), r);
}

#[test]
fn end_to_end_wide_digits_and_ctrl() {
    let mut h = H::wide(idle());
    let mut ctl = InputController::new(Fake { schema: Schema::Pinyin, raw: String::new() }, Sink::default());
    let mut run = |h: &mut H, acts: Vec<UiAction>| {
        for a in acts {
            if let UiAction::Input(a) = a {
                ctl.handle(a);
                let s = InputState { snapshot: ctl.state().clone(), chinese: ctl.is_chinese(), schema: ctl.schema() };
                h.state(s);
            }
        }
        ctl.sink_mut().0.clone()
    };
    // Digits from the digit row commit the composition first, then type themselves.
    let mut out = String::new();
    for c in ["n", "i", "h", "a", "o", "2", "0", "2", "6"] {
        let a = h.tap(c);
        out = run(&mut h, a);
    }
    assert_eq!(out, "你好2026");
    // Edit keys commit a pending composition before the chord.
    let a = h.tap("n");
    run(&mut h, a);
    let a = h.tap("selectall");
    assert_eq!(run(&mut h, a), format!("你好2026n<{:?}>", KeyChord::SELECT_ALL));
}

// ---------------------------------------------------------------------------------------------
// 电脑键盘 (TODO #31)
// ---------------------------------------------------------------------------------------------

fn kd(k: KeyCode) -> UiAction {
    input(Action::KeyDown(k))
}

fn ku(k: KeyCode) -> UiAction {
    input(Action::KeyUp(k))
}

fn pc_harness() -> H {
    let mut h = H::wide(idle());
    h.tap("layout");
    assert_eq!(h.tap("pcmode"), vec![UiAction::PcKeyboard(true)]);
    assert_eq!(h.v.layout(), Layout::Pc);
    h
}

#[test]
fn pc_keyboard_layout_and_bar() {
    let mut h = pc_harness();
    // Six rows: F row, digits, qwerty, home row, shift row, bottom row; a thin bar.
    let rows: std::collections::BTreeSet<i32> = h.v.keys.iter().map(|k| k.cell.y as i32).collect();
    assert!(rows.len() >= 6, "{rows:?}");
    assert!(h.v.m.bar_h < 40.0);
    for name in ["esc", "F1", "F12", "prtsc", "del", "`", "1", "=", "backspace", "tab", "q", "\\", "caps", "a", "'", "enter", "shift", "z", "/", "ctrl", "win", "alt", "fn", "space", "left", "up", "down", "right"] {
        assert!(h.v.key_center(name).is_some(), "{name}");
    }
    // The bar: back, voice, hide; no candidates, no edit area.
    let bar: Vec<_> = h.v.bar_keys.iter().map(|k| k.action.clone()).collect();
    assert_eq!(bar, vec![KeyAction::PcBack, KeyAction::Voice, KeyAction::Hide]);
    assert!(h.v.key_center("copy").is_none());
    // Shifted characters in the corner; Shift shows them on the face.
    let one = h.v.keys.iter().find(|k| k.action == KeyAction::Raw(KeyCode::Char('1'))).unwrap();
    assert_eq!(one.top_left.as_deref(), Some("!"));
    h.tap("shift");
    assert!(h.v.key_center("!").is_some() && h.v.key_center("A").is_some());
    h.paint();
    // Height does not change (no re-docking).
    let h0 = h.v.preferred_height(WIDE);
    assert_eq!(h.tap("pcback"), vec![UiAction::PcKeyboard(false)]);
    assert_eq!(h.v.layout(), Layout::Pinyin);
    assert_eq!(h.v.preferred_height(WIDE), h0);
    assert!(h.v.m.bar_h > 50.0);
}

#[test]
fn pc_keys_go_down_and_up_and_repeat() {
    let mut h = pc_harness();
    let a = h.at("a");
    assert_eq!(h.down(1, a), vec![kd(KeyCode::Char('a'))]);
    assert_eq!(h.wait(REPEAT_DELAY_MS), vec![kd(KeyCode::Char('a'))]);
    assert_eq!(h.wait(PC_REPEAT_MS), vec![kd(KeyCode::Char('a'))]);
    assert_eq!(h.wait(PC_REPEAT_MS), vec![kd(KeyCode::Char('a'))]);
    assert_eq!(h.up(1, a), vec![ku(KeyCode::Char('a'))]);
    assert_eq!(h.last.timer_ms, None, "no timer once nothing is held");
    // Arrows, Enter, F keys are raw keys too.
    let enter = h.at("enter");
    assert_eq!(h.down(1, enter), vec![kd(KeyCode::Edit(EditKey::Enter))]);
    assert_eq!(h.up(1, enter), vec![ku(KeyCode::Edit(EditKey::Enter))]);
    let f5 = h.at("F5");
    assert_eq!(h.down(1, f5), vec![kd(KeyCode::F(5))]);
    assert_eq!(h.up(1, f5), vec![ku(KeyCode::F(5))]);
    // Two keys at once (rollover): both go down, each comes up on its own release.
    let (s, d) = (h.at("s"), h.at("d"));
    assert_eq!(h.down(1, s), vec![kd(KeyCode::Char('s'))]);
    assert_eq!(h.down(2, d), vec![kd(KeyCode::Char('d'))]);
    assert_eq!(h.up(1, s), vec![ku(KeyCode::Char('s'))]);
    assert_eq!(h.up(2, d), vec![ku(KeyCode::Char('d'))]);
    // A cancelled touch releases its key.
    h.down(1, a);
    assert_eq!(h.ev(1, PointerPhase::Cancel, a), vec![ku(KeyCode::Char('a'))]);
}

#[test]
fn pc_modifiers_are_real_keys() {
    let mut h = pc_harness();
    let one = h.at("1");
    // Tapped Shift applies to the next key: down before it, up after it.
    assert!(h.tap("shift").is_empty());
    assert_eq!(h.down(1, one), vec![kd(KeyCode::Shift), kd(KeyCode::Char('1'))]);
    assert_eq!(h.up(1, one), vec![ku(KeyCode::Char('1')), ku(KeyCode::Shift)]);
    assert_eq!(h.down(1, one), vec![kd(KeyCode::Char('1'))], "one-shot");
    h.up(1, one);
    // Held Ctrl + another finger: Ctrl goes down with the first key and up when lifted.
    let (ctrl, c, v) = (h.at("ctrl"), h.at("c"), h.at("v"));
    assert!(h.down(2, ctrl).is_empty());
    assert_eq!(h.down(1, c), vec![kd(KeyCode::Ctrl), kd(KeyCode::Char('c'))]);
    assert_eq!(h.up(1, c), vec![ku(KeyCode::Char('c'))]);
    assert_eq!(h.down(1, v), vec![kd(KeyCode::Char('v'))], "Ctrl is still down");
    assert_eq!(h.up(1, v), vec![ku(KeyCode::Char('v'))]);
    assert_eq!(h.up(2, ctrl), vec![ku(KeyCode::Ctrl)]);
    assert!(h.v.latch.iter().all(|l| *l == Latch::Off), "a used hold does not latch");
    // Alt held while tapping Tab twice: one Alt press around both (task switcher stays open).
    let (alt, tab) = (h.at("alt"), h.at("tab"));
    h.down(2, alt);
    let mut acts = h.down(1, tab);
    acts.extend(h.up(1, tab));
    acts.extend(h.down(1, tab));
    acts.extend(h.up(1, tab));
    acts.extend(h.up(2, alt));
    let t = KeyCode::Edit(EditKey::Tab);
    assert_eq!(acts, vec![kd(KeyCode::Alt), kd(t), ku(t), kd(t), ku(t), ku(KeyCode::Alt)]);
    // A long-pressed Shift alone is really pressed (switches the target IME's 中/英).
    let shift = h.at("shift");
    assert!(h.down(2, shift).is_empty());
    assert_eq!(h.wait(LONG_PRESS_MS), vec![kd(KeyCode::Shift)]);
    assert_eq!(h.up(2, shift), vec![ku(KeyCode::Shift)]);
    assert!(h.v.latch.iter().all(|l| *l == Latch::Off));
    // Locked Ctrl: down before the first key, held until unlocked.
    h.tap("ctrl");
    h.t += 100;
    assert!(h.tap("ctrl").is_empty());
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Locked);
    let a = h.at("a");
    assert_eq!(h.down(1, a), vec![kd(KeyCode::Ctrl), kd(KeyCode::Char('a'))]);
    assert_eq!(h.up(1, a), vec![ku(KeyCode::Char('a'))]);
    assert_eq!(h.down(1, a), vec![kd(KeyCode::Char('a'))]);
    assert_eq!(h.up(1, a), vec![ku(KeyCode::Char('a'))]);
    h.t += 500;
    assert_eq!(h.tap("ctrl"), vec![ku(KeyCode::Ctrl)]);
    assert_eq!(h.v.latch[Modifier::Ctrl.index()], Latch::Off);
    // Locked Shift stays per key (it is caps lock).
    h.tap("shift");
    h.t += 100;
    h.tap("shift");
    assert_eq!(h.down(1, a), vec![kd(KeyCode::Shift), kd(KeyCode::Char('a'))]);
    assert_eq!(h.up(1, a), vec![ku(KeyCode::Char('a')), ku(KeyCode::Shift)]);
    h.t += 500;
    assert!(h.tap("shift").is_empty());
    // Win alone opens Start; Win held + D = Win+D.
    assert_eq!(h.tap("win"), vec![key(KeyChord::win_alone())]);
    let (win, d) = (h.at("win"), h.at("d"));
    h.down(2, win);
    assert_eq!(h.down(1, d), vec![kd(KeyCode::Win), kd(KeyCode::Char('d'))]);
    h.up(1, d);
    assert_eq!(h.up(2, win), vec![ku(KeyCode::Win)]);
}

#[test]
fn pc_caps_fn_and_leaving_releases_keys() {
    let mut h = pc_harness();
    assert_eq!(h.tap("caps"), vec![kd(KeyCode::CapsLock), ku(KeyCode::CapsLock)]);
    assert!(h.v.pc_caps);
    assert!(h.v.key_center("Q").is_some(), "caps shows capitals");
    h.paint();
    h.tap("caps");
    assert!(!h.v.pc_caps);
    // Fn: arrows become Home/End/PgUp/PgDn, Del becomes Insert.
    h.tap("fn");
    let home = h.at("home");
    assert_eq!(h.down(1, home), vec![kd(KeyCode::Edit(EditKey::Home))]);
    h.up(1, home);
    assert!(h.v.key_center("left").is_some(), "Fn was one-shot");
    // Leaving the layout while a key and a modifier are held releases them.
    let (shift, a) = (h.at("shift"), h.at("a"));
    h.down(3, shift);
    assert_eq!(h.down(2, a), vec![kd(KeyCode::Shift), kd(KeyCode::Char('a'))]);
    let back = h.v.bar_keys.iter().find(|k| k.action == KeyAction::PcBack).unwrap().cell;
    let acts = h.tap_at((back.x + back.w / 2.0, back.y + back.h / 2.0));
    assert!(acts.contains(&ku(KeyCode::Char('a'))) && acts.contains(&ku(KeyCode::Shift)), "{acts:?}");
    assert_eq!(acts.last(), Some(&UiAction::PcKeyboard(false)));
    assert!(h.up(2, a).is_empty() && h.up(3, shift).is_empty(), "already released");
    // Host API (tray / settings).
    assert!(h.v.set_pc_keyboard(true));
    assert!(!h.v.set_pc_keyboard(true));
    assert!(h.v.pc_keyboard());
}

#[test]
fn pc_end_to_end_key_sequence() {
    let mut h = pc_harness();
    let mut ctl = InputController::new(Fake { schema: Schema::Pinyin, raw: String::new() }, Sink::default());
    let mut run = |acts: Vec<UiAction>| {
        for a in acts {
            if let UiAction::Input(a) = a {
                ctl.handle(a);
            }
        }
        std::mem::take(&mut ctl.sink_mut().0)
    };
    // n i → the app's IME gets real keys; nothing is composed by us.
    assert_eq!(run(h.tap("n")), "<Char('n') true><Char('n') false>");
    // Ctrl+A (tap Ctrl, tap A).
    run(h.tap("ctrl"));
    assert_eq!(run(h.tap("a")), "<Ctrl true><Char('a') true><Char('a') false><Ctrl false>");
}

// ---------------------------------------------------------------------------------------------
// Trackpad, selection, clipboard (TODO #32)
// ---------------------------------------------------------------------------------------------

fn trackpad_steps(acts: &[UiAction]) -> (i32, i32) {
    let (mut x, mut y) = (0, 0);
    for a in acts {
        let e = match a {
            UiAction::Input(Action::Edit(e)) => *e,
            UiAction::Input(Action::Key(KeyChord { key: Some(KeyCode::Edit(e)), .. })) => *e,
            _ => continue,
        };
        match e {
            EditKey::Right => x += 1,
            EditKey::Left => x -= 1,
            EditKey::Down => y += 1,
            EditKey::Up => y -= 1,
            _ => {}
        }
    }
    (x, y)
}

#[test]
fn trackpad_moves_caret_and_accelerates() {
    let mut h = H::wide(idle());
    let sp = h.at("space");
    h.down(1, sp);
    h.wait(TRACKPAD_PRESS_MS);
    assert!(h.v.trackpad_active());
    h.paint();
    // Slow drag right: about one character per 14 DIPs.
    let mut acts = Vec::new();
    for i in 1..=10 {
        h.t += 40;
        acts.extend(h.ev(1, PointerPhase::Move, (sp.0 + i as f32 * 7.0, sp.1)));
    }
    let slow = trackpad_steps(&acts);
    assert!((4..=6).contains(&slow.0) && slow.1 == 0, "{slow:?}");
    assert!(acts.iter().all(|a| matches!(a, UiAction::Input(Action::Edit(_)))), "no Shift before selecting");
    // The same distance fast moves further.
    let x0 = sp.0 + 70.0;
    let mut acts = Vec::new();
    for i in 1..=10 {
        h.t += 8;
        acts.extend(h.ev(1, PointerPhase::Move, (x0 - i as f32 * 7.0, sp.1)));
    }
    let fast = trackpad_steps(&acts);
    assert!(fast.0 < -slow.0 * 2, "{fast:?} vs {slow:?}");
    // Vertical drags move lines.
    let mut acts = Vec::new();
    for i in 1..=6 {
        h.t += 40;
        acts.extend(h.ev(1, PointerPhase::Move, (x0 - 70.0, sp.1 - i as f32 * 10.0)));
    }
    assert!(trackpad_steps(&acts).1 <= -2);
    // Lifting without selecting: back to the keys, no selection bar.
    h.up(1, sp);
    assert!(!h.v.trackpad_active() && !h.v.selecting());
}

#[test]
fn trackpad_second_finger_selects_then_selection_bar() {
    let mut h = H::wide(idle());
    let sp = h.at("space");
    h.down(1, sp);
    h.wait(TRACKPAD_PRESS_MS);
    // Another finger anywhere taps: selecting starts (no key fires).
    let q = h.at("q");
    assert!(h.tap_at_id(2, q).is_empty());
    assert!(h.v.pad_select);
    let mut acts = Vec::new();
    for i in 1..=4 {
        h.t += 40;
        acts.extend(h.ev(1, PointerPhase::Move, (sp.0 + i as f32 * 7.0, sp.1)));
    }
    let shift_right = key(KeyChord { shift: true, ..KeyChord::key(KeyCode::Edit(EditKey::Right)) });
    assert!(!acts.is_empty() && acts.iter().all(|a| *a == shift_right), "{acts:?}");
    // A second tap extends by a word.
    let word = key(KeyChord { ctrl: true, shift: true, ..KeyChord::key(KeyCode::Edit(EditKey::Right)) });
    assert_eq!(h.tap_at_id(2, q), vec![word]);
    h.up(1, sp);
    // Something was selected: the selection bar is up.
    assert!(h.v.selecting());
    assert!(h.v.key_center("sel_copy").is_some());
    assert_eq!(h.tap("sel_copy"), vec![key(KeyChord::COPY)]);
    assert!(!h.v.selecting());
}

#[test]
fn selection_mode_bar_extends_with_shift() {
    let mut h = H::wide(idle());
    assert!(h.tap("select").is_empty());
    assert!(h.v.selecting());
    h.paint();
    let sel = |e: EditKey, ctrl: bool| key(KeyChord { ctrl, shift: true, ..KeyChord::key(KeyCode::Edit(e)) });
    // Bar buttons fire on press and repeat.
    let wl = h.at("sel_wordleft");
    assert_eq!(h.down(1, wl), vec![sel(EditKey::Left, true)]);
    assert_eq!(h.wait(REPEAT_DELAY_MS), vec![sel(EditKey::Left, true)]);
    h.up(1, wl);
    for (name, e) in [("sel_left", EditKey::Left), ("sel_right", EditKey::Right), ("sel_up", EditKey::Up), ("sel_down", EditKey::Down), ("sel_home", EditKey::Home), ("sel_end", EditKey::End)] {
        assert_eq!(h.tap(name), vec![sel(e, false)], "{name}");
    }
    assert_eq!(h.tap("selectall"), vec![key(KeyChord::SELECT_ALL)]);
    // The keyboard's own arrows also extend the selection.
    let l = h.at("left");
    assert_eq!(h.down(1, l), vec![sel(EditKey::Left, false)]);
    h.up(1, l);
    // Right-hand buttons act and leave selection mode.
    assert_eq!(h.tap("sel_cut"), vec![key(KeyChord::CUT)]);
    assert!(!h.v.selecting());
    h.tap("select");
    assert_eq!(h.tap("sel_delete"), vec![input(Action::Edit(EditKey::Delete))]);
    h.tap("select");
    assert_eq!(h.tap("sel_paste"), vec![key(KeyChord::PASTE)]);
    h.tap("select");
    assert!(h.tap("sel_done").is_empty());
    assert!(!h.v.selecting());
    // Typing ends selection mode too; the host can open it (复制 with nothing selected).
    assert!(h.v.enter_select_mode());
    assert!(!h.v.enter_select_mode());
    h.tap("x");
    assert!(!h.v.selecting());
    // Not while composing.
    h.state(st(true, Schema::Pinyin, "ni", NIHAO));
    assert!(!h.v.enter_select_mode());
}

fn clips(texts: &[&str]) -> Vec<ClipItem> {
    texts.iter().enumerate().map(|(i, t)| ClipItem { id: 10 + i as u64, text: t.to_string(), pinned: false }).collect()
}

#[test]
fn copy_shows_toast_and_clipboard_bar() {
    let mut h = H::wide(idle());
    assert!(h.v.set_clips(clips(&["最新复制的一段文字", "hello", "第三条"])));
    assert!(!h.v.set_clips(clips(&["最新复制的一段文字", "hello", "第三条"])));
    assert!(h.v.key_center("clip0").is_none(), "toolbar until something is copied");
    h.t += 10;
    let r = h.v.notify_copied(h.t);
    assert_eq!(r.timer_ms, Some(TOAST_MS), "one-shot timer for the toast");
    assert!(h.v.toast.is_some());
    let c = h.paint();
    assert!(c.texts.iter().any(|(t, _)| t == "已复制"));
    assert!(c.texts.iter().any(|(t, _)| t == "hello"));
    // Tap a card: paste it.
    assert_eq!(h.tap("clip1"), vec![UiAction::Paste("hello".into())]);
    // The toast goes after a second; nothing else is scheduled.
    h.wait(TOAST_MS);
    assert!(h.v.toast.is_none());
    assert_eq!(h.last.timer_ms, None);
    // Close button: back to the toolbar.
    assert!(h.tap("clipclose").is_empty());
    assert!(h.v.key_center("clip0").is_none());
    // Typing also ends the clipboard bar.
    h.v.notify_copied(h.t);
    assert!(h.v.key_center("clip0").is_some());
    h.tap("k");
    assert!(h.v.key_center("clip0").is_none());
    // Hidden keyboard: reset.
    h.v.notify_copied(h.t);
    assert!(h.v.reset_transient());
    assert!(h.v.toast.is_none() && h.v.key_center("clip0").is_none());
    // Paste key preview.
    assert!(h.v.set_paste_preview(Some("  一二三\n四五六七八九 ")));
    let paste = h.v.keys.iter().find(|k| k.action == KeyAction::Chord(KeyChord::PASTE)).unwrap();
    assert_eq!(paste.sub.as_deref(), Some("一二三 四五…"));
    assert!(h.v.set_paste_preview(None));
    assert!(h.v.keys.iter().find(|k| k.action == KeyAction::Chord(KeyChord::PASTE)).unwrap().sub.is_none());
}

#[test]
fn clipboard_panel_paste_pin_delete_clear() {
    let mut h = H::wide(idle());
    let mut items = clips(&["一", "二", "三"]);
    items[2].pinned = true;
    h.v.set_clips(items);
    h.tap("clipboard");
    assert_eq!(h.v.panel, Panel::Clipboard);
    let c = h.paint();
    assert!(c.texts.iter().any(|(t, _)| t == "三"));
    // Pinned entries first: position 0 is 三.
    assert_eq!(h.tap("clip0"), vec![UiAction::Paste("三".into())]);
    assert_eq!(h.v.panel, Panel::Keys, "pasting returns to the keys");
    h.tap("clipboard");
    // Long-press: 固定 / 删除 buttons on the card.
    let p = h.at("clip1");
    h.down(1, p);
    assert!(h.wait(LONG_PRESS_MS).is_empty());
    assert!(h.up(1, p).is_empty());
    assert_eq!(h.v.clip_menu, Some(10));
    h.paint();
    let cell = h.v.clip_grid_cell(h.v.grid.unwrap(), 1);
    let pin = (cell.x + cell.w * 0.25, cell.y + cell.h / 2.0);
    assert_eq!(h.tap_at(pin), vec![UiAction::PinClip { id: 10, pinned: true }]);
    assert_eq!(h.v.clip_menu, None);
    h.down(1, p);
    h.wait(LONG_PRESS_MS);
    h.up(1, p);
    let del = (cell.x + cell.w * 0.75, cell.y + cell.h / 2.0);
    assert_eq!(h.tap_at(del), vec![UiAction::DeleteClip(10)]);
    // A tap elsewhere just closes the menu.
    h.down(1, p);
    h.wait(LONG_PRESS_MS);
    h.up(1, p);
    assert!(h.tap("clip0").is_empty());
    assert_eq!(h.v.clip_menu, None);
    assert_eq!(h.tap("clearclips"), vec![UiAction::ClearClips]);
    h.v.set_clips(Vec::new());
    let c = h.paint();
    assert!(c.texts.iter().any(|(t, _)| t.contains("复制的文字")));
    h.tap("back");
    assert_eq!(h.v.panel, Panel::Keys);
}

#[test]
fn narrow_toolbar_has_select_and_clipboard() {
    let mut h = H::new();
    assert!(h.v.key_center("clipboard").is_some());
    assert!(h.v.key_center("select").is_some());
    h.tap("select");
    assert!(h.v.selecting());
    h.paint();
    // Every selection bar button fits on a 960-wide bar.
    let right = h.v.bar_keys.iter().map(|k| k.cell.x + k.cell.w).fold(0.0, f32::max);
    assert!(right <= W);
    let cells: Vec<Rect> = h.v.bar_keys.iter().map(|k| k.cell).collect();
    for (i, a) in cells.iter().enumerate() {
        for b in &cells[i + 1..] {
            assert!(a.x + a.w <= b.x + 0.5 || b.x + b.w <= a.x + 0.5, "overlap {a:?} {b:?}");
        }
    }
}

#[test]
fn clip_preview_is_one_short_line() {
    assert_eq!(crate::clip_preview("a\r\n  b\tc", 10), "a b c");
    assert_eq!(crate::clip_preview("一二三四五六七", 4), "一二三四…");
    assert_eq!(crate::clip_preview("   ", 4), "");
}

// ---------------------------------------------------------------------------------------------
// Voice (TODO #29): mic key state, modifiers released, 语音球 tile, longer toasts
// ---------------------------------------------------------------------------------------------

#[test]
fn voice_key_releases_latched_modifiers() {
    let mut h = H::wide(idle());
    h.tap("ctrl");
    assert!(h.v.mods().on(Modifier::Ctrl));
    assert_eq!(h.tap("voice"), vec![UiAction::Voice]);
    assert!(!h.v.mods().on(Modifier::Ctrl), "Ctrl must not combine with the engine's hotkey");
    // 电脑键盘: a held modifier is really down; tapping the mic sends it up first.
    let mut h = pc_harness();
    let shift = h.at("shift");
    h.down(3, shift);
    h.wait(400); // long press: Shift really down
    let voice = h.at("voice");
    let acts = h.tap_at(voice);
    assert_eq!(acts.last(), Some(&UiAction::Voice), "{acts:?}");
    assert!(acts.contains(&UiAction::Input(Action::KeyUp(KeyCode::Shift))), "{acts:?}");
    assert!(h.up(3, shift).is_empty(), "already released");
}

#[test]
fn voice_active_paints_the_mic_and_voice_ball_tile() {
    let mut h = H::wide(idle());
    let before = h.paint().fills;
    assert!(h.v.set_voice_active(true));
    assert!(!h.v.set_voice_active(true));
    assert!(h.v.voice_active());
    assert_eq!(h.paint().fills, before + 1, "red disc behind the mic");
    // Layout menu → 语音球.
    h.tap("layout");
    assert_eq!(h.tap("voiceball"), vec![UiAction::VoiceBall]);
    assert_eq!(h.v.panel, Panel::Keys);
}

#[test]
fn t9_keys_by_digit_for_tests() {
    let mut h = H::wide(st(true, Schema::T9, "", &[]));
    assert_eq!(h.tap("t9_6"), vec![UiAction::Input(Action::Char('6'))]);
    assert_ne!(h.v.key_center("t9_6"), h.v.key_center("6"), "the digit pad's 6 is another key");
}

#[test]
fn long_toasts_stay_longer() {
    let mut h = H::wide(idle());
    let r = h.v.show_toast("已复制", h.t);
    assert_eq!(r.timer_ms, Some(TOAST_MS));
    let r = h.v.show_toast("微信输入法没有开始收音，已改用系统语音输入", h.t);
    assert!(r.timer_ms.unwrap() > 3000 && r.timer_ms.unwrap() <= 4000, "{r:?}");
}

#[test]
fn voice_mode_offers_an_exit_button() {
    // Wide (edit area on), phone and 电脑键盘 bars all show 「退出语音模式」 in voice mode.
    for mut h in [H::wide(idle()), H::with(idle()), pc_harness()] {
        assert!(h.v.key_center("exitvoice").is_none());
        assert!(h.v.set_voice_mode(true));
        assert!(!h.v.set_voice_mode(true));
        let (x, _) = h.v.key_center("exitvoice").expect("exit button in the bar");
        assert!(x > 0.0);
        assert_eq!(h.tap("exitvoice"), vec![UiAction::VoiceBall]);
        // No two bar keys overlap.
        let cells: Vec<Rect> = h.v.bar_keys.iter().map(|k| k.cell).collect();
        for (i, a) in cells.iter().enumerate() {
            for b in &cells[i + 1..] {
                assert!(a.x + a.w <= b.x + 0.5 || b.x + b.w <= a.x + 0.5, "{cells:?}");
            }
        }
        h.paint();
    }
    // The layout menu's tile turns voice mode off too.
    let mut h = H::wide(idle());
    h.v.set_voice_mode(true);
    h.tap("layout");
    let tile = h.v.keys.iter().find(|k| k.action == KeyAction::VoiceBall).unwrap();
    assert_eq!(tile.sub.as_deref(), Some("退出语音球"));
    assert!(tile.selected);
    assert_eq!(h.tap("voiceball"), vec![UiAction::VoiceBall]);
    assert!(h.v.set_voice_mode(false));
    assert!(h.v.key_center("exitvoice").is_none());
}

#[test]
fn key_popup_setting_hides_the_bubble() {
    let mut h = H::new();
    assert!(h.v.key_popup());
    assert!(h.v.set_key_popup(false));
    assert!(!h.v.set_key_popup(false), "no change");
    let base = h.paint().texts.iter().filter(|(t, _)| t == "g").count();
    h.down(1, h.at("g"));
    let n = h.paint().texts.iter().filter(|(t, _)| t == "g").count();
    assert_eq!(n, base, "no bubble");
    assert_eq!(h.up(1, h.at("g")), vec![input(Action::Char('g'))], "the key still types");
}

#[test]
fn long_press_delay_is_a_setting() {
    let mut h = H::new();
    assert_eq!(h.v.long_press_ms(), LONG_PRESS_MS);
    assert!(h.v.set_long_press_ms(500));
    assert!(h.v.set_long_press_ms(5), "clamped, changed");
    assert_eq!(h.v.long_press_ms(), 150);
    h.v.set_long_press_ms(500);
    let p = h.at("q");
    h.down(1, p);
    assert_eq!(h.last.timer_ms, Some(500));
    h.wait(LONG_PRESS_MS);
    assert!(!matches!(h.v.touches[0].mode, Mode::Long { .. }), "not yet at 350 ms");
    h.wait(500 - LONG_PRESS_MS);
    assert!(matches!(h.v.touches[0].mode, Mode::Long { .. }));
}

#[test]
fn toolbar_gear_opens_settings() {
    for w in [W, WIDE] {
        let mut h = H::sized(w, st(true, Schema::Pinyin, "", &[]));
        assert!(h.v.key_center("settings").is_some(), "gear on the idle toolbar ({w})");
        assert_eq!(h.tap("settings"), vec![UiAction::OpenSettings]);
    }
    // Not while composing (the candidate bar replaces the toolbar).
    let h = H::with(st(true, Schema::Pinyin, "ni", NIHAO));
    assert!(h.v.key_center("settings").is_none());
}

// ---------------------------------------------------------------------------------------------
// v0.2 settings: 按键音, 全角标点, 候选字号, 双拼方案
// ---------------------------------------------------------------------------------------------

#[test]
fn key_sound_sends_a_click_first_on_press() {
    let mut h = H::new();
    assert!(!h.v.key_sound(), "off by default");
    assert!(h.down(1, h.at("g")).is_empty(), "no click while off");
    h.up(1, h.at("g"));
    assert!(h.v.set_key_sound(true));
    assert!(!h.v.set_key_sound(true), "no change");
    assert_eq!(h.down(1, h.at("g")), vec![UiAction::KeyClick(KeyClick::Char)], "on press, before the key fires");
    assert_eq!(h.up(1, h.at("g")), vec![input(Action::Char('g'))], "release only types");
    // Function keys sound different; press-fired keys get the click before their input.
    assert_eq!(h.down(1, h.at("backspace")), vec![UiAction::KeyClick(KeyClick::Func), input(Action::Backspace)]);
    h.up(1, h.at("backspace"));
    assert_eq!(h.down(1, h.at("space"))[0], UiAction::KeyClick(KeyClick::Func));
    h.up(1, h.at("space"));
    assert_eq!(h.down(1, h.at("，"))[0], UiAction::KeyClick(KeyClick::Char));
    h.up(1, h.at("，"));
    // The toolbar is not a key.
    assert!(h.down(1, h.at("settings")).is_empty());
}

#[test]
fn full_width_punctuation_setting_switches_the_keys() {
    for w in [W, WIDE] {
        let mut h = H::sized(w, idle());
        assert!(h.v.full_width_punct());
        assert!(h.v.key_center("，").is_some(), "{w}");
        assert!(h.v.set_full_width_punct(false));
        assert!(!h.v.set_full_width_punct(false));
        assert!(h.v.key_center("，").is_none(), "{w}: no full-width comma");
        assert_eq!(h.tap(","), vec![text(",")], "{w}");
        // English is ASCII either way; 中文 with the setting on is full-width again.
        h.v.set_full_width_punct(true);
        assert_eq!(h.tap("，"), vec![text("，")]);
    }
    // The 九宫格 punctuation column follows it too.
    let mut h = H::with(st(true, Schema::T9, "", &[]));
    assert_eq!(h.v.column_items()[0], "，");
    h.v.set_full_width_punct(false);
    assert_eq!(h.v.column_items()[0], ",");
}

#[test]
fn candidate_size_scales_the_candidates() {
    for w in [W, WIDE] {
        let mut h = H::sized(w, st(true, Schema::Pinyin, "ni", NIHAO));
        let size = |h: &H| h.v.cand_style().size;
        let width = |h: &mut H| {
            h.paint();
            h.v.layout_cache.strip[0].1
        };
        let (s0, w0) = (size(&h), width(&mut h));
        assert!(h.v.set_candidate_size(CandidateSize::ExtraLarge));
        assert!(!h.v.set_candidate_size(CandidateSize::ExtraLarge));
        assert!((size(&h) - s0 * 1.3).abs() < 0.01, "{w}");
        let w1 = width(&mut h);
        assert!(w1 > w0, "{w}: the strip was laid out again ({w1} vs {w0})");
        // The text still fits the bar.
        assert!(size(&h) < h.v.m.bar_h * 0.7, "{w}: {} in a {} bar", size(&h), h.v.m.bar_h);
        h.v.set_candidate_size(CandidateSize::Small);
        assert!(size(&h) < s0);
        assert!(width(&mut h) <= w0);
        let canvas = h.paint();
        assert!(canvas.texts.iter().any(|(t, _)| t == "你好"));
    }
}

#[test]
fn every_shuangpin_scheme_labels_every_key() {
    for scheme in ShuangpinScheme::ALL {
        for c in 'a'..='z' {
            assert!(layout::shuangpin_hint(scheme, c).is_some(), "{scheme:?} {c}");
        }
        assert_eq!(layout::shuangpin_hint(scheme, ';').is_some(), scheme.uses_semicolon(), "{scheme:?}");
        // u / i / v are sh / ch / zh in all four.
        assert_eq!(layout::shuangpin_hint(scheme, 'u'), Some("sh"));
        assert_eq!(layout::shuangpin_hint(scheme, 'i'), Some("ch"));
        assert!(layout::shuangpin_hint(scheme, 'v').unwrap().starts_with("zh"));
    }
    assert_eq!(layout::shuangpin_hint(ShuangpinScheme::Xiaohe, 'k'), Some("ing uai"));
    assert_eq!(layout::shuangpin_hint(ShuangpinScheme::Ziranma, 'y'), Some("ing uai"));
    assert_eq!(layout::shuangpin_hint(ShuangpinScheme::Microsoft, ';'), Some("ing"));
    assert_eq!(layout::shuangpin_hint(ShuangpinScheme::Sogou, 'd'), Some("iang uang"));
}

#[test]
fn shuangpin_scheme_changes_the_key_faces() {
    let sub = |h: &H, c: char| h.v.keys.iter().find(|k| k.action == KeyAction::Letter(c)).and_then(|k| k.sub.clone());
    let mut h = H::with(st(true, Schema::Shuangpin, "", &[]));
    assert_eq!(h.v.shuangpin(), ShuangpinScheme::Xiaohe);
    assert_eq!(sub(&h, 'k').as_deref(), Some("ing uai"));
    assert!(h.v.key_center("；").is_none(), "小鹤 has no ； key");
    assert!(h.v.set_shuangpin(ShuangpinScheme::Ziranma));
    assert!(!h.v.set_shuangpin(ShuangpinScheme::Ziranma));
    assert_eq!(sub(&h, 'k').as_deref(), Some("ao"));
    assert!(h.v.keys.iter().any(|k| k.action == KeyAction::Space && k.label == "自然码双拼"), "space bar names the scheme");
    // 微软: a tenth key in the a–l row types ； (the controller turns it into the final ing).
    h.v.set_shuangpin(ShuangpinScheme::Microsoft);
    let semi = h.v.keys.iter().find(|k| k.label == "；").expect("； key").clone();
    assert_eq!(semi.sub.as_deref(), Some("ing"));
    let a = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('a')).unwrap().cell;
    let l = h.v.keys.iter().find(|k| k.action == KeyAction::Letter('l')).unwrap().cell;
    assert!((semi.cell.y - a.y).abs() < 0.1 && semi.cell.x > l.x, "after l in the a–l row");
    assert!(a.x <= h.v.m.pad_x + 0.1, "the row starts at the left edge");
    assert_eq!(h.tap("；"), vec![text("；")]);
    // Other layouts never show it, nor the finals.
    let mut p = H::new();
    p.v.set_shuangpin(ShuangpinScheme::Microsoft);
    assert!(p.v.key_center("；").is_none());
    assert!(sub(&p, 'k').is_none());
    // Wide layout: the ； key already exists and gets the hint.
    let mut w = H::wide(st(true, Schema::Shuangpin, "", &[]));
    w.v.set_shuangpin(ShuangpinScheme::Sogou);
    let semi = w.v.keys.iter().find(|k| k.label == "；").unwrap();
    assert_eq!(semi.sub.as_deref(), Some("ing"));
    assert_eq!(w.v.keys.iter().filter(|k| k.label == "；").count(), 1);
    // The layout menu names the scheme.
    w.tap("layout");
    assert!(w.v.keys.iter().any(|k| k.sub.as_deref() == Some("搜狗双拼")));
}

#[test]
fn url_and_email_fields_get_quick_keys() {
    let mut kb = KeyboardView::default();
    kb.resize(1440.0, 520.0);
    assert!(kb.key_center(".com").is_none());
    assert!(kb.set_field_hint(FieldHint::Url));
    assert!(!kb.set_field_hint(FieldHint::Url));
    for k in [".com", "/", "www."] {
        assert!(kb.key_center(k).is_some(), "{k}");
    }
    assert!(kb.set_field_hint(FieldHint::Email));
    assert!(kb.key_center("@").is_some());
    assert!(kb.key_center("@qq.com").is_some());
    assert!(kb.key_center("/").is_none());
    // Portrait Surface (960 DIP): fewer quick keys, the most useful ones stay.
    kb.resize(960.0, 420.0);
    assert!(kb.key_center(".com").is_some());
    assert!(kb.set_field_hint(FieldHint::Text));
    assert!(kb.key_center("@qq.com").is_none());
}

#[test]
fn pc_keyboard_is_taller_in_portrait() {
    let mut kb = KeyboardView::default();
    let phone = kb.preferred_height(960.0);
    let wide = kb.preferred_height(1440.0);
    assert!(kb.set_pc_keyboard(true));
    let h = kb.preferred_height(960.0);
    assert!(h > phone + 40.0, "{h} vs {phone}");
    kb.resize(960.0, h);
    assert!(kb.m.row_h >= 44.0, "row {}", kb.m.row_h);
    // Landscape keeps the wide height.
    assert_eq!(kb.preferred_height(1440.0), wide);
}
