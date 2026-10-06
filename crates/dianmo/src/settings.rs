//! `%APPDATA%\Dianmo\settings.ini`: plain `key=value` lines (no sections), UTF-8.
//!
//! Unknown keys and malformed lines are ignored, so older/newer versions can share the file.
//! Besides user preferences it holds the system touch keyboard settings Dianmo replaced
//! (`saved_*`): they are written before Dianmo changes the registry and cleared after restoring,
//! so a crash is repaired on the next start.

use std::fmt::Write as _;
use std::path::Path;

use dianmo_core::Schema;
use dianmo_ui::ThemeKind;

/// One registry DWORD as it was before Dianmo touched it (`None` = value absent).
pub type SavedDword = Option<u32>;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub schema: Schema,
    pub theme: ThemeKind,
    pub chinese: bool,
    /// Reserve screen space under the keyboard (AppBar).
    pub appbar: bool,
    /// Mirrors the `HKCU\...\Run` entry (the registry is the source of truth).
    pub autostart: bool,
    /// Keyboard height multiplier (0.7..=1.5).
    pub height: f32,
    /// Pop up on touch in an edit field / hide when touching elsewhere.
    pub auto_show: bool,
    /// Wide (landscape) layout: two-column edit area on the right (撤销、复制、粘贴 …).
    pub edit_area: bool,
    /// The 电脑键盘 (pass-through PC keyboard) layout was showing last time.
    pub pc_keyboard: bool,
    /// System touch keyboard settings to restore on exit: `(EnableDesktopModeAutoInvoke,
    /// TouchKeyboardTapInvoke)`. `Some` while Dianmo runs (or after it crashed).
    pub saved_tabtip: Option<(SavedDword, SavedDword)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: Schema::Pinyin,
            theme: ThemeKind::Light,
            chinese: true,
            appbar: true,
            autostart: false,
            height: 1.0,
            auto_show: true,
            edit_area: true,
            pc_keyboard: false,
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

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let (mut saved_a, mut saved_b) = (None, None);
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "schema" => s.schema = parse_schema(v).unwrap_or(s.schema),
                "theme" => {
                    s.theme = match v {
                        "dark" => ThemeKind::Dark,
                        "light" => ThemeKind::Light,
                        _ => s.theme,
                    }
                }
                "chinese" => s.chinese = parse_bool(v).unwrap_or(s.chinese),
                "appbar" => s.appbar = parse_bool(v).unwrap_or(s.appbar),
                "autostart" => s.autostart = parse_bool(v).unwrap_or(s.autostart),
                "auto_show" => s.auto_show = parse_bool(v).unwrap_or(s.auto_show),
                "edit_area" => s.edit_area = parse_bool(v).unwrap_or(s.edit_area),
                "pc_keyboard" => s.pc_keyboard = parse_bool(v).unwrap_or(s.pc_keyboard),
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
        if let (Some(a), Some(b)) = (saved_a, saved_b) {
            s.saved_tabtip = Some((a, b));
        }
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# 点墨 Dianmo 设置（key=value；删掉本文件即恢复默认）\n");
        let theme = if self.theme == ThemeKind::Dark { "dark" } else { "light" };
        let _ = writeln!(out, "schema={}", schema_name(self.schema));
        let _ = writeln!(out, "theme={theme}");
        let _ = writeln!(out, "chinese={}", self.chinese);
        let _ = writeln!(out, "appbar={}", self.appbar);
        let _ = writeln!(out, "autostart={}", self.autostart);
        let _ = writeln!(out, "auto_show={}", self.auto_show);
        let _ = writeln!(out, "edit_area={}", self.edit_area);
        let _ = writeln!(out, "pc_keyboard={}", self.pc_keyboard);
        let _ = writeln!(out, "height={}", self.height);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            schema: Schema::T9,
            theme: ThemeKind::Dark,
            chinese: false,
            appbar: false,
            autostart: true,
            height: 1.25,
            auto_show: false,
            edit_area: false,
            pc_keyboard: true,
            saved_tabtip: Some((None, Some(1))),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
        let d = Settings::default();
        assert_eq!(Settings::parse(&d.serialize()), d);
    }

    #[test]
    fn tolerant_parsing() {
        let s = Settings::parse("\u{feff}schema = shuangpin\r\n[x]\ngarbage\ntheme=purple\nheight=9\nchinese=no\n");
        assert_eq!(s.schema, Schema::Shuangpin);
        assert_eq!(s.theme, ThemeKind::Light);
        assert_eq!(s.height, 1.5);
        assert!(!s.chinese);
        assert!(s.edit_area, "default on");
        assert!(!Settings::parse("edit_area=off").edit_area);
        assert!(!s.pc_keyboard, "default off");
        assert!(Settings::parse("pc_keyboard=1").pc_keyboard);
        assert_eq!(s.saved_tabtip, None);
        // Half a saved pair is ignored.
        assert_eq!(Settings::parse("saved_desktop_mode_auto_invoke=0\n").saved_tabtip, None);
        assert_eq!(Settings::parse("height=NaN").height, 1.0);
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
