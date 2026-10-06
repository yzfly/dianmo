//! Keyboard layouts: data tables (easy to tweak) and the builders that turn them into positioned
//! [`Key`]s for the current size and state.

use dianmo_core::Schema;

use crate::canvas::Rect;

/// Segoe MDL2 Assets code points (present on Windows 10; Segoe Fluent Icons keeps them).
pub mod icon {
    pub const MIC: &str = "\u{E720}";
    pub const KEYBOARD: &str = "\u{E765}";
    pub const BACK: &str = "\u{E72B}";
    pub const CHEVRON_DOWN: &str = "\u{E70D}";
    pub const CHEVRON_UP: &str = "\u{E70E}";
    pub const EMOJI: &str = "\u{E76E}";
    pub const BACKSPACE: &str = "\u{E750}";
    pub const ENTER: &str = "\u{E751}";
    pub const SHIFT: &str = "\u{E752}";
}

/// The letter-key layouts the user can pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    /// 26-key 全拼.
    Pinyin,
    /// 26-key 小鹤双拼.
    Shuangpin,
    /// 九宫格拼音.
    T9,
    /// English 26-key.
    English,
}

impl Layout {
    pub fn of(chinese: bool, schema: Schema) -> Self {
        match (chinese, schema) {
            (false, _) => Layout::English,
            (true, Schema::Pinyin) => Layout::Pinyin,
            (true, Schema::Shuangpin) => Layout::Shuangpin,
            (true, Schema::T9) => Layout::T9,
        }
    }

    /// Shown faintly on the space bar and in the layout menu.
    pub fn name(self) -> &'static str {
        match self {
            Layout::Pinyin => "全拼",
            Layout::Shuangpin => "小鹤双拼",
            Layout::T9 => "九宫格",
            Layout::English => "English",
        }
    }
}

/// Symbol panel tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymTab {
    Chinese,
    English,
    Emoji,
}

// ---------------------------------------------------------------------------------------------
// Data tables
// ---------------------------------------------------------------------------------------------

pub const LETTER_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// Swipe-up / long-press secondaries, per letter row.
pub const SECONDARY_ROW1: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
pub const SECONDARY_ROW2_ZH: [&str; 9] = ["@", "#", "¥", "%", "&", "*", "-", "+", "="];
pub const SECONDARY_ROW2_EN: [&str; 9] = ["@", "#", "$", "%", "&", "*", "-", "+", "="];
pub const SECONDARY_ROW3_ZH: [&str; 7] = ["（", "）", "“", "”", "：", "；", "、"];
pub const SECONDARY_ROW3_EN: [&str; 7] = ["(", ")", "\"", "'", ":", ";", "/"];

/// 小鹤双拼 finals shown under each letter (official flypy chart; u/i/v also stand for sh/ch/zh).
pub const FLYPY: [(char, &str); 26] = [
    ('q', "iu"),
    ('w', "ei"),
    ('e', "e"),
    ('r', "uan"),
    ('t', "üe"),
    ('y', "un"),
    ('u', "sh"),
    ('i', "ch"),
    ('o', "uo"),
    ('p', "ie"),
    ('a', "a"),
    ('s', "ong iong"),
    ('d', "ai"),
    ('f', "en"),
    ('g', "eng"),
    ('h', "ang"),
    ('j', "an"),
    ('k', "ing uai"),
    ('l', "iang uang"),
    ('z', "ou"),
    ('x', "ia ua"),
    ('c', "ao"),
    ('v', "zh ui"),
    ('b', "in"),
    ('n', "iao"),
    ('m', "ian"),
];

pub fn flypy_hint(c: char) -> Option<&'static str> {
    FLYPY.iter().find(|(k, _)| *k == c).map(|(_, h)| *h)
}

/// Extra long-press alternates for letters in English mode (after the secondary).
pub fn english_accents(c: char) -> &'static [&'static str] {
    match c {
        'a' => &["à", "á", "â", "ä", "ā"],
        'e' => &["è", "é", "ê", "ë", "ē"],
        'i' => &["ì", "í", "î", "ï"],
        'o' => &["ò", "ó", "ô", "ö", "ø"],
        'u' => &["ù", "ú", "û", "ü"],
        'c' => &["ç"],
        'n' => &["ñ"],
        's' => &["ß"],
        _ => &[],
    }
}

