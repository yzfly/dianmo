//! The settings window's model (dianmo-ui `SettingsModel`) ↔ `settings.ini` + system state.
//!
//! Pure mapping, tested on Linux: [`model`] builds what the settings window, the about page and
//! onboarding show; [`apply`] stores a `Set*` [`SettingsAction`] in [`Settings`]. Making a change
//! take effect (keyboard, ball, autostart, scheduled task …) is up to `app.rs`.

use dianmo_core::Schema;
use dianmo_ui::settings::{
    EngineStatus, LayoutChoice, SettingsAction, SettingsModel, Side, Status, ThemeChoice, UpdateState,
    VoiceEngineChoice, VoiceEngines,
};
use dianmo_ui::{ClipItem, ThemeKind};

use crate::settings::Settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `YYYY-MM-DD`, set by build.rs (empty if unknown).
pub fn build_date() -> &'static str {
    option_env!("DIANMO_BUILD_DATE").unwrap_or("")
}

/// Settings that are saved but only take effect in a later version (shown with a
/// 「下个版本生效」 tag). Row keys of dianmo-ui's settings pages. None in 0.2.
pub const COMING_SOON: [&str; 0] = [];

/// Where to get a missing voice engine.
pub const WETYPE_URL: &str = "https://z.weixin.qq.com/";
pub const DOUBAO_IME_URL: &str = "https://shurufa.doubao.com/pc";

/// What the settings window shows besides `settings.ini`: system state the app keeps up to date
/// (some of it checked in the background).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SysState {
    /// The highest-privilege scheduled task (管理员窗口支持).
    pub admin_task: Status,
    pub engines: VoiceEngines,
    /// librime / 雾凇拼音.
    pub dictionary: Status,
    pub clip_count: usize,
    pub pinned_clips: Vec<ClipItem>,
    pub update: UpdateState,
    /// 模糊音 can be applied (librime is running).
    pub fuzzy_supported: bool,
    /// Applying 模糊音 / its failure (empty: nothing to say).
    pub fuzzy_status: Status,
    /// Words in the user dictionary (`None` = not counted).
    pub user_words: Option<u32>,
    /// Import / export / clear of the user dictionary in progress or done.
    pub user_dict_status: Status,
}

pub fn layout_choice(s: &Settings) -> LayoutChoice {
    if !s.chinese {
        return LayoutChoice::English;
    }
    match s.schema {
        Schema::Pinyin => LayoutChoice::Pinyin,
        Schema::Shuangpin => LayoutChoice::Shuangpin,
        Schema::T9 => LayoutChoice::T9,
    }
}

/// The scheme and 中文 for a layout choice (English keeps the scheme it had).
pub fn layout_schema(l: LayoutChoice, current: Schema) -> (Schema, bool) {
    match l {
        LayoutChoice::Pinyin => (Schema::Pinyin, true),
        LayoutChoice::Shuangpin => (Schema::Shuangpin, true),
        LayoutChoice::T9 => (Schema::T9, true),
        LayoutChoice::English => (current, false),
    }
}

/// The keyboard theme for a choice ([`ThemeChoice::System`] follows the Windows app mode).
pub fn effective_theme(choice: ThemeChoice, system_dark: bool) -> ThemeKind {
    match choice {
        ThemeChoice::Light => ThemeKind::Light,
        ThemeChoice::Dark => ThemeKind::Dark,
        ThemeChoice::System if system_dark => ThemeKind::Dark,
        ThemeChoice::System => ThemeKind::Light,
    }
}

/// Dark title bar for app windows: `None` = follow Windows.
pub fn window_dark(choice: ThemeChoice) -> Option<bool> {
    match choice {
        ThemeChoice::System => None,
        ThemeChoice::Light => Some(false),
        ThemeChoice::Dark => Some(true),
    }
}

/// Floating ball position when the user hasn't dragged it yet (dianmo-win's default).
pub const DEFAULT_BALL_Y: f32 = 0.62;

