//! Keyboard layouts: data tables (easy to tweak) and the builders that turn them into positioned
//! [`Key`]s for the current size and state.

use dianmo_core::{Action, EditKey, KeyChord, KeyCode, Schema};

use crate::canvas::Rect;
use crate::settings::ShuangpinScheme;

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
    pub const UNDO: &str = "\u{E7A7}";
    pub const REDO: &str = "\u{E7A6}";
    pub const SELECT_ALL: &str = "\u{E8B3}";
    pub const CUT: &str = "\u{E8C6}";
    pub const COPY: &str = "\u{E8C8}";
    pub const PASTE: &str = "\u{E77F}";
    pub const CHEVRON_LEFT: &str = "\u{E76B}";
    pub const CHEVRON_RIGHT: &str = "\u{E76C}";
    /// MultiSelect: the 选择 (selection mode) tool.
    pub const SELECT: &str = "\u{E762}";
    /// History: the clipboard history.
    pub const CLIPBOARD: &str = "\u{E81C}";
    pub const PIN: &str = "\u{E718}";
    pub const CLOSE: &str = "\u{E711}";
    /// Gear: the settings window.
    pub const SETTINGS: &str = "\u{E713}";
}

/// The letter-key layouts the user can pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    /// 26-key 全拼.
    Pinyin,
    /// 26-key 双拼 (scheme: `KeyboardView::set_shuangpin`, 小鹤 by default).
    Shuangpin,
    /// 九宫格拼音.
    T9,
    /// English 26-key.
    English,
    /// 电脑键盘 (TODO #31): a full PC keyboard whose keys are sent as real key presses
    /// (pass-through: the target app's own IME handles them). Not tied to a schema.
    Pc,
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
            Layout::Pc => "电脑键盘",
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

/// 自然码双拼 finals. Source for these three tables: the `speller/algebra` of rime-ice's
/// `double_pinyin*.schema.yaml` (collected in docs/status/dianmo-rime.md「双拼键位」).
pub const ZIRANMA: [(char, &str); 26] = [
    ('q', "iu"),
    ('w', "ia ua"),
    ('e', "e"),
    ('r', "uan üan"),
    ('t', "üe"),
    ('y', "ing uai"),
    ('u', "sh"),
    ('i', "ch"),
    ('o', "uo"),
    ('p', "un ün"),
    ('a', "a"),
    ('s', "ong iong"),
    ('d', "iang uang"),
    ('f', "en"),
    ('g', "eng"),
    ('h', "ang"),
    ('j', "an"),
    ('k', "ao"),
    ('l', "ai"),
    ('z', "ei"),
    ('x', "ie"),
    ('c', "iao"),
    ('v', "zh ui"),
    ('b', "ou"),
    ('n', "in"),
    ('m', "ian"),
];

/// 微软双拼 finals (Rime `double_pinyin_mspy`). `;` = ing.
pub const MSPY: [(char, &str); 27] = [
    ('q', "iu"),
    ('w', "ia ua"),
    ('e', "e"),
    ('r', "er uan üan"),
    ('t', "ue üe"),
    ('y', "ü uai"),
    ('u', "sh"),
    ('i', "ch"),
    ('o', "uo"),
    ('p', "un ün"),
    ('a', "a"),
    ('s', "ong iong"),
    ('d', "iang uang"),
    ('f', "en"),
    ('g', "eng"),
    ('h', "ang"),
    ('j', "an"),
    ('k', "ao"),
    ('l', "ai"),
    (';', "ing"),
    ('z', "ei"),
    ('x', "ie"),
    ('c', "iao"),
    ('v', "zh ui ue"),
    ('b', "ou"),
    ('n', "in"),
    ('m', "ian"),
];

/// 搜狗双拼 finals (Rime `double_pinyin_sogou`). `;` = ing. Same key faces as 微软 except v
/// (no ue / üe); they differ mostly in zero-initial syllables.
pub const SOGOU: [(char, &str); 27] = [
    ('q', "iu"),
    ('w', "ia ua"),
    ('e', "e"),
    ('r', "er uan üan"),
    ('t', "ue üe"),
    ('y', "ü uai"),
    ('u', "sh"),
    ('i', "ch"),
    ('o', "uo"),
    ('p', "un ün"),
    ('a', "a"),
    ('s', "ong iong"),
    ('d', "iang uang"),
    ('f', "en"),
    ('g', "eng"),
    ('h', "ang"),
    ('j', "an"),
    ('k', "ao"),
    ('l', "ai"),
    (';', "ing"),
    ('z', "ei"),
    ('x', "ie"),
    ('c', "iao"),
    ('v', "zh ui"),
    ('b', "ou"),
    ('n', "in"),
    ('m', "ian"),
];

/// The key-face table of a 双拼 scheme.
pub fn shuangpin_table(scheme: ShuangpinScheme) -> &'static [(char, &'static str)] {
    match scheme {
        ShuangpinScheme::Xiaohe => &FLYPY,
        ShuangpinScheme::Ziranma => &ZIRANMA,
        ShuangpinScheme::Microsoft => &MSPY,
        ShuangpinScheme::Sogou => &SOGOU,
    }
}

/// Finals shown under key `c` (a letter, or `;`) for a 双拼 scheme.
pub fn shuangpin_hint(scheme: ShuangpinScheme, c: char) -> Option<&'static str> {
    shuangpin_table(scheme).iter().find(|(k, _)| *k == c).map(|(_, h)| *h)
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

// Wide (Surface landscape) layout, DESIGN.md §2「Surface 宽屏布局」. Symbol keys are
// (main, with shift); shift swaps them like a physical keyboard.

pub const DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
pub const DIGIT_SHIFT_ZH: [&str; 10] = ["！", "@", "#", "¥", "%", "……", "&", "*", "（", "）"];
pub const DIGIT_SHIFT_EN: [&str; 10] = ["!", "@", "#", "$", "%", "^", "&", "*", "(", ")"];
pub const WIDE_ROW0_ZH: [(&str, &str); 2] = [("-", "——"), ("=", "+")];
pub const WIDE_ROW0_EN: [(&str, &str); 2] = [("-", "_"), ("=", "+")];
pub const WIDE_ROW1_ZH: [(&str, &str); 3] = [("【", "「"), ("】", "」"), ("、", "｜")];
pub const WIDE_ROW1_EN: [(&str, &str); 3] = [("[", "{"), ("]", "}"), ("\\", "|")];
pub const WIDE_ROW2_ZH: [(&str, &str); 2] = [("；", "："), ("“", "”")];
pub const WIDE_ROW2_EN: [(&str, &str); 2] = [(";", ":"), ("'", "\"")];
pub const WIDE_ROW3_ZH: [(&str, &str); 3] = [("，", "《"), ("。", "》"), ("？", "！")];
pub const WIDE_ROW3_EN: [(&str, &str); 3] = [(",", "<"), (".", ">"), ("/", "?")];
/// US-layout key under each symbol key of rows 0–3 (what Ctrl/Alt/Win + the key sends).
pub const WIDE_ROW0_CODES: [char; 2] = ['-', '='];
pub const WIDE_ROW1_CODES: [char; 3] = ['[', ']', '\\'];
pub const WIDE_ROW2_CODES: [char; 2] = [';', '\''];
pub const WIDE_ROW3_CODES: [char; 3] = [',', '.', '/'];
/// Long-press secondaries of the qwerty row when the digits have their own row.
pub const WIDE_SECONDARY_ROW1_ZH: [&str; 10] = ["～", "·", "——", "+", "=", "_", "「", "」", "『", "』"];
pub const WIDE_SECONDARY_ROW1_EN: [&str; 10] = ["~", "`", "_", "+", "=", "^", "{", "}", "<", ">"];
/// Wide number panel: symbol block left of the digits.
pub const WIDE_NUM_SYMS: [[&str; 3]; 3] = [["+", "-", "*"], ["/", "%", "="], ["(", ")", ":"]];
/// Wide T9: punctuation block between the T9 keys and the digit pad (2 columns × 4 rows).
pub const WIDE_T9_PUNCT_ZH: [&str; 8] = ["，", "。", "？", "！", "、", "：", "；", "……"];
pub const WIDE_T9_PUNCT_EN: [&str; 8] = [",", ".", "?", "!", "'", "\"", ":", ";"];

