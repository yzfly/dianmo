//! Phone-style input rules. The keyboard UI turns touches into [`Action`]s; this is the only
//! place that decides what an action means for the engine and the focused app.

use crate::engine::{Engine, Schema, Snapshot};
use crate::sink::{EditKey, KeyChord, KeyCode, TextSink};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A character key. In Chinese mode, schema input goes to the engine; anything else
    /// (punctuation, digits on 26-key layouts, uppercase letters) is committed as text.
    Char(char),
    /// Text from the symbol panel, emoji, clipboard, ...
    Text(String),
    Backspace,
    Space,
    Enter,
    /// Pick a candidate by absolute index.
    Select(usize),
    /// T9: lock the leading digits to a spelling from the left column.
    PickSpelling(String),
    Edit(EditKey),
    /// A key combination (edit keys, Ctrl/Alt/Win/Fn), pressed and released at once.
    /// The composition is committed first.
    Key(KeyChord),
    /// Raw key press / release (pass-through layouts; the app's IME sees real keys). A press
    /// commits the composition first.
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    /// Drop the composition (swipe left on backspace).
    ClearComposition,
    ToggleChinese,
    SetSchema(Schema),
}

pub struct InputController<E, S> {
    engine: E,
    sink: S,
    chinese: bool,
    state: Snapshot,
    /// 空格上屏首选 (setting, default on). Off: Space while composing commits the raw input and
    /// then a space (DESIGN.md §2「空格规则」).
    space_commits_first: bool,
    /// 中文时用全角标点 (setting, default on): an ASCII punctuation `Char` in Chinese mode is
    /// committed as its full-width form. (The keyboard's own punctuation keys follow the same
    /// setting in dianmo-ui.)
    full_width_punct: bool,
    /// 微软 / 搜狗双拼 type the final「ing」 with `;`: while composing in Chinese 双拼, `;` (or the
    /// key's full-width `；`) goes to the engine.
    semicolon_input: bool,
}

/// Full-width form of an ASCII punctuation character in Chinese mode (`,` → `，`). Quotes are
/// left alone (which of “ ” is meant depends on context).
pub fn full_width(c: char) -> Option<&'static str> {
    Some(match c {
        ',' => "，",
        '.' => "。",
        '?' => "？",
        '!' => "！",
        ':' => "：",
        ';' => "；",
        '(' => "（",
        ')' => "）",
        '[' => "【",
        ']' => "】",
        '<' => "《",
        '>' => "》",
        '\\' => "、",
        '~' => "～",
        '^' => "……",
        '_' => "——",
        '$' => "￥",
        _ => return None,
    })
}

impl<E: Engine, S: TextSink> InputController<E, S> {
    pub fn new(engine: E, sink: S) -> Self {
        Self {
            engine,
            sink,
            chinese: true,
            state: Snapshot::default(),
            space_commits_first: true,
            full_width_punct: true,
            semicolon_input: false,
        }
    }

    /// 空格上屏首选 (see the field). Takes effect with the next key.
    pub fn set_space_commits_first(&mut self, on: bool) {
        self.space_commits_first = on;
    }

    pub fn space_commits_first(&self) -> bool {
        self.space_commits_first
    }

    /// 中文时用全角标点 (see the field).
    pub fn set_full_width_punct(&mut self, on: bool) {
        self.full_width_punct = on;
    }

    pub fn full_width_punct(&self) -> bool {
        self.full_width_punct
    }

    /// `;` is part of the 双拼 scheme (微软 / 搜狗: ing).
    pub fn set_semicolon_input(&mut self, on: bool) {
        self.semicolon_input = on;
    }

    /// `c` (or the semicolon key's text) goes to the engine as `;` right now.
    fn semicolon_composes(&self, c: char) -> bool {
        self.semicolon_input
            && matches!(c, ';' | '；')
            && self.chinese
            && self.engine.schema() == Schema::Shuangpin
            && self.is_composing()
    }