pub const PUNCT_ZH: &[&str] = &["，", "。", "？", "！", "、", "：", "；", "……"];
pub const PUNCT_EN: &[&str] = &[",", ".", "?", "!", "'", "\"", ":", ";", "@"];

/// T9 left column when not composing.
pub const T9_PUNCT: &[&str] = &["，", "。", "？", "！", "、", "：", "；", "……", "～", "“", "”"];
/// Number pad left column.
pub const NUM_COLUMN: &[&str] = &["+", "-", "*", "/", "%", "=", ",", "@", "#", "(", ")", ":", "~"];

pub const T9_LETTERS: [&str; 8] = ["ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ"];

pub const SYM_ZH: &[&str] = &[
    "，", "。", "？", "！", "、", "：", "；", "“", "”", "‘", "’", "（", "）", "《", "》", "〈", "〉", "【", "】",
    "「", "」", "『", "』", "……", "——", "·", "～", "￥", "％", "＃", "＆", "＊", "＠", "＋", "－", "＝", "／",
    "＼", "｜", "〔", "〕", "°", "℃", "№", "※", "→", "←", "↑", "↓", "√", "×", "÷", "○", "●", "★", "☆",
];
pub const SYM_EN: &[&str] = &[
    ",", ".", "?", "!", ":", ";", "'", "\"", "(", ")", "[", "]", "{", "}", "<", ">", "@", "#", "$", "%",
    "^", "&", "*", "-", "_", "+", "=", "/", "\\", "|", "~", "`", "€", "£", "¥", "§", "©", "®", "™", "…",
];
pub const EMOJI: &[&str] = &[
    "😀", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "😅", "😭", "😡", "😱", "😴", "🥰", "😉", "🙄", "😏",
    "🤗", "🤝", "👍", "👎", "👏", "🙏", "💪", "👌", "✌️", "🎉", "❤️", "💔", "🔥", "✨", "🌹", "🎂", "☕",
    "🍺", "💯", "✅", "❌", "⭐", "🌙",
];

pub fn symbols(tab: SymTab) -> &'static [&'static str] {
    match tab {
        SymTab::Chinese => SYM_ZH,
        SymTab::English => SYM_EN,
        SymTab::Emoji => EMOJI,
    }
}

// ---------------------------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum KeyAction {
    /// A letter: emits `Char`, uppercased by shift.
    Letter(char),
    /// Emits `Char` as is (T9 digits, the syllable separator).
    Char(char),
    /// Emits `Text` (punctuation, digits on the number pad, symbols).
    Text(String),
    /// Fires on press and repeats; swipe left clears the composition.
    Backspace,
    Shift,
    Space,
    Enter,
    ToggleChinese,
    LayoutMenu,
    Symbols,
    Numbers,
    /// Back to the letter keys.
    Back,
    ClearComposition,
    Tab(SymTab),
    Voice,
    Hide,
    ExpandCandidates,
    CollapseCandidates,
    SetLayout(Layout),
    ToggleTheme,
    /// T9 key 1: syllable separator while composing, else "1".
    T9One,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// White character key.
    Char,
    /// Grey function key.
    Func,
    /// Accent key (enter).
    Accent,
    /// Function key in a latched state (caps lock, selected tab).
    Active,
    /// Flat icon in the candidate bar.
    Flat,
    /// Layout menu tile.
    Tile,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    /// Layout cell: the hit area. Drawn inset by half the gap.
    pub cell: Rect,
    pub action: KeyAction,
    pub label: String,
    pub icon: bool,
    /// Small text under the label (小鹤 finals, menu captions).
    pub sub: Option<String>,
    /// Small text above the label (T9 digits).
    pub top: Option<String>,
    /// Shown small in the top-right corner (the swipe-up secondary).
    pub corner: Option<String>,
    /// Emitted on swipe-up / long-press.
    pub secondary: Option<String>,
    /// Long-press popup; the secondary is pre-selected.
    pub alternates: Vec<String>,
    pub tone: Tone,
    /// Show the pop-up bubble while pressed.
    pub bubble: bool,
    /// Label size relative to the letter size.
    pub scale: f32,
    /// Selected (menu tile of the current layout).
    pub selected: bool,
}

