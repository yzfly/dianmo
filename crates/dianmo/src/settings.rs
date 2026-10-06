//! `%APPDATA%\Dianmo\settings.ini`: plain `key=value` lines (no sections), UTF-8.
//!
//! Unknown keys and malformed lines are ignored, so older/newer versions can share the file.
//! Besides user preferences it holds the system touch keyboard settings Dianmo replaced
//! (`saved_*`): they are written before Dianmo changes the registry and cleared after restoring,
//! so a crash is repaired on the next start.
//!
//! What the settings window shows and changes is mapped in `prefs.rs`.

use std::fmt::Write as _;
use std::path::Path;

use dianmo_core::Schema;
use dianmo_ui::settings::{CandidateSize, FuzzyPair, InputMode, LongPress, ShuangpinScheme, ThemeChoice};

/// One registry DWORD as it was before Dianmo touched it (`None` = value absent).
pub type SavedDword = Option<u32>;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub schema: Schema,
    /// 跟随系统 / 浅色 / 深色 (the keyboard and the settings window).
    pub theme: ThemeChoice,
    /// 中文 (false = the English layout).
    pub chinese: bool,
    /// Reserve screen space under the keyboard (AppBar).
    pub appbar: bool,
    /// Mirrors the task's logon trigger / the `HKCU\...\Run` entry (the system is the source of
    /// truth).
    pub autostart: bool,
    /// Keyboard height multiplier (0.7..=1.5).
    pub height: f32,
    /// Pop up on touch in an edit field / hide when touching elsewhere.
    pub auto_show: bool,
    /// Wide (landscape) layout: two-column edit area on the right (撤销、复制、粘贴 …).
    pub edit_area: bool,
    /// 键盘 / 语音球 (the floating ball starts/stops voice input; the keyboard doesn't pop up for
    /// text fields) / 电脑键盘 (pass-through PC keys). Replaces the old `voice_mode` and
    /// `pc_keyboard` keys, which are still read.
    pub input_mode: InputMode,
    /// Voice engine: `wetype` (微信输入法, default) | `doubao_ime` (豆包输入法) | `doubao` (third-party
    /// 豆包语音) | `system` (Win+H).
    /// Kept as the setting string; `voice::VoiceEngine::from_setting` parses it.
    pub voice_engine: String,
    /// 豆包语音's exe (empty = look in the usual places).
    pub voice_doubao_exe: String,
    /// Floating ball position: (on the right edge, centre height as a fraction of the work area).
    /// `None` until the user moves it.
    pub ball: Option<(bool, f32)>,
    /// Show the floating ball while the keyboard is hidden (always shown in voice mode).
    pub show_ball: bool,
    /// Bubble above pressed keys.
    pub key_popup: bool,
    /// Key click sound (not implemented yet; stored).
    pub key_sound: bool,
    pub long_press: LongPress,
    /// Stored; takes effect in a later version.
    pub candidate_size: CandidateSize,
    /// Stored; takes effect in a later version.
    pub full_width_punct: bool,
    /// Stored; takes effect in a later version.
    pub space_commits_first: bool,
    /// 模糊音, indexed by `FuzzyPair::index` (stored; wired into the rime schema later).
    pub fuzzy: [bool; 7],
    pub shuangpin: ShuangpinScheme,
    /// Record clipboard history.
    pub clip_history: bool,
    /// Unpinned clipboard entries kept.
    pub clip_limit: u32,
    /// Don't record copies while a password field has focus.
    pub clip_skip_passwords: bool,
    /// Check GitHub for a new version (P6).
    pub auto_update: bool,
    /// When updates were last checked (unix seconds; 0 = never).
    pub last_update_check: u64,
    /// The first-run onboarding was finished or dismissed.
    pub onboarded: bool,
    /// System touch keyboard settings to restore on exit: `(EnableDesktopModeAutoInvoke,
    /// TouchKeyboardTapInvoke)`. `Some` while Dianmo runs (or after it crashed).
    pub saved_tabtip: Option<(SavedDword, SavedDword)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: Schema::Pinyin,
            theme: ThemeChoice::System,
            chinese: true,
            appbar: true,
            autostart: false,
            height: 1.0,
            auto_show: true,
            edit_area: true,
            input_mode: InputMode::Keyboard,
            voice_engine: "wetype".to_owned(),
            voice_doubao_exe: String::new(),
            ball: None,
            show_ball: true,
            key_popup: true,
            key_sound: false,
            long_press: LongPress::Medium,
            candidate_size: CandidateSize::Standard,
            full_width_punct: true,
            space_commits_first: true,
            fuzzy: [false; 7],
            shuangpin: ShuangpinScheme::Xiaohe,
            clip_history: true,
            clip_limit: 50,
            clip_skip_passwords: true,
            auto_update: true,
            last_update_check: 0,
            onboarded: false,
            saved_tabtip: None,
        }
    }
}