    /// What the candidate bar shows.
    pub fn state(&self) -> &Snapshot {
        &self.state
    }

    pub fn is_chinese(&self) -> bool {
        self.chinese
    }

    pub fn schema(&self) -> Schema {
        self.engine.schema()
    }

    pub fn is_composing(&self) -> bool {
        self.state.is_composing()
    }

    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }

    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    pub fn handle(&mut self, action: Action) -> &Snapshot {
        match action {
            Action::Char(c) => {
                if (self.chinese && (self.engine.schema().accepts(c) || (c == '\'' && self.is_composing())))
                    || self.semicolon_composes(c)
                {
                    let s = self.engine.input(if c == '；' { ';' } else { c });
                    self.apply(s);
                } else {
                    self.commit_default();
                    match full_width(c).filter(|_| self.chinese && self.full_width_punct) {
                        Some(fw) => self.sink.commit_text(fw),
                        None => self.sink.commit_text(c.encode_utf8(&mut [0; 4])),
                    }
                }
            }
            Action::Text(text) => {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && self.semicolon_composes(c)
                {
                    let s = self.engine.input(';');
                    self.apply(s);
                } else {
                    self.commit_default();
                    self.sink.commit_text(&text);
                }
            }
            Action::Backspace => {
                if self.is_composing() {
                    let s = self.engine.backspace();
                    self.apply(s);
                } else {
                    self.sink.send_key(EditKey::Backspace);
                }
            }
            Action::Space => {
                if self.is_composing() && self.space_commits_first {
                    self.commit_default();
                } else if self.is_composing() {
                    // 空格上屏首选 off: what was typed, as typed, then the space (mixed
                    // Chinese / English typing).
                    let s = self.engine.commit_raw();
                    self.apply(s);
                    self.sink.commit_text(" ");
                } else {
                    self.sink.commit_text(" ");
                }
            }
            Action::Enter => {
                if self.is_composing() {
                    let s = self.engine.commit_raw();
                    self.apply(s);
                } else {
                    self.sink.send_key(EditKey::Enter);
                }
            }
            Action::Select(index) => {
                if self.is_composing() {
                    let s = self.engine.select(index);
                    self.apply(s);
                }
            }
            Action::PickSpelling(spelling) => {
                if self.is_composing() {
                    let s = self.engine.pick_t9_spelling(&spelling);
                    self.apply(s);
                }
            }
            Action::Edit(key) => {
                // Cursor keys would desync the app from our composition; commit it first.
                self.commit_default();
                self.sink.send_key(key);
            }
            Action::Key(chord) => {
                // Same as cursor keys: the app must see the composed text before e.g. Ctrl+A.
                self.commit_default();
                self.sink.send_chord(chord);
            }
            Action::KeyDown(key) => {
                self.commit_default();
                self.sink.key_event(key, true);
            }
            Action::KeyUp(key) => self.sink.key_event(key, false),
            Action::ClearComposition => {
                let s = self.engine.clear();
                self.apply(s);
            }
            Action::ToggleChinese => {
                if self.is_composing() {
                    let s = self.engine.commit_raw();
                    self.apply(s);
                }
                self.chinese = !self.chinese;
            }
            Action::SetSchema(schema) => {
                self.engine.set_schema(schema);
                self.state = self.engine.snapshot();
            }
        }
        &self.state
    }

    /// Commits the first candidate (or the raw input if there is none). No-op when idle.
    fn commit_default(&mut self) {
        while self.is_composing() {
            let s = if self.state.candidates.is_empty() { self.engine.commit_raw() } else { self.engine.select(0) };
            let progressed = s.commit.is_some() || s.preedit != self.state.preedit;
            self.apply(s);
            if !progressed {
                let s = self.engine.commit_raw();
                self.apply(s);
                break;
            }
        }
    }

    fn apply(&mut self, mut s: Snapshot) {
        if let Some(text) = s.commit.take()
            && !text.is_empty() {
                self.sink.commit_text(&text);
            }
        self.state = s;
    }
}