impl Key {
    pub fn new(action: KeyAction, label: impl Into<String>, tone: Tone) -> Self {
        Self {
            cell: Rect::default(),
            action,
            label: label.into(),
            icon: false,
            sub: None,
            top: None,
            corner: None,
            secondary: None,
            alternates: Vec::new(),
            tone,
            bubble: false,
            scale: 1.0,
            selected: false,
        }
    }

    pub fn func(action: KeyAction, label: &str) -> Self {
        Self::new(action, label, Tone::Func).scaled(0.62)
    }

    pub fn icon(action: KeyAction, glyph: &str, tone: Tone) -> Self {
        Self { icon: true, ..Self::new(action, glyph, tone) }.scaled(0.78)
    }

    pub fn text(s: &str) -> Self {
        Self { bubble: true, ..Self::new(KeyAction::Text(s.to_string()), s, Tone::Char) }
    }

    pub fn scaled(mut self, s: f32) -> Self {
        self.scale = s;
        self
    }

    pub fn with_secondary(mut self, s: &str, alternates: Vec<String>) -> Self {
        self.corner = Some(s.to_string());
        self.secondary = Some(s.to_string());
        self.alternates = alternates;
        self
    }

    pub fn has_long_press(&self) -> bool {
        self.secondary.is_some() || !self.alternates.is_empty()
    }