pub fn model(s: &Settings, sys: &SysState) -> SettingsModel {
    SettingsModel {
        autostart: s.autostart,
        auto_show: s.auto_show,
        input_mode: s.input_mode,
        theme: s.theme,
        admin_task: sys.admin_task.clone(),
        reserve_space: s.appbar,
        layout: layout_choice(s),
        keyboard_height: s.height,
        edit_area: s.edit_area,
        key_popup: s.key_popup,
        key_sound: s.key_sound,
        key_sound_volume: s.key_sound_volume,
        key_sound_style: s.key_sound_style,
        long_press: s.long_press,
        ball: s.show_ball,
        ball_side: if s.ball.is_some_and(|(right, _)| right) { Side::Right } else { Side::Left },
        candidate_size: s.candidate_size,
        full_width_punct: s.full_width_punct,
        space_commits_first: s.space_commits_first,
        fuzzy: s.fuzzy,
        fuzzy_supported: sys.fuzzy_supported,
        fuzzy_status: sys.fuzzy_status.clone(),
        shuangpin: s.shuangpin,
        dictionary: sys.dictionary.clone(),
        user_words: sys.user_words,
        user_dict_status: sys.user_dict_status.clone(),
        voice_engine: voice_choice(&s.voice_engine),
        engines: sys.engines.clone(),
        doubao_exe: s.voice_doubao_exe.clone(),
        clip_history: s.clip_history,
        clip_limit: s.clip_limit,
        clip_skip_passwords: s.clip_skip_passwords,
        clip_count: sys.clip_count,
        pinned_clips: sys.pinned_clips.clone(),
        version: VERSION.to_owned(),
        build_date: build_date().to_owned(),
        update: sys.update.clone(),
        auto_update: s.auto_update,
        coming_soon: COMING_SOON.iter().map(|k| (*k).to_owned()).collect(),
    }
}

pub fn voice_choice(setting: &str) -> VoiceEngineChoice {
    VoiceEngineChoice::ALL.into_iter().find(|e| e.key() == setting).unwrap_or_default()
}

/// Stores a `Set*` action in `s`. Returns whether anything changed. Commands and system state
/// (autostart, the admin task, user dictionary, clipboard entries, updates …) return false:
/// the app handles them.
pub fn apply(s: &mut Settings, a: &SettingsAction) -> bool {
    use SettingsAction as A;
    fn set<T: PartialEq>(slot: &mut T, v: T) -> bool {
        let changed = *slot != v;
        *slot = v;
        changed
    }
    match a {
        A::SetAutoShow(v) => set(&mut s.auto_show, *v),
        A::SetInputMode(v) => set(&mut s.input_mode, *v),
        A::SetTheme(v) => set(&mut s.theme, *v),
        A::SetReserveSpace(v) => set(&mut s.appbar, *v),
        A::SetLayout(l) => {
            let (schema, chinese) = layout_schema(*l, s.schema);
            set(&mut s.schema, schema) | set(&mut s.chinese, chinese)
        }
        A::SetKeyboardHeight(h) => set(&mut s.height, if h.is_finite() { h.clamp(0.7, 1.5) } else { 1.0 }),
        A::SetEditArea(v) => set(&mut s.edit_area, *v),
        A::SetKeyPopup(v) => set(&mut s.key_popup, *v),
        A::SetKeySound(v) => set(&mut s.key_sound, *v),
        A::SetKeySoundVolume(v) => set(&mut s.key_sound_volume, *v),
        A::SetKeySoundStyle(v) => set(&mut s.key_sound_style, *v),
        A::SetLongPress(v) => set(&mut s.long_press, *v),
        A::SetBall(v) => set(&mut s.show_ball, *v),
        A::SetBallSide(side) => {
            let y = s.ball.map_or(DEFAULT_BALL_Y, |(_, y)| y);
            set(&mut s.ball, Some((*side == Side::Right, y)))
        }
        A::SetCandidateSize(v) => set(&mut s.candidate_size, *v),
        A::SetFullWidthPunct(v) => set(&mut s.full_width_punct, *v),
        A::SetSpaceCommitsFirst(v) => set(&mut s.space_commits_first, *v),
        A::SetFuzzy(p, v) => set(&mut s.fuzzy[p.index()], *v),
        A::SetShuangpin(v) => set(&mut s.shuangpin, *v),
        A::SetVoiceEngine(e) => set(&mut s.voice_engine, e.key().to_owned()),
        A::ResetDoubaoExe => set(&mut s.voice_doubao_exe, String::new()),
        A::SetClipHistory(v) => set(&mut s.clip_history, *v),
        A::SetClipLimit(n) => set(&mut s.clip_limit, (*n).clamp(1, 1000)),
        A::SetClipSkipPasswords(v) => set(&mut s.clip_skip_passwords, *v),
        A::SetAutoUpdate(v) => set(&mut s.auto_update, *v),
        _ => false,
    }
}