/// Hints shown under letters while Ctrl / Win is active (DESIGN.md §2「电脑按键」).
pub const CTRL_HINTS: &[(char, &str)] = &[
    ('a', "全选"),
    ('c', "复制"),
    ('v', "粘贴"),
    ('x', "剪切"),
    ('z', "撤销"),
    ('y', "重做"),
    ('s', "保存"),
    ('f', "查找"),
    ('h', "替换"),
    ('w', "关闭"),
    ('t', "新标签"),
    ('n', "新建"),
    ('o', "打开"),
    ('p', "打印"),
    ('r', "刷新"),
];
pub const WIN_HINTS: &[(char, &str)] = &[
    ('d', "桌面"),
    ('e', "文件"),
    ('v', "剪贴板"),
    ('r', "运行"),
    ('i', "设置"),
    ('l', "锁屏"),
    ('s', "搜索"),
    ('a', "通知"),
    ('x', "菜单"),
];

pub fn hint(table: &[(char, &'static str)], c: char) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == c).map(|(_, h)| *h)
}

pub const PUNCT_ZH: &[&str] = &["，", "。", "？", "！", "、", "：", "；", "……"];
pub const PUNCT_EN: &[&str] = &[",", ".", "?", "!", "'", "\"", ":", ";", "@"];

/// T9 left column when not composing.
pub const T9_PUNCT: &[&str] = &["，", "。", "？", "！", "、", "：", "；", "……", "～", "“", "”"];
/// The same with 全角标点 off.
pub const T9_PUNCT_HALF: &[&str] = &[",", ".", "?", "!", ":", ";", "'", "\"", "~", "(", ")"];
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

/// Keys that change how the next key is sent. Shift also uppercases letters and picks the
/// shifted symbol; Fn switches the digit row to F1–F12 and the arrows to Home/End/PgUp/PgDn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Win,
    Fn,
}

impl Modifier {
    pub const ALL: [Modifier; 5] = [Modifier::Shift, Modifier::Ctrl, Modifier::Alt, Modifier::Win, Modifier::Fn];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum KeyAction {
    /// A letter: emits `Char`, uppercased by shift; with Ctrl/Alt/Win a key chord.
    Letter(char),
    /// Emits `Char` as is (T9 digits, the syllable separator).
    Char(char),
    /// Emits `Text` (punctuation, digits, symbols); with Ctrl/Alt/Win the chord of `Key::code`.
    Text(String),
    /// Fires on press and repeats; swipe left clears the composition.
    Backspace,
    /// Shift, Ctrl, Alt, Win, Fn: tap = next key only, double tap = locked, hold = chord with
    /// keys other fingers tap.
    Mod(Modifier),
    /// Locks/unlocks Shift.
    CapsLock,
    Space,
    Enter,
    ToggleChinese,
    LayoutMenu,
    Symbols,
    Numbers,
    /// Esc, F1–F12, Tab, arrows, ... (DESIGN.md §2「电脑按键」).
    PcKeys,
    /// Back to the letter keys.
    Back,
    ClearComposition,
    Tab(SymTab),
    Voice,
    /// Layout menu: voice mode (shrink into the voice ball).
    VoiceBall,
    Hide,
    /// Toolbar ⚙: open the settings window.
    Settings,
    ExpandCandidates,
    CollapseCandidates,
    SetLayout(Layout),
    ToggleTheme,
    /// T9 key 1: syllable separator while composing, else "1".
    T9One,
    /// An editing key (Tab, Esc, arrows, Home/End, Del). Fires on press and repeats, unless the
    /// key has a `hold` action (Tab → Esc, toolbar ← → line start/end); then it fires on release.
    Edit(EditKey),
    /// A key chord (edit area, F keys); fires on release. Active modifiers are added.
    Chord(KeyChord),
    /// Select all, then delete (edit area 清空).
    ClearAll,
    /// Candidate grid paging.
    PageUp,
    PageDown,
    /// 电脑键盘: a real key, down on press (auto-repeating while held), up on release.
    Raw(KeyCode),
    /// A chord that fires on press and repeats while held (selection bar arrows).
    Press(KeyChord),
    /// Toggles selection mode (selection bar; arrows extend the selection).
    SelectMode,
    /// Selection bar buttons.
    Sel(SelAct),
    /// Opens the clipboard panel.
    ClipPanel,
    /// Closes the clipboard card bar (back to the toolbar).
    CloseClipBar,
    /// Clipboard panel: remove every unpinned entry.
    ClearClips,
    /// 电脑键盘: back to the previous layout.
    PcBack,
}

/// Right-hand buttons of the selection bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelAct {
    Copy,
    Cut,
    Paste,
    Delete,
    Done,
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
    /// Small text under the label (小鹤 finals, menu captions, toolbar captions, Ctrl hints).
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
    /// Long-press action instead of a secondary character (Tab → Esc, ← → line start).
    pub hold: Option<Action>,
    /// The physical key a Text key sits on, for Ctrl/Alt/Win chords ("，" → ',').
    pub code: Option<char>,
    /// Small text in the top-left corner (电脑键盘: the shifted character).
    pub top_left: Option<String>,
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
            hold: None,
            code: None,
            top_left: None,
        }
    }

    pub fn func(action: KeyAction, label: &str) -> Self {
        Self::new(action, label, Tone::Func).scaled(0.62)
    }

    pub fn icon(action: KeyAction, glyph: &str, tone: Tone) -> Self {
        Self { icon: true, ..Self::new(action, glyph, tone) }.scaled(0.78)
    }

    pub fn text(s: &str) -> Self {
        let code = {
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) if c.is_ascii_graphic() => Some(c),
                _ => None,
            }
        };
        Self { bubble: true, code, ..Self::new(KeyAction::Text(s.to_string()), s, Tone::Char) }
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

    pub fn with_hold(mut self, hold: Action, hint: &str) -> Self {
        self.hold = Some(hold);
        if !hint.is_empty() {
            self.corner = Some(hint.to_string());
        }
        self
    }

    pub fn with_code(mut self, code: char) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_sub(mut self, sub: &str) -> Self {
        self.sub = Some(sub.to_string());
        self
    }

    pub fn has_long_press(&self) -> bool {
        self.secondary.is_some() || !self.alternates.is_empty() || self.hold.is_some()
    }

    /// Fires when pressed (and repeats while held) rather than on release.
    pub fn fires_on_press(&self) -> bool {
        match self.action {
            KeyAction::Backspace | KeyAction::Raw(_) | KeyAction::Press(_) => true,
            KeyAction::Edit(_) => self.hold.is_none(),
            _ => false,
        }
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

/// Rows of the phone layout.
pub(crate) const ROWS: usize = 4;
/// Rows of the wide layout (digit row on top).
pub(crate) const WIDE_ROWS: usize = 5;
/// Keyboard width (DIPs) from which the wide layout is used (Surface landscape is 1440).
pub const WIDE_MIN_WIDTH: f32 = 1100.0;
/// Width of the wide 26-key block in key units (iPad Pro style).
pub(crate) const WIDE_UNITS: f32 = 14.5;
/// Smallest key height of the wide layout, whatever the height setting.
pub(crate) const WIDE_MIN_ROW: f32 = 52.0;

pub fn is_wide(width: f32) -> bool {
    width >= WIDE_MIN_WIDTH
}

/// Sizes derived from the window size. All DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub w: f32,
    pub h: f32,
    pub bar_h: f32,
    /// Top of the key area.
    pub top: f32,
    pub row_h: f32,
    /// Rows of the letter layout (4 phone, 5 wide); every panel fills the same area.
    pub rows: usize,
    pub wide: bool,
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

fn wide_row(width: f32) -> f32 {
    (width * 0.0396).clamp(54.0, 60.0)
}

fn wide_bar(width: f32) -> f32 {
    (width * 0.04).clamp(52.0, 60.0)
}

/// Phone layout height (the host multiplies by the height setting).
pub(crate) fn preferred_height(width: f32) -> f32 {
    let row = preferred_row(width);
    let s = row / 64.0;
    bar_height(row) + ROWS as f32 * row + 10.0 * s
}

/// Wide layout height for a height setting: only the key rows scale, never below
/// [`WIDE_MIN_ROW`]. 1440 wide: 353 DIP at 1.0 (37% of a 960-DIP screen).
pub(crate) fn preferred_height_wide(width: f32, scale: f32) -> f32 {
    let row0 = wide_row(width);
    let row = (row0 * scale).max(WIDE_MIN_ROW);
    wide_bar(width) + WIDE_ROWS as f32 * row + 10.0 * row0 / 64.0
}

/// Rows of the 电脑键盘 layout (the function-key row counts as one).
pub(crate) const PC_ROWS: usize = 6;
/// Width of the 电脑键盘 in key units (a laptop keyboard row).
pub(crate) const PC_UNITS: f32 = 15.0;

impl Metrics {
    pub fn new(w: f32, h: f32) -> Self {
        Self::build(w, h, false)
    }

    /// 电脑键盘: same window height, a thin top bar and six key rows.
    pub fn pc(w: f32, h: f32) -> Self {
        Self::build(w, h, true)
    }

    fn build(w: f32, h: f32, pc: bool) -> Self {
        let wide = is_wide(w);
        let (pref_row, pref_bar, rows) =
            if wide { (wide_row(w), wide_bar(w), WIDE_ROWS) } else { (preferred_row(w), bar_height(preferred_row(w)), ROWS) };
        let bar_h = pref_bar.min(h * 0.18).max(30.0);
        let (bar_h, rows) = if pc { ((bar_h * 0.62).clamp(28.0, 38.0), PC_ROWS) } else { (bar_h, rows) };
        let s0 = pref_row / 64.0;
        let pad_top = 3.0 * s0;
        let pad_bottom = 7.0 * s0;
        let row_h = ((h - bar_h - pad_top - pad_bottom) / rows as f32).max(20.0);
        let s = (row_h / 64.0).clamp(0.7, 1.25);
        let gap_x = (w * 0.0055).clamp(5.0, 9.0);
        Self {
            w,
            h,
            bar_h,
            top: bar_h + pad_top,
            row_h,
            rows,
            wide,
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
        Rect::new(self.pad_x, self.top, self.w - 2.0 * self.pad_x, self.row_h * self.rows as f32)
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

    /// Splits the key area into `n` equal rows: (top of row 0, row height).
    pub fn grid_rows(&self, n: usize) -> (f32, f32) {
        let a = self.keys_area();
        (a.y, a.h / n as f32)
    }
}

// ---------------------------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Latch {
    #[default]
    Off,
    /// Applies to the next key only.
    Once,
    Locked,
}

/// Modifier state as the builders see it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mods {
    pub latch: [Latch; 5],
    /// Held down by a finger right now.
    pub held: [bool; 5],
}

impl Mods {
    pub fn on(&self, m: Modifier) -> bool {
        self.latch[m.index()] != Latch::Off || self.held[m.index()]
    }

    pub fn latch(&self, m: Modifier) -> Latch {
        self.latch[m.index()]
    }

    /// Ctrl, Alt or Win: keys are sent as chords.
    pub fn chording(&self) -> bool {
        self.on(Modifier::Ctrl) || self.on(Modifier::Alt) || self.on(Modifier::Win)
    }
}

/// Everything the builders need to know about the current state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildCtx {
    pub layout: Layout,
    pub chinese: bool,
    /// Punctuation keys are full-width (中文 with 全角标点 on).
    pub zh_punct: bool,
    /// 双拼 scheme (key-face finals, the `;` key).
    pub sp: ShuangpinScheme,
    pub composing: bool,
    pub mods: Mods,
    /// Wide 26-key layout: show the two-column edit area on the right.
    pub edit_area: bool,
    /// 电脑键盘: Caps Lock is on (as far as we know: we toggle it with our Caps key).
    pub caps: bool,
}