    /// Alternates for the long-press popup and the index selected first.
    pub fn popup(&self) -> (Vec<String>, usize) {
        if !self.alternates.is_empty() {
            let sel = self.secondary.as_ref().and_then(|s| self.alternates.iter().position(|a| a == s)).unwrap_or(0);
            (self.alternates.clone(), sel)
        } else {
            (self.secondary.iter().cloned().collect(), 0)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------------

pub(crate) const ROWS: usize = 4;

/// Sizes derived from the window size. All DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    /// Top of the key area.
    pub top: f32,
    pub row_h: f32,
    pub pad_x: f32,
    pub gap_x: f32,
    pub gap_y: f32,
    pub radius: f32,
    /// Font scale (1.0 at a 64-DIP row).
    pub s: f32,
}

pub(crate) fn preferred_row(width: f32) -> f32 {
    (width * 0.047).clamp(50.0, 68.0)
}

pub(crate) fn bar_height(row: f32) -> f32 {
    (row * 0.78).clamp(42.0, 54.0)
}

pub(crate) fn preferred_height(width: f32) -> f32 {
    let row = preferred_row(width);
    let s = row / 64.0;
    bar_height(row) + ROWS as f32 * row + 10.0 * s
}

impl Metrics {
    pub fn new(w: f32, h: f32) -> Self {
        let pref_row = preferred_row(w);
        let bar_h = bar_height(pref_row).min(h * 0.18).max(30.0);
        let s0 = pref_row / 64.0;
        let pad_top = 3.0 * s0;
        let pad_bottom = 7.0 * s0;
        let row_h = ((h - bar_h - pad_top - pad_bottom) / ROWS as f32).max(20.0);
        let s = (row_h / 64.0).clamp(0.7, 1.25);
        let gap_x = (w * 0.0055).clamp(5.0, 9.0);
        Self {
            w,
            h,
            bar_h,
            top: bar_h + pad_top,
            row_h,
            pad_x: (gap_x * 0.5).max(3.0),
            gap_x,
            gap_y: (row_h * 0.15).clamp(6.0, 11.0),
            radius: (row_h * 0.11).clamp(5.0, 8.0),
            s,
        }
    }

    pub fn row_y(&self, row: usize) -> f32 {
        self.top + row as f32 * self.row_h
    }

    /// The area below the candidate bar.
    pub fn keys_area(&self) -> Rect {
        Rect::new(self.pad_x, self.top, self.w - 2.0 * self.pad_x, self.row_h * ROWS as f32)
    }

    /// Key face rect for a cell.
    pub fn face(&self, cell: Rect) -> Rect {
        Rect::new(
            cell.x + self.gap_x * 0.5,
            cell.y + self.gap_y * 0.5,
            (cell.w - self.gap_x).max(0.0),
            (cell.h - self.gap_y).max(0.0),
        )
    }

    pub fn letter_size(&self) -> f32 {
        27.0 * self.s
    }
}

// ---------------------------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------------------------

/// Everything the builders need to know about the current state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildCtx {
    pub layout: Layout,
    pub chinese: bool,
    pub composing: bool,
    pub shift: Shift,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Shift {
    #[default]
    Off,
    Once,
    Locked,
}

/// A scrollable list region (T9 spellings / punctuation, number-pad symbols).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnKind {
    T9,
    Numbers,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Built {
    pub keys: Vec<Key>,
    pub column: Option<(Rect, ColumnKind)>,
    /// Symbol grid or candidate grid area.
    pub grid: Option<Rect>,
}

fn place(out: &mut Vec<Key>, x0: f32, y: f32, h: f32, unit: f32, items: Vec<(f32, Key)>) {
    let mut x = x0;
    for (wu, mut k) in items {
        k.cell = Rect::new(x, y, wu * unit, h);
        x += wu * unit;
        out.push(k);
    }
}

fn punct_key(chinese: bool) -> Key {
    let (list, main, sec) = if chinese { (PUNCT_ZH, "，", "。") } else { (PUNCT_EN, ",", ".") };
    Key::text(main).with_secondary(sec, list.iter().map(|s| s.to_string()).collect())
}

fn enter_key() -> Key {
    Key::icon(KeyAction::Enter, icon::ENTER, Tone::Accent)
}

fn backspace_key() -> Key {
    Key::icon(KeyAction::Backspace, icon::BACKSPACE, Tone::Func)
}

fn space_key(layout: Layout) -> Key {
    Key::new(KeyAction::Space, layout.name(), Tone::Char).scaled(0.5)
}

fn toggle_key() -> Key {
    Key::func(KeyAction::ToggleChinese, "中/英")
}

fn letter_key(c: char, row: usize, idx: usize, ctx: &BuildCtx) -> Key {
    let upper = ctx.shift != Shift::Off;
    let label = if upper { c.to_ascii_uppercase().to_string() } else { c.to_string() };
    let sec = match row {
        0 => SECONDARY_ROW1[idx],
        1 if ctx.chinese => SECONDARY_ROW2_ZH[idx],
        1 => SECONDARY_ROW2_EN[idx],
        _ if ctx.chinese => SECONDARY_ROW3_ZH[idx],
        _ => SECONDARY_ROW3_EN[idx],
    };
    let mut alts = Vec::new();
    if ctx.layout == Layout::English && !english_accents(c).is_empty() {
        alts.push(sec.to_string());
        alts.extend(english_accents(c).iter().map(|a| {
            if upper { a.to_uppercase() } else { a.to_string() }
        }));
    }
    let mut k = Key { bubble: true, ..Key::new(KeyAction::Letter(c), label, Tone::Char) }.with_secondary(sec, alts);
    if ctx.layout == Layout::Shuangpin {
        k.sub = flypy_hint(c).map(str::to_string);
    }
    k
}

pub(crate) fn build_letters(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let unit = area.w / 10.0;
    let h = m.row_h;
    let mut keys = Vec::new();
    for (row, letters) in LETTER_ROWS.iter().enumerate() {
        let mut items: Vec<(f32, Key)> = Vec::new();
        if row == 2 {
            let left = if ctx.chinese && ctx.composing {
                Key::func(KeyAction::Char('\''), "分词")
            } else {
                let tone = if ctx.shift == Shift::Locked { Tone::Active } else { Tone::Func };
                Key::icon(KeyAction::Shift, icon::SHIFT, tone)
            };
            items.push((1.5, left));
        }
        for (i, c) in letters.chars().enumerate() {
            items.push((1.0, letter_key(c, row, i, ctx)));
        }
        if row == 2 {
            items.push((1.5, backspace_key()));
        }
        let x0 = if row == 1 { area.x + unit * 0.5 } else { area.x };
        place(&mut keys, x0, m.row_y(row), h, unit, items);
    }
    let bottom = vec![
        (1.25, Key::func(KeyAction::Symbols, "符号")),
        (1.25, Key::func(KeyAction::Numbers, "123")),
        (1.0, Key::icon(KeyAction::LayoutMenu, icon::KEYBOARD, Tone::Func)),
        (3.0, space_key(ctx.layout)),
        (1.0, toggle_key()),
        (1.0, punct_key(ctx.chinese)),
        (1.5, enter_key()),
    ];
    place(&mut keys, area.x, m.row_y(3), h, unit, bottom);
    Built { keys, ..Built::default() }
}

/// Column widths for the T9 and number pads: side columns and three main columns.
fn pad_columns(area: Rect) -> (f32, f32) {
    let side = area.w * 0.17;
    let main = (area.w - 2.0 * side) / 3.0;
    (side, main)
}

pub(crate) fn build_t9(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let (side, main) = pad_columns(area);
    let h = m.row_h;
    let x_main = area.x + side;
    let x_right = x_main + 3.0 * main;
    let mut keys = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            let n = row * 3 + col + 1;
            let mut k = if n == 1 {
                let mut k = Key::new(KeyAction::T9One, "分词", Tone::Char).scaled(0.62);
                k.top = Some("1".into());
                k.alternates = ["1", "@", ".", "/", ":", "_"].iter().map(|s| s.to_string()).collect();
                k
            } else {
                let digit = char::from(b'0' + n as u8);
                let mut k = Key::new(KeyAction::Char(digit), T9_LETTERS[n - 2], Tone::Char).scaled(0.84);
                k.top = Some(digit.to_string());
                k
            };
            k.cell = Rect::new(x_main + col as f32 * main, m.row_y(row), main, h);
            keys.push(k);
        }
    }
    let mut bs = backspace_key();
    bs.cell = Rect::new(x_right, m.row_y(0), side, h);
    keys.push(bs);
    let mut clear = Key::func(KeyAction::ClearComposition, "重输");
    clear.cell = Rect::new(x_right, m.row_y(1), side, h);
    keys.push(clear);
    let mut enter = enter_key();
    enter.cell = Rect::new(x_right, m.row_y(2), side, 2.0 * h);
    keys.push(enter);
    // Bottom row under the column and the main keys.
    let mut sym = Key::func(KeyAction::Symbols, "符号");
    sym.cell = Rect::new(area.x, m.row_y(3), side, h);
    keys.push(sym);
    let unit = 3.0 * main / 6.0;
    place(
        &mut keys,
        x_main,
        m.row_y(3),
        h,
        unit,
        vec![
            (1.5, Key::func(KeyAction::Numbers, "123")),
            (1.0, Key::icon(KeyAction::LayoutMenu, icon::KEYBOARD, Tone::Func)),
            (2.5, space_key(ctx.layout)),
            (1.0, toggle_key()),
        ],
    );
    let column = Rect::new(area.x, m.row_y(0), side, 3.0 * h);
    Built { keys, column: Some((column, ColumnKind::T9)), grid: None }
}