/// How a voice engine shows in the settings, from the background check: `None` = not checked
/// yet (「正在检测…」), `Some(None)` = usable, `Some(Some(why))` = not usable.
pub fn engine_status(e: VoiceEngineChoice, check: Option<Option<&str>>) -> EngineStatus {
    let download_url = match e {
        VoiceEngineChoice::WeType => Some(WETYPE_URL.to_owned()),
        VoiceEngineChoice::DoubaoIme => Some(DOUBAO_IME_URL.to_owned()),
        _ => None,
    };
    let note = match e {
        VoiceEngineChoice::WeType | VoiceEngineChoice::DoubaoIme => {
            "以管理员身份运行的窗口收不到它的语音快捷键（Windows 权限隔离），请在普通窗口里说话".to_owned()
        }
        _ => String::new(),
    };
    match check {
        None => EngineStatus { note, download_url, ..EngineStatus::default() },
        Some(None) => EngineStatus {
            available: true,
            detail: match e {
                VoiceEngineChoice::System => "Windows 自带，无需安装".to_owned(),
                _ => "已就绪".to_owned(),
            },
            note,
            download_url,
        },
        Some(Some(why)) => EngineStatus {
            available: false,
            detail: why.trim().trim_end_matches(['。', '.']).to_owned(),
            note,
            // Only offer the download when it isn't installed at all.
            download_url: download_url.filter(|_| ["没有安装", "未安装", "找不到"].iter().any(|w| why.contains(w))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dianmo_ui::settings::{CandidateSize, FuzzyPair, InputMode, KeySoundStyle, KeySoundVolume, LongPress, ShuangpinScheme};

    #[test]
    fn model_reflects_settings_and_system() {
        let s = Settings {
            schema: Schema::Shuangpin,
            theme: ThemeChoice::Dark,
            input_mode: InputMode::VoiceBall,
            appbar: false,
            ball: Some((true, 0.3)),
            voice_engine: "doubao_ime".into(),
            ..Settings::default()
        };
        let sys = SysState { clip_count: 7, admin_task: Status::ok("x"), ..SysState::default() };
        let m = model(&s, &sys);
        assert_eq!(m.layout, LayoutChoice::Shuangpin);
        assert_eq!(m.theme, ThemeChoice::Dark);
        assert_eq!(m.input_mode, InputMode::VoiceBall);
        assert!(!m.reserve_space);
        assert_eq!(m.ball_side, Side::Right);
        assert_eq!(m.voice_engine, VoiceEngineChoice::DoubaoIme);
        assert_eq!(m.clip_count, 7);
        assert_eq!(m.admin_task, Status::ok("x"));
        assert_eq!(m.version, VERSION);
        assert!(m.coming_soon.is_empty(), "everything takes effect in 0.2");
        let english = Settings { chinese: false, ..s };
        assert_eq!(model(&english, &sys).layout, LayoutChoice::English);
        assert_eq!(model(&Settings::default(), &sys).ball_side, Side::Left, "the ball starts on the left");
    }

    #[test]
    fn every_engine_maps_both_ways() {
        for e in VoiceEngineChoice::ALL {
            let mut s = Settings::default();
            apply(&mut s, &SettingsAction::SetVoiceEngine(e));
            assert_eq!(voice_choice(&s.voice_engine), e);
            assert_eq!(Settings::parse(&s.serialize()).voice_engine, e.key(), "the ini accepts it");
        }
        assert_eq!(voice_choice("bogus"), VoiceEngineChoice::WeType);
    }

    #[test]
    fn applying_actions_round_trips_through_the_model() {
        use SettingsAction as A;
        let actions = [
            A::SetAutoShow(false),
            A::SetInputMode(InputMode::PcKeyboard),
            A::SetTheme(ThemeChoice::Light),
            A::SetReserveSpace(false),
            A::SetLayout(LayoutChoice::T9),
            A::SetKeyboardHeight(1.25),
            A::SetEditArea(false),
            A::SetKeyPopup(false),
            A::SetKeySound(true),
            A::SetKeySoundVolume(KeySoundVolume::Low),
            A::SetKeySoundStyle(KeySoundStyle::Soft),
            A::SetLongPress(LongPress::Short),
            A::SetBall(false),
            A::SetBallSide(Side::Right),
            A::SetCandidateSize(CandidateSize::Large),
            A::SetFullWidthPunct(false),
            A::SetSpaceCommitsFirst(false),
            A::SetFuzzy(FuzzyPair::NL, true),
            A::SetShuangpin(ShuangpinScheme::Ziranma),
            A::SetClipHistory(false),
            A::SetClipLimit(100),
            A::SetClipSkipPasswords(false),
            A::SetAutoUpdate(false),
        ];
        let mut s = Settings::default();
        let mut expected = model(&s, &SysState::default());
        for a in &actions {
            assert!(apply(&mut s, a), "{a:?} changes something");
            assert!(!apply(&mut s, a), "{a:?} twice changes nothing");
            expected.apply(a);
        }
        // The view's own idea of the change (`SettingsModel::apply`) matches what we store.
        assert_eq!(model(&s, &SysState::default()), expected);
        // And it survives the ini.
        assert_eq!(model(&Settings::parse(&s.serialize()), &SysState::default()), expected);
        assert_eq!(s.ball, Some((true, DEFAULT_BALL_Y)));
        // English keeps the scheme.
        apply(&mut s, &A::SetLayout(LayoutChoice::English));
        assert_eq!((s.schema, s.chinese), (Schema::T9, false));
        assert!(!apply(&mut s, &A::CheckUpdate), "commands are the app's");
    }

    #[test]
    fn theme_resolution() {
        assert_eq!(effective_theme(ThemeChoice::System, true), ThemeKind::Dark);
        assert_eq!(effective_theme(ThemeChoice::System, false), ThemeKind::Light);
        assert_eq!(effective_theme(ThemeChoice::Light, true), ThemeKind::Light);
        assert_eq!(window_dark(ThemeChoice::System), None);
        assert_eq!(window_dark(ThemeChoice::Dark), Some(true));
    }

    #[test]
    fn engine_status_texts() {
        let unknown = engine_status(VoiceEngineChoice::WeType, None);
        assert!(!unknown.available && unknown.detail.is_empty(), "UI shows 正在检测…");
        let ok = engine_status(VoiceEngineChoice::System, Some(None));
        assert!(ok.available && !ok.detail.is_empty());
        let missing = engine_status(VoiceEngineChoice::WeType, Some(Some("没有安装微信输入法。")));
        assert_eq!(missing.detail, "没有安装微信输入法");
        assert_eq!(missing.download_url.as_deref(), Some(WETYPE_URL));
        let off = engine_status(VoiceEngineChoice::DoubaoIme, Some(Some("豆包输入法没有运行")));
        assert!(off.download_url.is_none(), "installed: no download link");
    }
}