impl BuildCtx {
    pub fn shift(&self) -> bool {
        self.mods.on(Modifier::Shift)
    }
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
    Key::text(main).with_secondary(sec, list.iter().map(|s| s.to_string()).collect()).with_code(',')
}

fn enter_key() -> Key {
    Key::icon(KeyAction::Enter, icon::ENTER, Tone::Accent)
}

fn backspace_key() -> Key {
    Key::icon(KeyAction::Backspace, icon::BACKSPACE, Tone::Func)
}

/// The layout's name as the space bar and the menu show it (双拼 by scheme).
pub(crate) fn layout_label(layout: Layout, sp: ShuangpinScheme) -> &'static str {
    if layout == Layout::Shuangpin { sp.name() } else { layout.name() }
}

fn space_key(ctx: &BuildCtx) -> Key {
    Key::new(KeyAction::Space, layout_label(ctx.layout, ctx.sp), Tone::Char).scaled(0.5)
}

/// 微软 / 搜狗双拼: the `；` key also types the final「ing」 while composing (the controller
/// routes it to the engine).
fn sp_semicolon(ctx: &BuildCtx) -> bool {
    ctx.layout == Layout::Shuangpin && ctx.sp.uses_semicolon()
}

fn toggle_key() -> Key {
    Key::func(KeyAction::ToggleChinese, "中/英")
}

/// A modifier key; lit while active, `locked` draws the lock bar.
fn mod_key(m: Modifier, mods: &Mods) -> Key {
    let (label, icon) = match m {
        Modifier::Shift => (icon::SHIFT, true),
        Modifier::Ctrl => ("Ctrl", false),
        Modifier::Alt => ("Alt", false),
        Modifier::Win => ("Win", false),
        Modifier::Fn => ("Fn", false),
    };
    let tone = if mods.on(m) && m != Modifier::Shift || mods.latch(m) == Latch::Locked { Tone::Active } else { Tone::Func };
    if icon { Key::icon(KeyAction::Mod(m), label, tone) } else { Key::new(KeyAction::Mod(m), label, tone).scaled(0.62) }
}

fn edit_key(key: EditKey, label: &str) -> Key {
    Key::func(KeyAction::Edit(key), label)
}

