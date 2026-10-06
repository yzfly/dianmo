//! Built-in stand-in engine used when librime can't be loaded (`rime.dll` missing, data broken).
//! It keeps the keyboard usable: what you type is offered as the only candidate and committed
//! as is. The candidate comment says why there are no Chinese words.

use dianmo_core::{Candidate, Engine, Schema, Snapshot};

pub const NO_DICT: &str = "词库未加载";
pub const LOADING: &str = "词库加载中";

pub struct BasicEngine {
    schema: Schema,
    buf: String,
    note: &'static str,
}

impl BasicEngine {
    pub fn new(schema: Schema) -> Self {
        Self { schema, buf: String::new(), note: NO_DICT }
    }

    /// Stand-in while librime starts in the background.
    pub fn loading(schema: Schema) -> Self {
        Self { note: LOADING, ..Self::new(schema) }
    }

    #[cfg_attr(not(feature = "rime"), allow(dead_code))]
    /// Changes the candidate comment (e.g. loading → not loaded).
    pub fn set_note(&mut self, note: &'static str) {
        self.note = note;
    }

    fn state(&self, commit: Option<String>) -> Snapshot {
        let candidates = if self.buf.is_empty() {
            Vec::new()
        } else {
            vec![Candidate { text: self.buf.clone(), comment: Some(self.note.to_owned()) }]
        };
        Snapshot { preedit: self.buf.clone(), candidates, commit }
    }

    fn commit_all(&mut self) -> Snapshot {
        let text = std::mem::take(&mut self.buf);
        self.state(Some(text))
    }
}

impl Engine for BasicEngine {
    fn schema(&self) -> Schema {
        self.schema
    }

    fn set_schema(&mut self, schema: Schema) {
        self.schema = schema;
        self.buf.clear();
    }

    fn input(&mut self, c: char) -> Snapshot {
        if c != '\'' || !self.buf.is_empty() {
            self.buf.push(c);
        }
        self.state(None)
    }

    fn backspace(&mut self) -> Snapshot {
        self.buf.pop();
        self.state(None)
    }

    fn select(&mut self, index: usize) -> Snapshot {
        if index == 0 && !self.buf.is_empty() { self.commit_all() } else { self.state(None) }
    }

    fn commit_raw(&mut self) -> Snapshot {
        self.commit_all()
    }

    fn clear(&mut self) -> Snapshot {
        self.buf.clear();
        self.state(None)
    }

    fn candidates(&mut self, start: usize, count: usize) -> Vec<Candidate> {
        self.state(None).candidates.into_iter().skip(start).take(count).collect()
    }

    fn snapshot(&mut self) -> Snapshot {
        self.state(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dianmo_core::{Action, EditKey, InputController, TextSink};

    #[derive(Default)]
    struct Rec(String);
    impl TextSink for Rec {
        fn commit_text(&mut self, t: &str) {
            self.0.push_str(t);
        }
        fn send_key(&mut self, k: EditKey) {
            self.0.push_str(&format!("<{k:?}>"));
        }
    }

    #[test]
    fn types_letters_through_the_controller() {
        let mut c = InputController::new(BasicEngine::new(Schema::Pinyin), Rec::default());
        for ch in "nihao".chars() {
            c.handle(Action::Char(ch));
        }
        assert_eq!(c.state().preedit, "nihao");
        assert_eq!(c.state().candidates[0].comment.as_deref(), Some(NO_DICT));
        c.handle(Action::Backspace);
        c.handle(Action::Char('，'));
        c.handle(Action::Char('x'));
        c.handle(Action::Select(0));
        c.handle(Action::Char('y'));
        c.handle(Action::Enter);
        c.handle(Action::Char('z'));
        c.handle(Action::Space);
        assert!(!c.is_composing());
        assert_eq!(c.sink_mut().0, "niha，xyz");
        assert_eq!(c.engine_mut().candidates(0, 10), vec![]);
    }
}