pub(crate) fn build_numbers(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let (side, main) = pad_columns(area);
    let h = m.row_h;
    let x_main = area.x + side;
    let x_right = x_main + 3.0 * main;
    let mut keys = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            let d = (row * 3 + col + 1).to_string();
            let mut k = Key::text(&d).scaled(1.05);
            k.cell = Rect::new(x_main + col as f32 * main, m.row_y(row), main, h);
            keys.push(k);
        }
    }
    let mut back = Key::func(KeyAction::Back, "返回");
    back.cell = Rect::new(area.x, m.row_y(3), side, h);
    keys.push(back);
    let mut sym = Key::func(KeyAction::Symbols, "符号");
    sym.cell = Rect::new(x_main, m.row_y(3), main, h);
    keys.push(sym);
    let mut zero = Key::text("0").scaled(1.05);
    zero.cell = Rect::new(x_main + main, m.row_y(3), main, h);
    keys.push(zero);
    let mut space = space_key(ctx.layout);
    space.label = String::new();
    space.cell = Rect::new(x_main + 2.0 * main, m.row_y(3), main, h);
    keys.push(space);
    let mut bs = backspace_key();
    bs.cell = Rect::new(x_right, m.row_y(0), side, h);
    keys.push(bs);
    let mut dot = Key::text(".").with_secondary(",", vec![".".into(), ",".into(), "。".into(), "，".into()]);
    dot.corner = None;
    dot.cell = Rect::new(x_right, m.row_y(1), side, h);
    keys.push(dot);
    let mut enter = enter_key();
    enter.cell = Rect::new(x_right, m.row_y(2), side, 2.0 * h);
    keys.push(enter);
    let column = Rect::new(area.x, m.row_y(0), side, 3.0 * h);
    Built { keys, column: Some((column, ColumnKind::Numbers)), grid: None }
}