fn arrow_key(key: EditKey) -> Key {
    let glyph = match key {
        EditKey::Left => icon::CHEVRON_LEFT,
        EditKey::Right => icon::CHEVRON_RIGHT,
        EditKey::Up => icon::CHEVRON_UP,
        _ => icon::CHEVRON_DOWN,
    };
    Key::icon(KeyAction::Edit(key), glyph, Tone::Func).scaled(0.6)
}

/// Tab; long-press sends Esc (`hint`: show "Esc" in the corner where Esc has no key of its own).
fn tab_key(hint: bool) -> Key {
    edit_key(EditKey::Tab, "Tab").with_hold(Action::Edit(EditKey::Escape), if hint { "Esc" } else { "" })
}

/// F1–F24 key (fires on release; active modifiers are added).
fn f_key(n: u8) -> Key {
    Key::func(KeyAction::Chord(KeyChord::key(KeyCode::F(n))), &format!("F{n}"))
}

/// A symbol key of the wide layout: shift swaps main and shifted.
fn sym_key(main: &str, shifted: &str, code: char, ctx: &BuildCtx) -> Key {
    let (a, b) = if ctx.shift() { (shifted, main) } else { (main, shifted) };
    let mut k = Key::text(a).with_secondary(b, Vec::new()).with_code(code);
    if a == "“" || a == "”" {
        k.alternates = ["“", "”", "‘", "’"].iter().map(|s| s.to_string()).collect();
    }
    k
}

fn letter_key(c: char, row: usize, idx: usize, ctx: &BuildCtx, wide: bool) -> Key {
    let upper = ctx.shift();
    let label = if upper { c.to_ascii_uppercase().to_string() } else { c.to_string() };
    let sec = match row {
        0 if wide && ctx.zh_punct => WIDE_SECONDARY_ROW1_ZH[idx],
        0 if wide => WIDE_SECONDARY_ROW1_EN[idx],
        0 => SECONDARY_ROW1[idx],
        1 if ctx.chinese => SECONDARY_ROW2_ZH[idx],
        1 => SECONDARY_ROW2_EN[idx],
        _ if ctx.zh_punct => SECONDARY_ROW3_ZH[idx],
        _ => SECONDARY_ROW3_EN[idx],
    };
    let mut alts = Vec::new();
    if ctx.layout == Layout::English && !english_accents(c).is_empty() {
        alts.push(sec.to_string());
        alts.extend(english_accents(c).iter().map(|a| if upper { a.to_uppercase() } else { a.to_string() }));
    }
    let mut k = Key { bubble: true, ..Key::new(KeyAction::Letter(c), label, Tone::Char) }.with_secondary(sec, alts);
    // While chording, the hint for the shortcut replaces the 双拼 finals.
    let chord_hint = if ctx.mods.on(Modifier::Ctrl) {
        hint(CTRL_HINTS, c)
    } else if ctx.mods.on(Modifier::Win) {
        if c == 's' && ctx.shift() { Some("截图") } else { hint(WIN_HINTS, c) }
    } else {
        None
    };
    if ctx.mods.chording() {
        k.sub = chord_hint.map(str::to_string);
    } else if ctx.layout == Layout::Shuangpin {
        k.sub = shuangpin_hint(ctx.sp, c).map(str::to_string);
    }
    k
}

/// 微软 / 搜狗双拼's extra `；` key on the phone layout (end of the a–l row): ing.
fn semicolon_key(ctx: &BuildCtx) -> Key {
    let mut k = Key::text(if ctx.zh_punct { "；" } else { ";" }).with_code(';');
    if !ctx.mods.chording() {
        k.sub = shuangpin_hint(ctx.sp, ';').map(str::to_string);
    }
    k
}

pub(crate) fn build_letters(m: &Metrics, ctx: &BuildCtx) -> Built {
    if m.wide {
        return build_letters_wide(m, ctx);
    }
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
                mod_key(Modifier::Shift, &ctx.mods)
            };
            items.push((1.5, left));
        }
        for (i, c) in letters.chars().enumerate() {
            items.push((1.0, letter_key(c, row, i, ctx, false)));
        }
        // 微软 / 搜狗双拼 need `；` (ing): a tenth key fills the a–l row.
        let semicolon = row == 1 && sp_semicolon(ctx);
        if semicolon {
            items.push((1.0, semicolon_key(ctx)));
        }
        if row == 2 {
            items.push((1.5, backspace_key()));
        }
        let x0 = if row == 1 && !semicolon { area.x + unit * 0.5 } else { area.x };
        place(&mut keys, x0, m.row_y(row), h, unit, items);
    }
    let bottom = vec![
        (1.25, Key::func(KeyAction::Symbols, "符号")),
        (1.25, Key::func(KeyAction::Numbers, "123")),
        (1.0, Key::icon(KeyAction::LayoutMenu, icon::KEYBOARD, Tone::Func)),
        (3.0, space_key(ctx)),
        (1.0, toggle_key()),
        (1.0, punct_key(ctx.zh_punct)),
        (1.5, enter_key()),
    ];
    place(&mut keys, area.x, m.row_y(3), h, unit, bottom);
    Built { keys, ..Built::default() }
}

/// Width of the wide edit area (two columns) for a key area width, 0 when off.
pub(crate) fn edit_area_width(area_w: f32, on: bool) -> f32 {
    if on { (area_w * 0.064).clamp(70.0, 96.0) * 2.0 } else { 0.0 }
}

