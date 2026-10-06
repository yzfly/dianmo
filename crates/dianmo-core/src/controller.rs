//! Phone-style input rules. The keyboard UI turns touches into [`Action`]s; this is the only
//! place that decides what an action means for the engine and the focused app.

use crate::engine::{Engine, Schema, Snapshot};
use crate::sink::{EditKey, TextSink};

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
}

impl<E: Engine, S: TextSink> InputController<E, S> {
    pub fn new(engine: E, sink: S) -> Self {
        Self { engine, sink, chinese: true, state: Snapshot::default() }
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
                if self.chinese && (self.engine.schema().accepts(c) || (c == '\'' && self.is_composing())) {
                    let s = self.engine.input(c);
                    self.apply(s);
                } else {
                    self.commit_default();
                    self.sink.commit_text(c.encode_utf8(&mut [0; 4]));
                }
            }
            Action::Text(text) => {
                self.commit_default();
                self.sink.commit_text(&text);
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
                if self.is_composing() {
                    self.commit_default();
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
        if let Some(text) = s.commit.take() {
            if !text.is_empty() {
                self.sink.commit_text(&text);
            }
        }
        self.state = s;
    }
}
