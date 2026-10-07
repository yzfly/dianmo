use dianmo_core::{Action, Candidate, EditKey, Engine, InputController, KeyChord, KeyCode, Schema, Snapshot, TextSink};

/// Tiny pinyin engine: whole-input words first, then words for a prefix of the input.
struct FakeEngine {
    schema: Schema,
    raw: String,
}

const DICT: &[(&str, &str)] = &[("nihao", "你好"), ("nihao", "拟好"), ("ni", "你"), ("ni", "泥"), ("hao", "好")];

impl FakeEngine {
    fn new() -> Self {
        Self { schema: Schema::Pinyin, raw: String::new() }
    }

    /// (text, input bytes it consumes), longest match first.
    fn matches(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<_> = DICT
            .iter()
            .filter(|(py, _)| self.raw.starts_with(py))
            .map(|(py, w)| (*w, py.len()))
            .collect();
        out.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        out
    }

    fn snap(&self, commit: Option<String>) -> Snapshot {
        Snapshot {
            preedit: self.raw.clone(),
            candidates: self.matches().into_iter().map(|(w, _)| Candidate::new(w)).collect(),
            commit,
        }
    }
}

impl Engine for FakeEngine {
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
        let Some((word, n)) = self.matches().get(index).copied() else { return self.snap(None) };
        self.raw.drain(..n);
        self.snap(Some(word.to_string()))
    }
    fn commit_raw(&mut self) -> Snapshot {
        let raw = std::mem::take(&mut self.raw);
        self.snap(Some(raw))
    }
    fn clear(&mut self) -> Snapshot {
        self.raw.clear();
        self.snap(None)
    }
    fn candidates(&mut self, start: usize, count: usize) -> Vec<Candidate> {
        self.snap(None).candidates.into_iter().skip(start).take(count).collect()
    }
    fn snapshot(&mut self) -> Snapshot {
        self.snap(None)
    }
}

#[derive(Default)]
struct Sink {
    log: Vec<String>,
}

impl TextSink for Sink {
    fn commit_text(&mut self, text: &str) {
        self.log.push(text.to_string());
    }
    fn send_key(&mut self, key: EditKey) {
        self.log.push(format!("<{key:?}>"));
    }
    fn send_chord(&mut self, chord: KeyChord) {
        let mut name = String::new();
        for (on, m) in [(chord.ctrl, "C-"), (chord.shift, "S-"), (chord.alt, "A-"), (chord.win, "W-")] {
            if on {
                name.push_str(m);
            }
        }
        match chord.key {
            Some(KeyCode::Char(c)) => name.push(c),
            Some(k) => name.push_str(&format!("{k:?}")),
            None => {}
        }
        self.log.push(format!("<{name}>"));
    }
    fn key_event(&mut self, key: KeyCode, down: bool) {
        self.log.push(format!("<{key:?}{}>", if down { "↓" } else { "↑" }));
    }
}

fn controller() -> InputController<FakeEngine, Sink> {
    InputController::new(FakeEngine::new(), Sink::default())
}

fn type_str(c: &mut InputController<FakeEngine, Sink>, s: &str) {
    for ch in s.chars() {
        c.handle(Action::Char(ch));
    }
}

fn log(c: &mut InputController<FakeEngine, Sink>) -> Vec<String> {
    c.sink_mut().log.clone()
}

#[test]
fn letters_compose_and_space_commits_first_candidate() {
    let mut c = controller();
    type_str(&mut c, "nihao");
    assert_eq!(c.state().preedit, "nihao");
    assert_eq!(c.state().candidates[0].text, "你好");
    assert!(log(&mut c).is_empty());
    c.handle(Action::Space);
    assert_eq!(log(&mut c), ["你好"]);
    assert!(!c.is_composing());
}

#[test]
fn partial_selection_keeps_composing_rest() {
    let mut c = controller();
    type_str(&mut c, "nihao");
    let idx = c.state().candidates.iter().position(|x| x.text == "泥").unwrap();
    c.handle(Action::Select(idx));
    assert_eq!(c.state().preedit, "hao");
    c.handle(Action::Select(0));
    assert_eq!(log(&mut c), ["泥", "好"]);
    assert!(!c.is_composing());
}

#[test]
fn enter_commits_raw_while_composing_and_is_enter_otherwise() {
    let mut c = controller();
    type_str(&mut c, "nih");
    c.handle(Action::Enter);
    c.handle(Action::Enter);
    assert_eq!(log(&mut c), ["nih", "<Enter>"]);
}

#[test]
fn backspace_edits_composition_then_app() {
    let mut c = controller();
    type_str(&mut c, "ni");
    c.handle(Action::Backspace);
    assert_eq!(c.state().preedit, "n");
    c.handle(Action::Backspace);
    assert!(!c.is_composing());
    c.handle(Action::Backspace);
    assert_eq!(log(&mut c), ["<Backspace>"]);
}

#[test]
fn punctuation_commits_top_candidate_first() {
    let mut c = controller();
    type_str(&mut c, "nihao");
    c.handle(Action::Char('，'));
    assert_eq!(log(&mut c), ["你好", "，"]);
}

#[test]
fn text_and_edit_keys_commit_composition_first() {
    let mut c = controller();
    type_str(&mut c, "hao");
    c.handle(Action::Text("😀".into()));
    type_str(&mut c, "ni");
    c.handle(Action::Edit(EditKey::Left));
    assert_eq!(log(&mut c), ["好", "😀", "你", "<Left>"]);
}

#[test]
fn unmatched_input_falls_back_to_raw() {
    let mut c = controller();
    type_str(&mut c, "xq");
    assert!(c.state().candidates.is_empty());
    c.handle(Action::Space);
    assert_eq!(log(&mut c), ["xq"]);
}