/// Wide 26 keys (DESIGN.md §2「Surface 宽屏布局」「电脑按键」「常驻编辑区」):
/// Esc 1–0 - = ⌫ / Tab q–p 【】、/ 大写 a–l ；“ ⏎ / ⇧ z–m ，。？ ⇧ /
/// Ctrl Win Alt Fn 符号 空格 中/英 ← ↑↓ → 收起, plus the edit area on the right.
fn build_letters_wide(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let edit_w = edit_area_width(area.w, ctx.edit_area);
    let sep = if edit_w > 0.0 { m.gap_x * 1.5 } else { 0.0 };
    let letters_w = area.w - edit_w - sep;
    let unit = letters_w / WIDE_UNITS;
    let h = m.row_h;
    // Punctuation follows 全角标点; 分词 follows 中文.
    let zh = ctx.zh_punct;
    let fn_on = ctx.mods.on(Modifier::Fn);
    let mut keys = Vec::new();

    // Row 0: Esc, digits (F1–F12 with Fn), - =, ⌫ (Del with Fn).
    let mut items: Vec<(f32, Key)> = Vec::new();
    items.push((1.0, if fn_on { Key::text("`").scaled(0.9) } else { edit_key(EditKey::Escape, "Esc") }));
    let shifted_digits = if zh { DIGIT_SHIFT_ZH } else { DIGIT_SHIFT_EN };
    for i in 0..10 {
        let k = if fn_on { f_key(i as u8 + 1) } else { sym_key(DIGITS[i], shifted_digits[i], DIGITS[i].chars().next().unwrap(), ctx) };
        items.push((1.0, k));
    }
    let row0 = if zh { WIDE_ROW0_ZH } else { WIDE_ROW0_EN };
    for (i, (a, b)) in row0.iter().enumerate() {
        items.push((1.0, if fn_on { f_key(11 + i as u8) } else { sym_key(a, b, WIDE_ROW0_CODES[i], ctx) }));
    }
    items.push((1.5, if fn_on { edit_key(EditKey::Delete, "Del") } else { backspace_key() }));
    place(&mut keys, area.x, m.row_y(0), h, unit, items);

    // Row 1: Tab (hold: Esc), q–p, 【 】 、.
    let mut items: Vec<(f32, Key)> = vec![(1.5, tab_key(false))];
    for (i, c) in LETTER_ROWS[0].chars().enumerate() {
        items.push((1.0, letter_key(c, 0, i, ctx, true)));
    }
    let row1 = if zh { WIDE_ROW1_ZH } else { WIDE_ROW1_EN };
    for (i, (a, b)) in row1.iter().enumerate() {
        items.push((1.0, sym_key(a, b, WIDE_ROW1_CODES[i], ctx)));
    }
    place(&mut keys, area.x, m.row_y(1), h, unit, items);

    // Row 2: caps, a–l, ； “, enter.
    let caps_tone = if ctx.mods.latch(Modifier::Shift) == Latch::Locked { Tone::Active } else { Tone::Func };
    let mut items: Vec<(f32, Key)> = vec![(1.75, Key::new(KeyAction::CapsLock, "大写", caps_tone).scaled(0.62))];
    for (i, c) in LETTER_ROWS[1].chars().enumerate() {
        items.push((1.0, letter_key(c, 1, i, ctx, true)));
    }
    let row2 = if zh { WIDE_ROW2_ZH } else { WIDE_ROW2_EN };
    for (i, (a, b)) in row2.iter().enumerate() {
        let mut k = sym_key(a, b, WIDE_ROW2_CODES[i], ctx);
        if WIDE_ROW2_CODES[i] == ';' && sp_semicolon(ctx) && !ctx.mods.chording() {
            // 微软 / 搜狗双拼: ； is also the final「ing」.
            k.sub = shuangpin_hint(ctx.sp, ';').map(str::to_string);
        }
        items.push((1.0, k));
    }
    items.push((1.75, enter_key()));
    place(&mut keys, area.x, m.row_y(2), h, unit, items);

    // Row 3: shift (分词 while composing Chinese), z–m, ，。？, shift.
    let left =
        if ctx.chinese && ctx.composing { Key::func(KeyAction::Char('\''), "分词") } else { mod_key(Modifier::Shift, &ctx.mods) };
    let mut items: Vec<(f32, Key)> = vec![(2.25, left)];
    for (i, c) in LETTER_ROWS[2].chars().enumerate() {
        items.push((1.0, letter_key(c, 2, i, ctx, true)));
    }
    let row3 = if zh { WIDE_ROW3_ZH } else { WIDE_ROW3_EN };
    for (i, (a, b)) in row3.iter().enumerate() {
        items.push((1.0, sym_key(a, b, WIDE_ROW3_CODES[i], ctx)));
    }
    items.push((2.25, mod_key(Modifier::Shift, &ctx.mods)));
    place(&mut keys, area.x, m.row_y(3), h, unit, items);

    // Row 4: Ctrl Win Alt Fn 符号 space 中/英 ← ↑↓ → 收起.
    let y4 = m.row_y(4);
    let (left_k, right_k, up_k, down_k) = if fn_on {
        (edit_key(EditKey::Home, "Home"), edit_key(EditKey::End, "End"), edit_key(EditKey::PageUp, "PgUp"), edit_key(EditKey::PageDown, "PgDn"))
    } else {
        (arrow_key(EditKey::Left), arrow_key(EditKey::Right), arrow_key(EditKey::Up), arrow_key(EditKey::Down))
    };
    let items: Vec<(f32, Key)> = vec![
        (1.25, mod_key(Modifier::Ctrl, &ctx.mods)),
        (1.0, mod_key(Modifier::Win, &ctx.mods)),
        (1.0, mod_key(Modifier::Alt, &ctx.mods)),
        (1.0, mod_key(Modifier::Fn, &ctx.mods)),
        (1.25, Key::func(KeyAction::Symbols, "符号")),
        (3.75, space_key(ctx)),
        (1.25, toggle_key()),
        (1.0, left_k),
    ];
    place(&mut keys, area.x, y4, h, unit, items);
    let x_ud = area.x + 11.5 * unit;
    let mut up = up_k;
    up.cell = Rect::new(x_ud, y4, unit, h / 2.0);
    let mut down = down_k;
    down.cell = Rect::new(x_ud, y4 + h / 2.0, unit, h / 2.0);
    keys.push(up);
    keys.push(down);
    place(
        &mut keys,
        x_ud + unit,
        y4,
        h,
        unit,
        vec![(1.0, right_k), (1.0, Key::func(KeyAction::Hide, "收起"))],
    );

    if edit_w > 0.0 {
        let x = area.x + area.w - edit_w;
        let cw = edit_w / 2.0;
        for (row, pair) in edit_area_keys().into_iter().enumerate() {
            for (col, mut k) in pair.into_iter().enumerate() {
                k.cell = Rect::new(x + col as f32 * cw, m.row_y(row), cw, h);
                keys.push(k);
            }
        }
    }
    Built { keys, ..Built::default() }
}

/// The edit area, two keys per row (DESIGN.md §2「常驻编辑区」「选择、复制与剪贴板」).
/// 行首 / 行尾 moved to the selection bar and the Fn layer (Fn + ← →) to make room for 选择 and
/// 剪贴板.
fn edit_area_keys() -> [[Key; 2]; 5] {
    let chord = |c: KeyChord, label: &str| Key::func(KeyAction::Chord(c), label).scaled(0.6);
    let func = |a: KeyAction, label: &str| Key::func(a, label).scaled(0.6);
    [
        [chord(KeyChord::UNDO, "撤销"), chord(KeyChord::REDO, "重做")],
        [func(KeyAction::SelectMode, "选择"), chord(KeyChord::SELECT_ALL, "全选")],
        [chord(KeyChord::COPY, "复制"), chord(KeyChord::PASTE, "粘贴")],
        [chord(KeyChord::CUT, "剪切"), chord(KeyChord::DELETE_WORD, "删词")],
        [func(KeyAction::ClearAll, "清空"), func(KeyAction::ClipPanel, "剪贴板")],
    ]
}

// ---------------------------------------------------------------------------------------------
// 电脑键盘 (TODO #31, DESIGN.md §2「电脑键盘布局」)
// ---------------------------------------------------------------------------------------------

/// (key, with shift) for the symbol keys of the 电脑键盘, by row.
pub const PC_ROW1: [(char, char); 13] = [
    ('`', '~'),
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
];
pub const PC_ROW2_TAIL: [(char, char); 3] = [('[', '{'), (']', '}'), ('\\', '|')];
pub const PC_ROW3_TAIL: [(char, char); 2] = [(';', ':'), ('\'', '"')];
pub const PC_ROW4_TAIL: [(char, char); 3] = [(',', '<'), ('.', '>'), ('/', '?')];

fn raw(code: KeyCode, label: &str) -> Key {
    Key::func(KeyAction::Raw(code), label)
}

fn raw_edit(e: EditKey, label: &str) -> Key {
    raw(KeyCode::Edit(e), label)
}

/// A symbol key: the shifted character small in the top-left corner; with Shift the whole
/// face shows the shifted character.
fn raw_sym(main: char, shifted: char, ctx: &BuildCtx) -> Key {
    let mut k = Key::new(KeyAction::Raw(KeyCode::Char(main)), if ctx.shift() { shifted } else { main }, Tone::Char).scaled(0.85);
    if !ctx.shift() {
        k.top_left = Some(shifted.to_string());
    }
    k
}

fn raw_letter(c: char, ctx: &BuildCtx) -> Key {
    let upper = ctx.shift() != ctx.caps;
    let label = if upper { c.to_ascii_uppercase() } else { c };
    Key::new(KeyAction::Raw(KeyCode::Char(c)), label, Tone::Char).scaled(0.85)
}

