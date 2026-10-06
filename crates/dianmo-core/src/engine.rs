//! The conversion engine seen by the rest of Dianmo (implemented by `dianmo-rime`).

/// Input schema. Each maps to a Rime schema shipped with Dianmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Schema {
    /// 全拼, 26 keys.
    Pinyin,
    /// 小鹤双拼, 26 keys.
    Shuangpin,
    /// 九宫格拼音: digits 2-9.
    T9,
}

impl Schema {
    /// Whether `c` is composition input for this schema (as opposed to text to commit directly).
    pub fn accepts(self, c: char) -> bool {
        match self {
            Schema::Pinyin | Schema::Shuangpin => c.is_ascii_lowercase(),
            Schema::T9 => matches!(c, '2'..='9'),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    /// Annotation shown next to the text, e.g. the full spelling for an abbreviation.
    pub comment: Option<String>,
}

impl Candidate {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), comment: None }
    }
}

/// Engine state after an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// What the user has typed, formatted for display ("ni hao"). Empty when not composing.
    pub preedit: String,
    /// The first batch of candidates, in absolute order (index 0 is the default choice).
    pub candidates: Vec<Candidate>,
    /// Text the operation produced for the target app. The controller sends it and clears it.
    pub commit: Option<String>,
}

impl Snapshot {
    pub fn is_composing(&self) -> bool {
        !self.preedit.is_empty()
    }
}

/// A conversion engine session. All calls happen on one thread.
pub trait Engine {
    fn schema(&self) -> Schema;
    /// Switches schema; drops any composition.
    fn set_schema(&mut self, schema: Schema);
    /// Feeds one character that `schema().accepts()` (or `'\''` as a syllable separator).
    fn input(&mut self, c: char) -> Snapshot;
    fn backspace(&mut self) -> Snapshot;
    /// Selects a candidate by absolute index. May commit only part of the input,
    /// leaving the rest composing (e.g. picking a single character).
    fn select(&mut self, index: usize) -> Snapshot;
    /// Commits the raw input as typed (phone behaviour of Enter while composing).
    fn commit_raw(&mut self) -> Snapshot;
    fn clear(&mut self) -> Snapshot;
    /// Candidates `start..start + count` for the expanded candidate grid.
    fn candidates(&mut self, start: usize, count: usize) -> Vec<Candidate>;
    /// T9 only: spellings the leading digits can stand for ("ni", "mi", ...), for the left column.
    fn t9_spellings(&mut self) -> Vec<String> {
        Vec::new()
    }
    /// T9 only: locks the leading digits to `spelling`.
    fn pick_t9_spelling(&mut self, _spelling: &str) -> Snapshot {
        self.snapshot()
    }
    /// Current state without changing anything.
    fn snapshot(&mut self) -> Snapshot;
}