#[test]
fn english_mode_and_uppercase_bypass_engine() {
    let mut c = controller();
    c.handle(Action::Char('N'));
    c.handle(Action::ToggleChinese);
    type_str(&mut c, "hi");
    c.handle(Action::Space);
    assert_eq!(log(&mut c), ["N", "h", "i", " "]);
}

#[test]
fn toggle_commits_raw_composition() {
    let mut c = controller();
    type_str(&mut c, "ni");
    c.handle(Action::ToggleChinese);
    assert!(!c.is_chinese());
    assert_eq!(log(&mut c), ["ni"]);
}

#[test]
fn t9_digits_compose_only_in_t9() {
    let mut c = controller();
    c.handle(Action::Char('6'));
    assert_eq!(log(&mut c), ["6"]);
    c.handle(Action::SetSchema(Schema::T9));
    c.handle(Action::Char('6'));
    assert_eq!(c.state().preedit, "6");
}

#[test]
fn clear_drops_composition_silently() {
    let mut c = controller();
    type_str(&mut c, "nihao");
    c.handle(Action::ClearComposition);
    assert!(!c.is_composing());
    assert!(log(&mut c).is_empty());
}

#[test]
fn chords_commit_composition_first() {
    let mut c = controller();
    type_str(&mut c, "nihao");
    c.handle(Action::Key(KeyChord::SELECT_ALL));
    assert!(!c.is_composing());
    c.handle(Action::Key(KeyChord::COPY));
    c.handle(Action::Key(KeyChord { shift: true, ..KeyChord::ctrl('t') }));
    c.handle(Action::Key(KeyChord::DELETE_WORD));
    c.handle(Action::Key(KeyChord::win_alone()));
    c.handle(Action::Key(KeyChord::key(KeyCode::F(5))));
    assert_eq!(log(&mut c), ["你好", "<C-a>", "<C-c>", "<C-S-t>", "<C-Edit(Backspace)>", "<W->", "<F(5)>"]);
    assert!(KeyChord::UNDO.has_modifier());
    // Raw key events: a press commits the composition first.
    let mut c = controller();
    type_str(&mut c, "ni");
    c.handle(Action::KeyDown(KeyCode::Shift));
    c.handle(Action::KeyDown(KeyCode::Char('a')));
    c.handle(Action::KeyUp(KeyCode::Char('a')));
    c.handle(Action::KeyUp(KeyCode::Shift));
    assert_eq!(log(&mut c), ["你", "<Shift↓>", "<Char('a')↓>", "<Char('a')↑>", "<Shift↑>"]);
    assert!(!KeyChord::key(KeyCode::F(1)).has_modifier());
}

#[test]
fn space_without_commit_first_types_the_raw_input_and_a_space() {
    let mut c = controller();
    assert!(c.space_commits_first(), "default on");
    c.set_space_commits_first(false);
    type_str(&mut c, "nihao");
    c.handle(Action::Space);
    assert_eq!(log(&mut c), ["nihao", " "]);
    assert!(!c.is_composing());
    // Idle: a plain space either way.
    c.handle(Action::Space);
    assert_eq!(log(&mut c), ["nihao", " ", " "]);
    // Back on: the first candidate.
    c.set_space_commits_first(true);
    type_str(&mut c, "ni");
    c.handle(Action::Space);
    assert_eq!(log(&mut c).last().map(String::as_str), Some("你"));
}

#[test]
fn full_width_punctuation_follows_the_setting() {
    let mut c = controller();
    assert!(c.full_width_punct(), "default on");
    type_str(&mut c, "hao");
    c.handle(Action::Char(','));
    c.handle(Action::Char('.'));
    c.handle(Action::Char('"'));
    assert_eq!(log(&mut c), ["好", "，", "。", "\""], "quotes are left alone");
    c.set_full_width_punct(false);
    c.handle(Action::Char(','));
    // Text keys (the keyboard's own punctuation, the symbol panel) are never converted.
    c.handle(Action::Text("，".into()));
    assert_eq!(log(&mut c)[4..], [",", "，"]);
    // English mode: always ASCII.
    c.set_full_width_punct(true);
    c.handle(Action::ToggleChinese);
    c.handle(Action::Char('?'));
    assert_eq!(log(&mut c).last().map(String::as_str), Some("?"));
    assert_eq!(dianmo_core::full_width('\\'), Some("、"));
    assert_eq!(dianmo_core::full_width('a'), None);
}

#[test]
fn semicolon_is_a_final_only_where_the_scheme_says_so() {
    let mut c = controller();
    c.handle(Action::SetSchema(Schema::Shuangpin));
    c.handle(Action::Char('x'));
    c.handle(Action::Text("；".into()));
    // Off (小鹤 / 自然码): punctuation, the composition is committed first.
    assert_eq!(log(&mut c), ["x", "；"]);
    c.set_semicolon_input(true);
    c.handle(Action::Char('x'));
    c.handle(Action::Text("；".into()));
    assert_eq!(c.state().preedit, "x;", "微软 / 搜狗: x; = xing");
    c.handle(Action::Char(';'));
    assert_eq!(c.state().preedit, "x;;");
    c.handle(Action::ClearComposition);
    // Not composing: still punctuation.
    c.handle(Action::Text("；".into()));
    assert_eq!(log(&mut c).last().map(String::as_str), Some("；"));
    // 全拼 never takes it.
    c.handle(Action::SetSchema(Schema::Pinyin));
    type_str(&mut c, "ni");
    c.handle(Action::Text("；".into()));
    assert!(!c.is_composing());
}