fn raw_arrow(e: EditKey) -> Key {
    let mut k = arrow_key(e);
    k.action = KeyAction::Raw(KeyCode::Edit(e));
    k
}

/// The 电脑键盘: six rows like a laptop keyboard, 15 units wide; the function-key row is 3/4 high.
/// Every key is a [`KeyAction::Raw`] key except the modifiers (same tap / double-tap / hold rules as
/// the other layouts). Fn: ← → ↑ ↓ = Home End PgUp PgDn, Del = Insert.
pub(crate) fn build_pc_keyboard(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let unit = area.w / PC_UNITS;
    let row = area.h / (PC_ROWS as f32 - 0.25);
    let f_h = row * 0.75;
    let y = |r: usize| if r == 0 { area.y } else { area.y + f_h + (r - 1) as f32 * row };
    let fn_on = ctx.mods.on(Modifier::Fn);
    let mods = &ctx.mods;
    let mut keys = Vec::new();

    // Row 0: Esc F1–F12 PrtSc Del (Fn: Insert).
    let mut items: Vec<(f32, Key)> = vec![(1.0, raw_edit(EditKey::Escape, "Esc"))];
    items.extend((1..=12).map(|n| (1.0, raw(KeyCode::F(n), &format!("F{n}")))));
    items.push((1.0, raw(KeyCode::PrintScreen, "PrtSc").scaled(0.55)));
    items.push((1.0, if fn_on { raw(KeyCode::Insert, "Ins") } else { raw_edit(EditKey::Delete, "Del") }));
    place(&mut keys, area.x, y(0), f_h, unit, items);

    // Row 1: ` 1–0 - = Backspace.
    let mut items: Vec<(f32, Key)> = PC_ROW1.iter().map(|&(a, b)| (1.0, raw_sym(a, b, ctx))).collect();
    let mut bs = backspace_key();
    bs.action = KeyAction::Raw(KeyCode::Edit(EditKey::Backspace));
    items.push((2.0, bs));
    place(&mut keys, area.x, y(1), row, unit, items);

    // Row 2: Tab q–p [ ] \.
    let mut items: Vec<(f32, Key)> = vec![(1.5, raw_edit(EditKey::Tab, "Tab"))];
    items.extend(LETTER_ROWS[0].chars().map(|c| (1.0, raw_letter(c, ctx))));
    for (i, &(a, b)) in PC_ROW2_TAIL.iter().enumerate() {
        items.push((if i == 2 { 1.5 } else { 1.0 }, raw_sym(a, b, ctx)));
    }
    place(&mut keys, area.x, y(2), row, unit, items);

    // Row 3: Caps a–l ; ' Enter.
    let mut caps = raw(KeyCode::CapsLock, "Caps");
    if ctx.caps {
        caps.tone = Tone::Active;
    }
    let mut items: Vec<(f32, Key)> = vec![(1.75, caps)];
    items.extend(LETTER_ROWS[1].chars().map(|c| (1.0, raw_letter(c, ctx))));
    items.extend(PC_ROW3_TAIL.iter().map(|&(a, b)| (1.0, raw_sym(a, b, ctx))));
    let mut enter = enter_key();
    enter.action = KeyAction::Raw(KeyCode::Edit(EditKey::Enter));
    items.push((2.25, enter));
    place(&mut keys, area.x, y(3), row, unit, items);

    // Row 4: Shift z–m , . / Shift.
    let mut items: Vec<(f32, Key)> = vec![(2.25, mod_key(Modifier::Shift, mods))];
    items.extend(LETTER_ROWS[2].chars().map(|c| (1.0, raw_letter(c, ctx))));
    items.extend(PC_ROW4_TAIL.iter().map(|&(a, b)| (1.0, raw_sym(a, b, ctx))));
    items.push((2.75, mod_key(Modifier::Shift, mods)));
    place(&mut keys, area.x, y(4), row, unit, items);

    // Row 5: Ctrl Win Alt Space Alt Fn Ctrl ← ↑↓ → (inverted T).
    let y5 = y(5);
    let mut space = raw(KeyCode::Char(' '), "");
    space.tone = Tone::Char;
    let items: Vec<(f32, Key)> = vec![
        (1.25, mod_key(Modifier::Ctrl, mods)),
        (1.25, mod_key(Modifier::Win, mods)),
        (1.25, mod_key(Modifier::Alt, mods)),
        (5.25, space),
        (1.0, mod_key(Modifier::Alt, mods)),
        (1.0, mod_key(Modifier::Fn, mods)),
        (1.0, mod_key(Modifier::Ctrl, mods)),
    ];
    place(&mut keys, area.x, y5, row, unit, items);
    let (l, r, u, d) = if fn_on {
        (raw_edit(EditKey::Home, "Home"), raw_edit(EditKey::End, "End"), raw_edit(EditKey::PageUp, "PgUp"), raw_edit(EditKey::PageDown, "PgDn"))
    } else {
        (raw_arrow(EditKey::Left), raw_arrow(EditKey::Right), raw_arrow(EditKey::Up), raw_arrow(EditKey::Down))
    };
    let x_arrows = area.x + 12.0 * unit;
    let mut put = |mut k: Key, x: f32, yy: f32, hh: f32| {
        k.cell = Rect::new(x, yy, unit, hh);
        keys.push(k);
    };
    put(l, x_arrows, y5, row);
    put(u, x_arrows + unit, y5, row / 2.0);
    put(d, x_arrows + unit, y5 + row / 2.0, row / 2.0);
    put(r, x_arrows + 2.0 * unit, y5, row);
    Built { keys, ..Built::default() }
}

// ---------------------------------------------------------------------------------------------
// Clipboard panel (TODO #32)
// ---------------------------------------------------------------------------------------------

/// Header row (返回 … 清空) and the card grid below it.
pub(crate) fn build_clipboard_panel(m: &Metrics) -> Built {
    let area = m.keys_area();
    let head_h = m.row_h * 0.85;
    let bw = (m.row_h * 1.9).min(area.w / 5.0);
    let mut back = Key::func(KeyAction::Back, "返回");
    back.cell = Rect::new(area.x, area.y, bw, head_h);
    let mut clear = Key::func(KeyAction::ClearClips, "清空");
    clear.cell = Rect::new(area.x + area.w - bw, area.y, bw, head_h);
    let grid = Rect::new(area.x, area.y + head_h, area.w, area.h - head_h);
    Built { keys: vec![back, clear], column: None, grid: Some(grid) }
}

/// Column widths for the T9 and number pads: side columns and three main columns.
fn pad_columns(area: Rect) -> (f32, f32) {
    let side = area.w * 0.17;
    let main = (area.w - 2.0 * side) / 3.0;
    (side, main)
}

pub(crate) fn build_t9(m: &Metrics, ctx: &BuildCtx) -> Built {
    if m.wide {
        return build_t9_wide(m, ctx);
    }
    let area = m.keys_area();
    let (side, main) = pad_columns(area);
    let h = m.row_h;
    let x_main = area.x + side;
    let mut keys = t9_main_keys(x_main, main, m.row_y(0), h);
    t9_right_column(&mut keys, x_main + 3.0 * main, side, m.row_y(0), h);
    // Bottom row under the column and the main keys.
    let mut sym = Key::func(KeyAction::Symbols, "符号");
    sym.cell = Rect::new(area.x, m.row_y(3), side, h);
    keys.push(sym);
    t9_bottom(&mut keys, x_main, 3.0 * main, m.row_y(3), h, ctx);
    let column = Rect::new(area.x, m.row_y(0), side, 3.0 * h);
    Built { keys, column: Some((column, ColumnKind::T9)), grid: None }
}