pub(crate) fn build_symbols(m: &Metrics, tab: SymTab) -> Built {
    let area = m.keys_area();
    let unit = area.w / 10.0;
    let mut keys = Vec::new();
    let tab_key = |t: SymTab, label: &str| {
        let mut k = Key::func(KeyAction::Tab(t), label);
        if t == tab {
            k.tone = Tone::Active;
        }
        k
    };
    place(
        &mut keys,
        area.x,
        m.row_y(3),
        m.row_h,
        unit,
        vec![
            (1.5, Key::func(KeyAction::Back, "返回")),
            (1.2, tab_key(SymTab::Chinese, "中文")),
            (1.2, tab_key(SymTab::English, "英文")),
            (1.2, tab_key(SymTab::Emoji, "表情")),
            (3.4, space_key(Layout::Pinyin).scaled(0.5)),
            (1.5, backspace_key()),
        ],
    );
    if let Some(k) = keys.iter_mut().find(|k| k.action == KeyAction::Space) {
        k.label = String::new();
    }
    let grid = Rect::new(area.x, m.row_y(0), area.w, 3.0 * m.row_h);
    Built { keys, column: None, grid: Some(grid) }
}

/// Right rail of the expanded candidate grid.
pub(crate) fn build_candidate_grid(m: &Metrics) -> Built {
    let area = m.keys_area();
    let rail = (area.w * 0.12).clamp(64.0, 150.0);
    let x = area.x + area.w - rail;
    let mut keys = Vec::new();
    let items = [
        Key::icon(KeyAction::CollapseCandidates, icon::CHEVRON_UP, Tone::Func),
        backspace_key(),
        Key::func(KeyAction::ClearComposition, "重输"),
        enter_key(),
    ];
    for (row, mut k) in items.into_iter().enumerate() {
        k.cell = Rect::new(x, m.row_y(row), rail, m.row_h);
        keys.push(k);
    }
    let grid = Rect::new(area.x, area.y, area.w - rail, area.h);
    Built { keys, column: None, grid: Some(grid) }
}

pub(crate) fn build_menu(m: &Metrics, current: Layout, dark: bool) -> Built {
    let area = m.keys_area();
    let tiles: [(KeyAction, &str, &str, bool); 5] = [
        (KeyAction::SetLayout(Layout::Pinyin), "拼", "全拼", current == Layout::Pinyin),
        (KeyAction::SetLayout(Layout::Shuangpin), "鹤", "小鹤双拼", current == Layout::Shuangpin),
        (KeyAction::SetLayout(Layout::T9), "九", "九宫格", current == Layout::T9),
        (KeyAction::SetLayout(Layout::English), "En", "English", current == Layout::English),
        (KeyAction::ToggleTheme, if dark { "☀" } else { "☾" }, if dark { "浅色" } else { "深色" }, false),
    ];
    let n = tiles.len() as f32;
    let tile_w = (area.w / n).min(m.row_h * 2.6);
    let total = tile_w * n;
    let x0 = area.x + (area.w - total) / 2.0;
    let y = area.y + m.row_h * 0.35;
    let h = m.row_h * 2.3;
    let mut keys = Vec::new();
    for (i, (action, glyph, caption, selected)) in tiles.into_iter().enumerate() {
        let mut k = Key::new(action, glyph, Tone::Tile);
        k.sub = Some(caption.to_string());
        k.selected = selected;
        k.cell = Rect::new(x0 + i as f32 * tile_w, y, tile_w, h);
        keys.push(k);
    }
    let mut back = Key::func(KeyAction::Back, "返回");
    back.cell = Rect::new(area.x + area.w / 2.0 - m.row_h * 1.25, m.row_y(3), m.row_h * 2.5, m.row_h);
    keys.push(back);
    Built { keys, column: None, grid: None }
}