pub fn schema_name(s: Schema) -> &'static str {
    match s {
        Schema::Pinyin => "pinyin",
        Schema::Shuangpin => "shuangpin",
        Schema::T9 => "t9",
    }
}

fn parse_schema(v: &str) -> Option<Schema> {
    match v {
        "pinyin" => Some(Schema::Pinyin),
        "shuangpin" | "flypy" => Some(Schema::Shuangpin),
        "t9" => Some(Schema::T9),
        _ => None,
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_saved(v: &str) -> Option<SavedDword> {
    if v == "absent" { Some(None) } else { v.parse().ok().map(Some) }
}

fn fmt_saved(v: SavedDword) -> String {
    v.map_or_else(|| "absent".to_owned(), |n| n.to_string())
}

/// `(value, setting string)` tables for the enum settings.
const THEMES: [(ThemeChoice, &str); 3] = [(ThemeChoice::System, "system"), (ThemeChoice::Light, "light"), (ThemeChoice::Dark, "dark")];
const MODES: [(InputMode, &str); 3] =
    [(InputMode::Keyboard, "keyboard"), (InputMode::VoiceBall, "voice"), (InputMode::PcKeyboard, "pc")];
const LONG_PRESS: [(LongPress, &str); 3] = [(LongPress::Short, "short"), (LongPress::Medium, "medium"), (LongPress::Long, "long")];
const CANDIDATE_SIZES: [(CandidateSize, &str); 4] = [
    (CandidateSize::Small, "small"),
    (CandidateSize::Standard, "standard"),
    (CandidateSize::Large, "large"),
    (CandidateSize::ExtraLarge, "xlarge"),
];
const SHUANGPIN: [(ShuangpinScheme, &str); 3] =
    [(ShuangpinScheme::Xiaohe, "xiaohe"), (ShuangpinScheme::Ziranma, "ziranma"), (ShuangpinScheme::Microsoft, "microsoft")];
/// `fuzzy=` lists the enabled pairs by these names (`FuzzyPair::ALL` order).
const FUZZY: [&str; 7] = ["z-zh", "c-ch", "s-sh", "n-l", "an-ang", "en-eng", "in-ing"];

fn lookup<T: Copy>(table: &[(T, &str)], v: &str) -> Option<T> {
    table.iter().find(|(_, s)| s.eq_ignore_ascii_case(v)).map(|(t, _)| *t)
}

fn name_of<T: Copy + PartialEq>(table: &[(T, &'static str)], t: T) -> &'static str {
    table.iter().find(|(x, _)| *x == t).map_or("", |(_, s)| s)
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let (mut saved_a, mut saved_b) = (None, None);
        let (mut ball_edge, mut ball_y) = (None, None);
        // Before `input_mode` existed (≤ 0.1): two switches.
        let (mut mode, mut voice_mode, mut pc_keyboard) = (None, false, false);
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "schema" => s.schema = parse_schema(v).unwrap_or(s.schema),
                "theme" => s.theme = lookup(&THEMES, v).unwrap_or(s.theme),
                "chinese" => s.chinese = parse_bool(v).unwrap_or(s.chinese),
                "appbar" => s.appbar = parse_bool(v).unwrap_or(s.appbar),
                "autostart" => s.autostart = parse_bool(v).unwrap_or(s.autostart),
                "auto_show" => s.auto_show = parse_bool(v).unwrap_or(s.auto_show),
                "edit_area" => s.edit_area = parse_bool(v).unwrap_or(s.edit_area),
                "input_mode" => mode = lookup(&MODES, v),
                "pc_keyboard" => pc_keyboard = parse_bool(v).unwrap_or(false),
                "voice_mode" => voice_mode = parse_bool(v).unwrap_or(false),
                "voice_engine" => {
                    let v = v.to_ascii_lowercase();
                    if matches!(v.as_str(), "wetype" | "doubao_ime" | "doubao" | "system") {
                        s.voice_engine = v;
                    }
                }
                "voice_doubao_exe" => s.voice_doubao_exe = v.to_owned(),
                "ball_edge" => {
                    ball_edge = match v {
                        "left" => Some(false),
                        "right" => Some(true),
                        _ => None,
                    }
                }
                "ball_y" => ball_y = v.parse::<f32>().ok().filter(|y| y.is_finite()).map(|y| y.clamp(0.0, 1.0)),
                "show_ball" => s.show_ball = parse_bool(v).unwrap_or(s.show_ball),
                "key_popup" => s.key_popup = parse_bool(v).unwrap_or(s.key_popup),
                "key_sound" => s.key_sound = parse_bool(v).unwrap_or(s.key_sound),
                "long_press" => s.long_press = lookup(&LONG_PRESS, v).unwrap_or(s.long_press),
                "candidate_size" => s.candidate_size = lookup(&CANDIDATE_SIZES, v).unwrap_or(s.candidate_size),
                "full_width_punct" => s.full_width_punct = parse_bool(v).unwrap_or(s.full_width_punct),
                "space_commits_first" => s.space_commits_first = parse_bool(v).unwrap_or(s.space_commits_first),
                "fuzzy" => {
                    s.fuzzy = [false; 7];
                    for p in v.split(',').map(str::trim) {
                        if let Some(i) = FUZZY.iter().position(|f| f.eq_ignore_ascii_case(p)) {
                            s.fuzzy[i] = true;
                        }
                    }
                }
                "shuangpin" => s.shuangpin = lookup(&SHUANGPIN, v).unwrap_or(s.shuangpin),
                "clip_history" => s.clip_history = parse_bool(v).unwrap_or(s.clip_history),
                "clip_limit" => {
                    if let Ok(n) = v.parse::<u32>() {
                        s.clip_limit = n.clamp(1, 1000);
                    }
                }
                "clip_skip_passwords" => s.clip_skip_passwords = parse_bool(v).unwrap_or(s.clip_skip_passwords),
                "auto_update" => s.auto_update = parse_bool(v).unwrap_or(s.auto_update),
                "last_update_check" => s.last_update_check = v.parse().unwrap_or(s.last_update_check),
                "onboarded" => s.onboarded = parse_bool(v).unwrap_or(s.onboarded),
                "height" => {
                    if let Ok(h) = v.parse::<f32>()
                        && h.is_finite()
                    {
                        s.height = h.clamp(0.7, 1.5);
                    }
                }
                "saved_desktop_mode_auto_invoke" => saved_a = parse_saved(v),
                "saved_touch_keyboard_tap_invoke" => saved_b = parse_saved(v),
                _ => {}
            }
        }
        s.input_mode = mode.unwrap_or(if voice_mode {
            InputMode::VoiceBall
        } else if pc_keyboard {
            InputMode::PcKeyboard
        } else {
            InputMode::Keyboard
        });
        if let (Some(a), Some(b)) = (saved_a, saved_b) {
            s.saved_tabtip = Some((a, b));
        }
        if let (Some(right), Some(y)) = (ball_edge, ball_y) {
            s.ball = Some((right, y));
        }
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# 点墨 Dianmo 设置（key=value；删掉本文件即恢复默认）\n");
        let _ = writeln!(out, "schema={}", schema_name(self.schema));
        let _ = writeln!(out, "theme={}", name_of(&THEMES, self.theme));
        let _ = writeln!(out, "chinese={}", self.chinese);
        let _ = writeln!(out, "appbar={}", self.appbar);
        let _ = writeln!(out, "autostart={}", self.autostart);
        let _ = writeln!(out, "auto_show={}", self.auto_show);
        let _ = writeln!(out, "edit_area={}", self.edit_area);
        let _ = writeln!(out, "input_mode={}", name_of(&MODES, self.input_mode));
        let _ = writeln!(out, "height={}", self.height);
        let _ = writeln!(out, "voice_engine={}", self.voice_engine);
        if !self.voice_doubao_exe.is_empty() {
            let _ = writeln!(out, "voice_doubao_exe={}", self.voice_doubao_exe);
        }
        if let Some((right, y)) = self.ball {
            let _ = writeln!(out, "ball_edge={}", if right { "right" } else { "left" });
            let _ = writeln!(out, "ball_y={y:.4}");
        }
        let _ = writeln!(out, "show_ball={}", self.show_ball);
        let _ = writeln!(out, "key_popup={}", self.key_popup);
        let _ = writeln!(out, "key_sound={}", self.key_sound);
        let _ = writeln!(out, "long_press={}", name_of(&LONG_PRESS, self.long_press));
        let _ = writeln!(out, "candidate_size={}", name_of(&CANDIDATE_SIZES, self.candidate_size));
        let _ = writeln!(out, "full_width_punct={}", self.full_width_punct);
        let _ = writeln!(out, "space_commits_first={}", self.space_commits_first);
        let fuzzy: Vec<&str> = FuzzyPair::ALL.iter().filter(|p| self.fuzzy[p.index()]).map(|p| FUZZY[p.index()]).collect();
        let _ = writeln!(out, "fuzzy={}", fuzzy.join(","));
        let _ = writeln!(out, "shuangpin={}", name_of(&SHUANGPIN, self.shuangpin));
        let _ = writeln!(out, "clip_history={}", self.clip_history);
        let _ = writeln!(out, "clip_limit={}", self.clip_limit);
        let _ = writeln!(out, "clip_skip_passwords={}", self.clip_skip_passwords);
        let _ = writeln!(out, "auto_update={}", self.auto_update);
        if self.last_update_check != 0 {
            let _ = writeln!(out, "last_update_check={}", self.last_update_check);
        }
        let _ = writeln!(out, "onboarded={}", self.onboarded);
        if let Some((a, b)) = self.saved_tabtip {
            out.push_str("# 点墨运行期间替换掉的系统触摸键盘设置，退出时恢复\n");
            let _ = writeln!(out, "saved_desktop_mode_auto_invoke={}", fmt_saved(a));
            let _ = writeln!(out, "saved_touch_keyboard_tap_invoke={}", fmt_saved(b));
        }
        out
    }

    /// Missing or unreadable file → defaults.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    /// Writes via a temp file + rename so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("ini.tmp");
        std::fs::write(&tmp, self.serialize())?;
        std::fs::rename(&tmp, path)
    }

    /// 「恢复默认设置」: user preferences back to defaults. Kept: what mirrors the system
    /// (autostart, the saved touch keyboard values), whether onboarding was done, and the ball's
    /// position.
    pub fn reset_preferences(&mut self) {
        *self = Settings {
            autostart: self.autostart,
            onboarded: self.onboarded,
            last_update_check: self.last_update_check,
            saved_tabtip: self.saved_tabtip,
            ball: self.ball,
            ..Settings::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            schema: Schema::T9,
            theme: ThemeChoice::Dark,
            chinese: false,
            appbar: false,
            autostart: true,
            height: 1.25,
            auto_show: false,
            edit_area: false,
            input_mode: InputMode::PcKeyboard,
            voice_engine: "doubao".to_owned(),
            voice_doubao_exe: r"C:\Tools\DouBaoVoice 1.2.exe".to_owned(),
            ball: Some((true, 0.25)),
            show_ball: false,
            key_popup: false,
            key_sound: true,
            long_press: LongPress::Long,
            candidate_size: CandidateSize::ExtraLarge,
            full_width_punct: false,
            space_commits_first: false,
            fuzzy: [true, false, false, true, false, false, true],
            shuangpin: ShuangpinScheme::Microsoft,
            clip_history: false,
            clip_limit: 200,
            clip_skip_passwords: false,
            auto_update: false,
            last_update_check: 1_790_000_000,
            onboarded: true,
            saved_tabtip: Some((None, Some(1))),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
        let d = Settings::default();
        assert_eq!(Settings::parse(&d.serialize()), d);
        for mode in InputMode::ALL {
            let s = Settings { input_mode: mode, ..Settings::default() };
            assert_eq!(Settings::parse(&s.serialize()).input_mode, mode);
        }
    }

    #[test]
    fn tolerant_parsing() {
        let s = Settings::parse("\u{feff}schema = shuangpin\r\n[x]\ngarbage\ntheme=purple\nheight=9\nchinese=no\n");
        assert_eq!(s.schema, Schema::Shuangpin);
        assert_eq!(s.theme, ThemeChoice::System, "default: follow Windows");
        assert_eq!(Settings::parse("theme=dark").theme, ThemeChoice::Dark);
        assert_eq!(Settings::parse("theme=light").theme, ThemeChoice::Light);
        assert_eq!(s.height, 1.5);
        assert!(!s.chinese);
        assert!(s.edit_area, "default on");
        assert!(!Settings::parse("edit_area=off").edit_area);
        assert_eq!(s.saved_tabtip, None);
        // Half a saved pair is ignored.
        assert_eq!(Settings::parse("saved_desktop_mode_auto_invoke=0\n").saved_tabtip, None);
        assert_eq!(Settings::parse("height=NaN").height, 1.0);
        assert_eq!(s.voice_engine, "wetype", "default engine");
        assert_eq!(Settings::parse("voice_engine=System").voice_engine, "system");
        assert_eq!(Settings::parse("voice_engine=bogus").voice_engine, "wetype");
        assert_eq!(Settings::parse("voice_engine=doubao_ime").voice_engine, "doubao_ime");
        assert_eq!(s.ball, None);
        assert_eq!(Settings::parse("ball_edge=left\nball_y=7").ball, Some((false, 1.0)));
        assert_eq!(Settings::parse("ball_y=0.3").ball, None, "both keys needed");
        assert!(!s.onboarded);
        assert_eq!(Settings::parse("fuzzy=n-l, bogus ,IN-ING").fuzzy, [false, false, false, true, false, false, true]);
        assert_eq!(Settings::parse("clip_limit=0").clip_limit, 1);
        assert_eq!(Settings::parse("long_press=quick").long_press, LongPress::Medium);
    }

    #[test]
    fn input_mode_replaces_the_old_switches() {
        assert_eq!(Settings::parse("").input_mode, InputMode::Keyboard);
        assert_eq!(Settings::parse("voice_mode=on").input_mode, InputMode::VoiceBall);
        assert_eq!(Settings::parse("pc_keyboard=1").input_mode, InputMode::PcKeyboard);
        assert_eq!(Settings::parse("pc_keyboard=1\nvoice_mode=true").input_mode, InputMode::VoiceBall);
        assert_eq!(Settings::parse("voice_mode=true\ninput_mode=pc").input_mode, InputMode::PcKeyboard, "new key wins");
        let text = Settings::parse("voice_mode=true").serialize();
        assert!(text.contains("input_mode=voice") && !text.contains("voice_mode"));
    }

    #[test]
    fn reset_keeps_system_state() {
        let mut s = Settings {
            autostart: true,
            onboarded: true,
            height: 1.3,
            theme: ThemeChoice::Dark,
            ball: Some((true, 0.4)),
            saved_tabtip: Some((Some(1), None)),
            ..Settings::default()
        };
        s.reset_preferences();
        assert!(s.autostart && s.onboarded);
        assert_eq!(s.height, 1.0);
        assert_eq!(s.theme, ThemeChoice::System);
        assert_eq!(s.ball, Some((true, 0.4)));
        assert_eq!(s.saved_tabtip, Some((Some(1), None)));
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("dianmo-settings-test-{}", std::process::id()));
        let path = dir.join("settings.ini");
        let s = Settings { schema: Schema::Shuangpin, ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        assert_eq!(Settings::load(&dir.join("missing.ini")), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
