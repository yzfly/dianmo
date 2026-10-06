use dianmo_core::{Action, Candidate, EditKey, Engine, InputController, Schema, Snapshot, TextSink};

use super::*;
use crate::canvas::{Canvas, Color, Rect, TextStyle};

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

const W: f32 = 1440.0;

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
        let mut v = KeyboardView::new(KeyboardConfig::default());
        let h = v.preferred_height(W);
        v.resize(W, h);
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

    h.down(1, (x, y));
    h.wait(VOICE_PRESS_MS);
    assert_eq!(h.up(1, (x, y)), vec![UiAction::Voice]);
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
    assert_eq!(h.v.shift, Shift::Locked);
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
    assert_eq!(h.v.shift, Shift::Off);
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
    h.tap_at((tile.x + tile.w / 2.0, tile.y + tile.h / 2.0));
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
    let land = v.preferred_height(1440.0);
    assert!(land > 300.0 && land <= 0.38 * 960.0, "{land}");
    let port = v.preferred_height(960.0);
    assert!(port > 230.0 && port < land, "{port}");
    let m = Metrics::new(1440.0, land);
    assert!((56.0..=70.0).contains(&m.row_h), "{}", m.row_h);
    assert!((42.0..=54.0).contains(&m.bar_h));
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