fn t9_main_keys(x_main: f32, main: f32, y0: f32, h: f32) -> Vec<Key> {
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
            k.cell = Rect::new(x_main + col as f32 * main, y0 + row as f32 * h, main, h);
            keys.push(k);
        }
    }
    keys
}

/// ⌫ / 重输 / ⏎ (two rows).
fn t9_right_column(keys: &mut Vec<Key>, x: f32, w: f32, y0: f32, h: f32) {
    let mut bs = backspace_key();
    bs.cell = Rect::new(x, y0, w, h);
    keys.push(bs);
    let mut clear = Key::func(KeyAction::ClearComposition, "重输");
    clear.cell = Rect::new(x, y0 + h, w, h);
    keys.push(clear);
    let mut enter = enter_key();
    enter.cell = Rect::new(x, y0 + 2.0 * h, w, 2.0 * h);
    keys.push(enter);
}

fn t9_bottom(keys: &mut Vec<Key>, x: f32, w: f32, y: f32, h: f32, ctx: &BuildCtx) {
    place(
        keys,
        x,
        y,
        h,
        w / 6.0,
        vec![
            (1.5, Key::func(KeyAction::Numbers, "123")),
            (1.0, Key::icon(KeyAction::LayoutMenu, icon::KEYBOARD, Tone::Func)),
            (2.5, space_key(ctx)),
            (1.0, toggle_key()),
        ],
    );
}

/// Key unit of the fixed-width wide pads (number panel, T9): never stretched past 100 DIPs.
fn pad_unit(area: Rect, units: f32) -> f32 {
    (area.w / units).min(100.0)
}

/// Wide T9: the thumb pad keeps its size; the rest of the width holds a punctuation block and
/// a digit pad (digits are typed directly, not through the 123 panel).
fn build_t9_wide(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    const UNITS: f32 = 1.5 + 5.1 + 1.5 + 0.5 + 2.1 + 0.4 + 3.3;
    let u = pad_unit(area, UNITS);
    let (y0, h) = m.grid_rows(4);
    let side = 1.5 * u;
    let main = 1.7 * u;
    let x_col = area.x + (area.w - UNITS * u) / 2.0;
    let x_main = x_col + side;
    let mut keys = t9_main_keys(x_main, main, y0, h);
    t9_right_column(&mut keys, x_main + 3.0 * main, side, y0, h);
    let mut sym = Key::func(KeyAction::Symbols, "符号");
    sym.cell = Rect::new(x_col, y0 + 3.0 * h, side, h);
    keys.push(sym);
    t9_bottom(&mut keys, x_main, 3.0 * main, y0 + 3.0 * h, h, ctx);
    // Punctuation block, 2 × 4.
    let x_p = x_main + 3.0 * main + side + 0.5 * u;
    let punct = if ctx.zh_punct { WIDE_T9_PUNCT_ZH } else { WIDE_T9_PUNCT_EN };
    for (i, p) in punct.iter().enumerate() {
        let mut k = Key::text(p).scaled(0.85);
        k.tone = Tone::Func;
        k.cell = Rect::new(x_p + (i % 2) as f32 * 1.05 * u, y0 + (i / 2) as f32 * h, 1.05 * u, h);
        keys.push(k);
    }
    // Digit pad, 1 2 3 on top like a phone; 0 is wide.
    let x_d = x_p + 2.1 * u + 0.4 * u;
    let dw = 1.1 * u;
    for (i, d) in ["1", "2", "3", "4", "5", "6", "7", "8", "9"].iter().enumerate() {
        let mut k = Key::text(d).scaled(0.95);
        k.cell = Rect::new(x_d + (i % 3) as f32 * dw, y0 + (i / 3) as f32 * h, dw, h);
        keys.push(k);
    }
    let mut zero = Key::text("0").scaled(0.95);
    zero.cell = Rect::new(x_d, y0 + 3.0 * h, 2.0 * dw, h);
    keys.push(zero);
    let mut dot = Key::text(".").scaled(0.95);
    dot.cell = Rect::new(x_d + 2.0 * dw, y0 + 3.0 * h, dw, h);
    keys.push(dot);
    let column = Rect::new(x_col, y0, side, 3.0 * h);
    Built { keys, column: Some((column, ColumnKind::T9)), grid: None }
}

pub(crate) fn build_numbers(m: &Metrics, ctx: &BuildCtx) -> Built {
    if m.wide {
        return build_numbers_wide(m, ctx);
    }
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
    let mut space = space_key(ctx);
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

/// Wide number panel: fixed-width digit pad in the middle, operators on the left, Tab and the
/// arrows on the right (DESIGN.md §2).
fn build_numbers_wide(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    const UNITS: f32 = 3.75 + 0.5 + 4.8 + 1.6 + 0.5 + 3.3;
    let u = pad_unit(area, UNITS);
    let (y0, h) = m.grid_rows(4);
    let y = |r: usize| y0 + r as f32 * h;
    let x0 = area.x + (area.w - UNITS * u) / 2.0;
    let mut keys = Vec::new();
    let mut put = |mut k: Key, x: f32, row: usize, w: f32, rows: f32| {
        k.cell = Rect::new(x, y(row), w, rows * h);
        keys.push(k);
    };
    // Operators.
    let sw = 1.25 * u;
    for (r, row) in WIDE_NUM_SYMS.iter().enumerate() {
        for (c, sym) in row.iter().enumerate() {
            let mut k = Key::text(sym).scaled(0.9);
            k.tone = Tone::Func;
            put(k, x0 + c as f32 * sw, r, sw, 1.0);
        }
    }
    put(Key::func(KeyAction::Back, "返回"), x0, 3, 2.0 * sw, 1.0);
    let mut comma = Key::text(",").scaled(0.9);
    comma.tone = Tone::Func;
    put(comma, x0 + 2.0 * sw, 3, sw, 1.0);
    // Digits.
    let xd = x0 + 3.75 * u + 0.5 * u;
    let dw = 1.6 * u;
    for i in 0..9 {
        put(Key::text(&(i + 1).to_string()).scaled(1.05), xd + (i % 3) as f32 * dw, i / 3, dw, 1.0);
    }
    put(Key::func(KeyAction::Symbols, "符号"), xd, 3, dw, 1.0);
    put(Key::text("0").scaled(1.05), xd + dw, 3, dw, 1.0);
    let mut space = space_key(ctx);
    space.label = String::new();
    put(space, xd + 2.0 * dw, 3, dw, 1.0);
    let xr = xd + 3.0 * dw;
    put(backspace_key(), xr, 0, dw, 1.0);
    let mut dot = Key::text(".").with_secondary(",", vec![".".into(), ",".into(), "。".into(), "，".into()]);
    dot.corner = None;
    put(dot, xr, 1, dw, 1.0);
    put(enter_key(), xr, 2, dw, 2.0);
    // Tab, symbols, Home/End and the arrows.
    let xa = xr + dw + 0.5 * u;
    let aw = 1.1 * u;
    let currency = if ctx.chinese { "¥" } else { "$" };
    let sym = |s: &str| {
        let mut k = Key::text(s).scaled(0.9);
        k.tone = Tone::Func;
        k
    };
    let rows: [[Key; 3]; 4] = [
        [tab_key(true), sym("@"), sym("#")],
        [sym(currency), sym("&"), sym("_")],
        [edit_key(EditKey::Home, "行首"), arrow_key(EditKey::Up), edit_key(EditKey::End, "行尾")],
        [arrow_key(EditKey::Left), arrow_key(EditKey::Down), arrow_key(EditKey::Right)],
    ];
    for (r, row) in rows.into_iter().enumerate() {
        for (c, k) in row.into_iter().enumerate() {
            put(k, xa + c as f32 * aw, r, aw, 1.0);
        }
    }
    Built { keys, column: None, grid: None }
}

/// The 电脑键 panel: Esc, Tab, modifiers, arrows, F1–F12 for the phone and T9 layouts
/// (DESIGN.md §2「电脑按键」). 12 units wide, 4 rows.
pub(crate) fn build_pc_keys(m: &Metrics, ctx: &BuildCtx) -> Built {
    let area = m.keys_area();
    let u = if m.wide { pad_unit(area, 12.0).max(area.w / 14.5) } else { area.w / 12.0 };
    let x0 = area.x + (area.w - 12.0 * u) / 2.0;
    let (y0, h) = m.grid_rows(4);
    let mut keys = Vec::new();
    let fs: Vec<(f32, Key)> = (1..=12).map(|n| (1.0, f_key(n))).collect();
    place(&mut keys, x0, y0, h, u, fs);
    place(
        &mut keys,
        x0,
        y0 + h,
        h,
        u,
        vec![
            (1.5, edit_key(EditKey::Escape, "Esc")),
            (1.5, edit_key(EditKey::Tab, "Tab")),
            (1.25, edit_key(EditKey::Home, "Home")),
            (1.25, edit_key(EditKey::End, "End")),
            (1.25, edit_key(EditKey::PageUp, "PgUp")),
            (1.25, edit_key(EditKey::PageDown, "PgDn")),
            (1.5, edit_key(EditKey::Delete, "Del")),
            (2.5, backspace_key()),
        ],
    );
    let mods = &ctx.mods;
    let mut space = space_key(ctx);
    space.label = String::new();
    place(
        &mut keys,
        x0,
        y0 + 2.0 * h,
        h,
        u,
        vec![
            (2.0, mod_key(Modifier::Shift, mods)),
            (1.5, mod_key(Modifier::Ctrl, mods)),
            (1.5, mod_key(Modifier::Alt, mods)),
            (1.5, mod_key(Modifier::Win, mods)),
            (1.5, Key::func(KeyAction::Chord(KeyChord::key(KeyCode::PrintScreen)), "PrtSc")),
            (2.0, space),
            (1.0, arrow_key(EditKey::Up)),
            (1.0, enter_key()),
        ],
    );
    let mut row3 = vec![(2.0, Key::func(KeyAction::Back, "返回"))];
    for c in ["`", "-", "=", "[", "]", "\\", "'"] {
        let mut k = Key::text(c).scaled(0.9);
        k.tone = Tone::Func;
        row3.push((1.0, k));
    }
    row3.extend([(1.0, arrow_key(EditKey::Left)), (1.0, arrow_key(EditKey::Down)), (1.0, arrow_key(EditKey::Right))]);
    place(&mut keys, x0, y0 + 3.0 * h, h, u, row3);
    Built { keys, column: None, grid: None }
}

pub(crate) fn build_symbols(m: &Metrics, tab: SymTab) -> Built {
    let area = m.keys_area();
    let bottom = m.rows - 1;
    let units = if m.wide { 14.5 } else { 10.0 };
    let unit = area.w / units;
    let mut keys = Vec::new();
    let tab_key = |t: SymTab, label: &str| {
        let mut k = Key::func(KeyAction::Tab(t), label);
        if t == tab {
            k.tone = Tone::Active;
        }
        k
    };
    let mut items = vec![
        (1.5, Key::func(KeyAction::Back, "返回")),
        (1.2, tab_key(SymTab::Chinese, "中文")),
        (1.2, tab_key(SymTab::English, "英文")),
        (1.2, tab_key(SymTab::Emoji, "表情")),
    ];
    if m.wide {
        items.push((1.2, Key::func(KeyAction::Numbers, "123")));
    }
    let used: f32 = items.iter().map(|(w, _)| w).sum::<f32>() + 1.5;
    items.push((units - used, Key::new(KeyAction::Space, "", Tone::Char).scaled(0.5)));
    items.push((1.5, backspace_key()));
    place(&mut keys, area.x, m.row_y(bottom), m.row_h, unit, items);
    if let Some(k) = keys.iter_mut().find(|k| k.action == KeyAction::Space) {
        k.label = String::new();
    }
    let grid = Rect::new(area.x, m.row_y(0), area.w, bottom as f32 * m.row_h);
    Built { keys, column: None, grid: Some(grid) }
}

/// Right rail of the expanded candidate grid (wide: with page up/down).
pub(crate) fn build_candidate_grid(m: &Metrics) -> Built {
    let area = m.keys_area();
    let rail = (area.w * 0.12).clamp(64.0, 150.0);
    let x = area.x + area.w - rail;
    let mut keys = Vec::new();
    let mut rows: Vec<Vec<Key>> = vec![vec![Key::icon(KeyAction::CollapseCandidates, icon::CHEVRON_UP, Tone::Func)]];
    if m.wide {
        rows.push(vec![Key::func(KeyAction::PageUp, "上页"), Key::func(KeyAction::PageDown, "下页")]);
    }
    rows.push(vec![backspace_key()]);
    rows.push(vec![Key::func(KeyAction::ClearComposition, "重输")]);
    rows.push(vec![enter_key()]);
    for (row, items) in rows.into_iter().enumerate() {
        let w = rail / items.len() as f32;
        for (i, mut k) in items.into_iter().enumerate() {
            k.cell = Rect::new(x + i as f32 * w, m.row_y(row), w, m.row_h);
            keys.push(k);
        }
    }
    let grid = Rect::new(area.x, area.y, area.w - rail, area.h);
    Built { keys, column: None, grid: Some(grid) }
}

/// `voice_mode`: the 语音球 tile is selected and turns voice mode off (「退出语音球」).
pub(crate) fn build_menu(m: &Metrics, current: Layout, sp: ShuangpinScheme, dark: bool, voice_mode: bool) -> Built {
    let area = m.keys_area();
    let tiles: [(KeyAction, &str, &str, bool); 7] = [
        (KeyAction::SetLayout(Layout::Pinyin), "拼", "全拼", current == Layout::Pinyin),
        (KeyAction::SetLayout(Layout::Shuangpin), sp.glyph(), sp.name(), current == Layout::Shuangpin),
        (KeyAction::SetLayout(Layout::T9), "九", "九宫格", current == Layout::T9),
        (KeyAction::SetLayout(Layout::English), "En", "English", current == Layout::English),
        (KeyAction::SetLayout(Layout::Pc), "PC", "电脑键盘", current == Layout::Pc),
        (KeyAction::ToggleTheme, if dark { "☀" } else { "☾" }, if dark { "浅色" } else { "深色" }, false),
        (KeyAction::VoiceBall, icon::MIC, if voice_mode { "退出语音球" } else { "语音球" }, voice_mode),
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
        k.icon = glyph == icon::MIC;
        k.sub = Some(caption.to_string());
        k.selected = selected;
        k.cell = Rect::new(x0 + i as f32 * tile_w, y, tile_w, h);
        keys.push(k);
    }
    let mut back = Key::func(KeyAction::Back, "返回");
    back.cell = Rect::new(area.x + area.w / 2.0 - m.row_h * 1.25, m.row_y(m.rows - 1), m.row_h * 2.5, m.row_h);
    keys.push(back);
    Built { keys, column: None, grid: None }
}
