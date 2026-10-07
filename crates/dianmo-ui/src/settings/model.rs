//! What the settings window shows ([`SettingsModel`]) and what it asks the host to do
//! ([`SettingsAction`]). Plain data: the host fills the model from `settings.ini` and the system
//! (scheduled task, registry, installed voice engines…) and applies the actions.

use crate::view::ClipItem;

/// Settings pages, in navigation order (PRODUCT.md §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    General,
    Keyboard,
    Input,
    Voice,
    Clipboard,
    About,
}

impl Page {
    pub const ALL: [Page; 6] = [Page::General, Page::Keyboard, Page::Input, Page::Voice, Page::Clipboard, Page::About];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "常规",
            Page::Keyboard => "键盘",
            Page::Input => "输入",
            Page::Voice => "语音",
            Page::Clipboard => "剪贴板",
            Page::About => "关于",
        }
    }

    /// Segoe MDL2 / Fluent icon code point.
    pub fn icon(self) -> &'static str {
        match self {
            Page::General => "\u{E713}",
            Page::Keyboard => "\u{E765}",
            Page::Input => "\u{E8D2}",
            Page::Voice => "\u{E720}",
            Page::Clipboard => "\u{E77F}",
            Page::About => "\u{E946}",
        }
    }
}

/// Green / yellow / red dot in front of a status line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// Grey: not checked yet.
    #[default]
    Unknown,
    Ok,
    /// Needs attention (yellow); usually comes with a fix button.
    Warn,
    Error,
}

/// A status with one sentence of explanation, e.g. `Status::ok("已注册，可在管理员窗口中使用")`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub level: Level,
    pub text: String,
}

impl Status {
    pub fn new(level: Level, text: impl Into<String>) -> Self {
        Self { level, text: text.into() }
    }

    pub fn ok(text: impl Into<String>) -> Self {
        Self::new(Level::Ok, text)
    }

    pub fn warn(text: impl Into<String>) -> Self {
        Self::new(Level::Warn, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(Level::Error, text)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

/// Default keyboard layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutChoice {
    #[default]
    Pinyin,
    /// 双拼 (scheme in [`SettingsModel::shuangpin`]).
    Shuangpin,
    T9,
    English,
}

/// How Dianmo appears (常规 › 输入模式). Exactly one at a time; also switchable from the tray and
/// the keyboard (语音球 button, 电脑键盘 tile).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    /// The touch keyboard (layout = [`SettingsModel::layout`]).
    #[default]
    Keyboard,
    /// Only the floating voice ball: tap to talk, long-press to expand the keyboard.
    VoiceBall,
    /// 电脑键盘: pass-through PC keys, the app's own IME does the typing.
    PcKeyboard,
}

impl InputMode {
    pub const ALL: [InputMode; 3] = [InputMode::Keyboard, InputMode::VoiceBall, InputMode::PcKeyboard];

    pub fn name(self) -> &'static str {
        match self {
            InputMode::Keyboard => "键盘",
            InputMode::VoiceBall => "语音球",
            InputMode::PcKeyboard => "电脑键盘",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            InputMode::Keyboard => "像手机一样的触屏键盘，点输入框自动弹出",
            InputMode::VoiceBall => "键盘缩成悬浮球，点一下说话、长按展开键盘",
            InputMode::PcKeyboard => "完整的实体键盘布局，按键直通，由当前应用的输入法处理",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LongPress {
    /// 250 ms
    Short,
    /// 350 ms (current default)
    #[default]
    Medium,
    /// 500 ms
    Long,
}

impl LongPress {
    pub fn millis(self) -> u64 {
        match self {
            LongPress::Short => 250,
            LongPress::Medium => 350,
            LongPress::Long => 500,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CandidateSize {
    Small,
    #[default]
    Standard,
    Large,
    ExtraLarge,
}

impl CandidateSize {
    pub const ALL: [CandidateSize; 4] =
        [CandidateSize::Small, CandidateSize::Standard, CandidateSize::Large, CandidateSize::ExtraLarge];

    /// Candidate text size relative to the standard one (the candidate bar keeps its height).
    pub fn scale(self) -> f32 {
        match self {
            CandidateSize::Small => 0.85,
            CandidateSize::Standard => 1.0,
            CandidateSize::Large => 1.15,
            CandidateSize::ExtraLarge => 1.3,
        }
    }
}

/// 双拼方案. The keyboard shows the scheme's finals under the letters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShuangpinScheme {
    /// 小鹤双拼 (default).
    #[default]
    Xiaohe,
    /// 自然码.
    Ziranma,
    /// 微软双拼 (`;` = ing).
    Microsoft,
    /// 搜狗双拼 (`;` = ing).
    Sogou,
}

impl ShuangpinScheme {
    pub const ALL: [ShuangpinScheme; 4] =
        [ShuangpinScheme::Xiaohe, ShuangpinScheme::Ziranma, ShuangpinScheme::Microsoft, ShuangpinScheme::Sogou];

    /// Full name: the space bar, the layout menu, the tray.
    pub fn name(self) -> &'static str {
        match self {
            ShuangpinScheme::Xiaohe => "小鹤双拼",
            ShuangpinScheme::Ziranma => "自然码双拼",
            ShuangpinScheme::Microsoft => "微软双拼",
            ShuangpinScheme::Sogou => "搜狗双拼",
        }
    }

    /// Short name (settings segments).
    pub fn short(self) -> &'static str {
        match self {
            ShuangpinScheme::Xiaohe => "小鹤",
            ShuangpinScheme::Ziranma => "自然码",
            ShuangpinScheme::Microsoft => "微软",
            ShuangpinScheme::Sogou => "搜狗",
        }
    }

    /// One character for the layout menu tile.
    pub fn glyph(self) -> &'static str {
        match self {
            ShuangpinScheme::Xiaohe => "鹤",
            ShuangpinScheme::Ziranma => "自",
            ShuangpinScheme::Microsoft => "微",
            ShuangpinScheme::Sogou => "搜",
        }
    }

    /// The scheme types the final「ing」 with `;` (an extra key on the keyboard).
    pub fn uses_semicolon(self) -> bool {
        matches!(self, ShuangpinScheme::Microsoft | ShuangpinScheme::Sogou)
    }
}

/// 按键音 volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeySoundVolume {
    Low,
    #[default]
    Medium,
    High,
}

impl KeySoundVolume {
    pub const ALL: [KeySoundVolume; 3] = [KeySoundVolume::Low, KeySoundVolume::Medium, KeySoundVolume::High];

    /// Linear gain applied to the samples.
    pub fn gain(self) -> f32 {
        match self {
            KeySoundVolume::Low => 0.3,
            KeySoundVolume::Medium => 0.6,
            KeySoundVolume::High => 1.0,
        }
    }
}

/// 按键音 sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeySoundStyle {
    /// 清脆: a short bright click.
    #[default]
    Crisp,
    /// 柔和: a soft low tap.
    Soft,
}

impl KeySoundStyle {
    pub const ALL: [KeySoundStyle; 2] = [KeySoundStyle::Crisp, KeySoundStyle::Soft];
}

/// One fuzzy-pinyin pair (模糊音).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzyPair {
    ZZh,
    CCh,
    SSh,
    NL,
    AnAng,
    EnEng,
    InIng,
}

impl FuzzyPair {
    pub const ALL: [FuzzyPair; 7] =
        [FuzzyPair::ZZh, FuzzyPair::CCh, FuzzyPair::SSh, FuzzyPair::NL, FuzzyPair::AnAng, FuzzyPair::EnEng, FuzzyPair::InIng];

    pub fn label(self) -> &'static str {
        match self {
            FuzzyPair::ZZh => "z = zh",
            FuzzyPair::CCh => "c = ch",
            FuzzyPair::SSh => "s = sh",
            FuzzyPair::NL => "n = l",
            FuzzyPair::AnAng => "an = ang",
            FuzzyPair::EnEng => "en = eng",
            FuzzyPair::InIng => "in = ing",
        }
    }

    /// Index into [`SettingsModel::fuzzy`].
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceEngineChoice {
    /// 微信输入法 (default).
    #[default]
    WeType,
    /// 豆包输入法 (ByteDance's IME, its global voice shortcut).
    DoubaoIme,
    /// 豆包语音 (third-party DouBaoVoice tool).
    Doubao,
    /// Windows voice typing (Win+H).
    System,
}

impl VoiceEngineChoice {
    pub const ALL: [VoiceEngineChoice; 4] =
        [VoiceEngineChoice::WeType, VoiceEngineChoice::DoubaoIme, VoiceEngineChoice::Doubao, VoiceEngineChoice::System];

    pub fn name(self) -> &'static str {
        match self {
            VoiceEngineChoice::WeType => "微信输入法",
            VoiceEngineChoice::DoubaoIme => "豆包输入法",
            VoiceEngineChoice::Doubao => "豆包语音",
            VoiceEngineChoice::System => "系统语音",
        }
    }

    /// Stable id (the value in `settings.ini`, row keys `engine_<key>`).
    pub fn key(self) -> &'static str {
        match self {
            VoiceEngineChoice::WeType => "wetype",
            VoiceEngineChoice::DoubaoIme => "doubao_ime",
            VoiceEngineChoice::Doubao => "doubao",
            VoiceEngineChoice::System => "system",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            VoiceEngineChoice::WeType => "识别准、带标点，需要先安装微信输入法",
            VoiceEngineChoice::DoubaoIme => "字节跳动的豆包输入法；它目前不响应模拟按键，点墨还调不起它的语音",
            VoiceEngineChoice::Doubao => "第三方的豆包语音小工具，需要先配置好它",
            VoiceEngineChoice::System => "Windows 自带的语音输入（Win+H），无需安装",
        }
    }
}

/// Detection result for one voice engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStatus {
    /// Installed and usable.
    pub available: bool,
    /// One sentence from the host, e.g. "已安装 2.1.3.18", "未检测到微信输入法" or
    /// "点墨以管理员权限运行时通过辅助进程调用".
    pub detail: String,
    /// Extra line under the status (how it is used, caveats), e.g.
    /// "点墨以管理员权限运行时通过辅助进程调用". Empty = none.
    pub note: String,
    /// Where to get it when it's missing (shown as「去下载」).
    pub download_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceEngines {
    pub wetype: EngineStatus,
    pub doubao_ime: EngineStatus,
    pub doubao: EngineStatus,
    pub system: EngineStatus,
}

impl VoiceEngines {
    pub fn get(&self, e: VoiceEngineChoice) -> &EngineStatus {
        match e {
            VoiceEngineChoice::WeType => &self.wetype,
            VoiceEngineChoice::DoubaoIme => &self.doubao_ime,
            VoiceEngineChoice::Doubao => &self.doubao,
            VoiceEngineChoice::System => &self.system,
        }
    }

    /// The engine to suggest: the first available one in preference order.
    pub fn recommended(&self) -> VoiceEngineChoice {
        VoiceEngineChoice::ALL.into_iter().find(|&e| self.get(e).available).unwrap_or(VoiceEngineChoice::System)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateState {
    /// Not checked in this session.
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available {
        version: String,
        /// Release notes (plain text; lines separated by `\n`).
        notes: String,
    },
    /// Download progress 0.0..=1.0.
    Downloading(f32),
    Failed(String),
}

/// Everything the settings window, the about page and onboarding display.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsModel {
    // ---- 常规
    pub autostart: bool,
    /// Pop up when an edit field is tapped.
    pub auto_show: bool,
    /// 键盘 / 语音球 / 电脑键盘.
    pub input_mode: InputMode,
    pub theme: ThemeChoice,
    /// Highest-privilege scheduled task (works in admin windows).
    pub admin_task: Status,
    /// Reserve screen space under the docked keyboard (AppBar).
    pub reserve_space: bool,

    // ---- 键盘
    pub layout: LayoutChoice,
    /// 0.7..=1.5
    pub keyboard_height: f32,
    /// Landscape: edit area (撤销、复制、粘贴…) on the right.
    pub edit_area: bool,
    /// Bubble above pressed keys.
    pub key_popup: bool,
    /// Key click sound.
    pub key_sound: bool,
    pub key_sound_volume: KeySoundVolume,
    pub key_sound_style: KeySoundStyle,
    pub long_press: LongPress,
    pub ball: bool,
    pub ball_side: Side,

    // ---- 输入
    pub candidate_size: CandidateSize,
    pub full_width_punct: bool,
    pub space_commits_first: bool,
    /// Indexed by [`FuzzyPair::index`].
    pub fuzzy: [bool; 7],
    /// Whether fuzzy pinyin can be applied (librime is running; false: the switches are stored
    /// and a tag says they take effect later).
    pub fuzzy_supported: bool,
    /// Applying 模糊音 (redeploying the schemes, ~20 s) or the last failure; empty = nothing to
    /// say.
    pub fuzzy_status: Status,
    pub shuangpin: ShuangpinScheme,
    /// Dictionary (rime) state, e.g. ok "雾凇拼音词库已加载".
    pub dictionary: Status,
    /// Number of words the user taught (None = unknown).
    pub user_words: Option<u32>,
    /// Import / export / clear of the user dictionary in progress or failed; empty = nothing to
    /// say. The buttons are disabled while one is running (level `Unknown`).
    pub user_dict_status: Status,

    // ---- 语音
    pub voice_engine: VoiceEngineChoice,
    pub engines: VoiceEngines,
    /// DouBaoVoice exe; empty = found automatically.
    pub doubao_exe: String,

    // ---- 剪贴板
    pub clip_history: bool,
    pub clip_limit: u32,
    pub clip_skip_passwords: bool,
    /// Entries currently stored (unpinned + pinned).
    pub clip_count: usize,
    pub pinned_clips: Vec<ClipItem>,

    // ---- 关于
    pub version: String,
    pub build_date: String,
    pub update: UpdateState,
    pub auto_update: bool,

    /// Row keys whose setting is saved but takes effect only in a later version: they get a
    /// 「下个版本生效」 tag (e.g. `candidate_size`).
    pub coming_soon: Vec<String>,
}

impl Default for SettingsModel {
    fn default() -> Self {
        Self {
            autostart: true,
            auto_show: true,
            input_mode: InputMode::Keyboard,
            theme: ThemeChoice::System,
            admin_task: Status::default(),
            reserve_space: true,
            layout: LayoutChoice::Pinyin,
            keyboard_height: 1.0,
            edit_area: true,
            key_popup: true,
            key_sound: false,
            key_sound_volume: KeySoundVolume::Medium,
            key_sound_style: KeySoundStyle::Crisp,
            long_press: LongPress::Medium,
            ball: true,
            ball_side: Side::Right,
            candidate_size: CandidateSize::Standard,
            full_width_punct: true,
            space_commits_first: true,
            fuzzy: [false; 7],
            fuzzy_supported: false,
            fuzzy_status: Status::default(),
            shuangpin: ShuangpinScheme::Xiaohe,
            dictionary: Status::default(),
            user_words: None,
            user_dict_status: Status::default(),
            voice_engine: VoiceEngineChoice::WeType,
            engines: VoiceEngines::default(),
            doubao_exe: String::new(),
            clip_history: true,
            clip_limit: 50,
            clip_skip_passwords: true,
            clip_count: 0,
            pinned_clips: Vec::new(),
            version: String::new(),
            build_date: String::new(),
            update: UpdateState::Unknown,
            auto_update: true,
            coming_soon: Vec::new(),
        }
    }
}

/// Choices offered for [`SettingsModel::clip_limit`].
pub const CLIP_LIMITS: [u32; 4] = [20, 50, 100, 200];

pub const HOMEPAGE: &str = "https://github.com/yzfly/dianmo";
pub const LICENSE_URL: &str = "https://github.com/yzfly/dianmo/blob/main/LICENSE";
pub const RELEASES_URL: &str = "https://github.com/yzfly/dianmo/releases";

/// A change made in the settings UI, or a command for the host.
///
/// `Set*` changes are applied to the view's own copy of the model right away (so the control
/// responds instantly); the host saves them, applies them and may send back a corrected model
/// with [`super::SettingsView::set_model`].
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    // 常规
    SetAutostart(bool),
    SetAutoShow(bool),
    SetInputMode(InputMode),
    SetTheme(ThemeChoice),
    SetReserveSpace(bool),
    /// Register the highest-privilege scheduled task again.
    FixAdminTask,
    /// Restore all settings to defaults (after inline confirmation).
    ResetDefaults,
    // 键盘
    SetLayout(LayoutChoice),
    SetKeyboardHeight(f32),
    SetEditArea(bool),
    SetKeyPopup(bool),
    SetKeySound(bool),
    SetKeySoundVolume(KeySoundVolume),
    SetKeySoundStyle(KeySoundStyle),
    SetLongPress(LongPress),
    SetBall(bool),
    SetBallSide(Side),
    // 输入
    SetCandidateSize(CandidateSize),
    SetFullWidthPunct(bool),
    SetSpaceCommitsFirst(bool),
    SetFuzzy(FuzzyPair, bool),
    SetShuangpin(ShuangpinScheme),
    ExportUserDict,
    ImportUserDict,
    /// After inline confirmation.
    ClearUserDict,
    // 语音
    SetVoiceEngine(VoiceEngineChoice),
    /// Show a file picker for DouBaoVoice's exe.
    PickDoubaoExe,
    /// Forget the chosen exe (find it automatically).
    ResetDoubaoExe,
    // 剪贴板
    SetClipHistory(bool),
    SetClipLimit(u32),
    SetClipSkipPasswords(bool),
    /// Remove every unpinned entry (after inline confirmation).
    ClearClipboardHistory,
    UnpinClip(u64),
    // 关于
    SetAutoUpdate(bool),
    CheckUpdate,
    /// Download and install the available update.
    InstallUpdate,
    /// Open GitHub's new-issue page prefilled with version, OS, screen and scaling.
    ReportIssue,
    /// Logs + settings as a zip (no clipboard or dictionary content).
    ExportDiagnostics,
    OpenLogDir,
    OpenUrl(String),
    /// Show the first-run onboarding again.
    ShowOnboarding,
    /// Onboarding finished or skipped (don't show it again).
    FinishOnboarding,
    /// Close the window (Esc).
    Close,
    /// The user switched to this page (navigation, top tabs or Tab). Not sent for
    /// [`super::SettingsView::set_page`]: the host knows the pages it asks for. The host uses it
    /// for 关于: the tray icon's 「新版本」 red dot goes once the about page has been seen.
    PageShown(Page),
}

impl SettingsModel {
    /// Applies a `Set*` action to the model. Returns whether anything changed; commands return
    /// false.
    pub fn apply(&mut self, a: &SettingsAction) -> bool {
        use SettingsAction as A;
        fn set<T: PartialEq>(slot: &mut T, v: T) -> bool {
            let changed = *slot != v;
            *slot = v;
            changed
        }
        match a.clone() {
            A::SetAutostart(v) => set(&mut self.autostart, v),
            A::SetAutoShow(v) => set(&mut self.auto_show, v),
            A::SetInputMode(v) => set(&mut self.input_mode, v),
            A::SetTheme(v) => set(&mut self.theme, v),
            A::SetReserveSpace(v) => set(&mut self.reserve_space, v),
            A::SetLayout(v) => set(&mut self.layout, v),
            A::SetKeyboardHeight(v) => set(&mut self.keyboard_height, v.clamp(0.7, 1.5)),
            A::SetEditArea(v) => set(&mut self.edit_area, v),
            A::SetKeyPopup(v) => set(&mut self.key_popup, v),
            A::SetKeySound(v) => set(&mut self.key_sound, v),
            A::SetKeySoundVolume(v) => set(&mut self.key_sound_volume, v),
            A::SetKeySoundStyle(v) => set(&mut self.key_sound_style, v),
            A::SetLongPress(v) => set(&mut self.long_press, v),
            A::SetBall(v) => set(&mut self.ball, v),
            A::SetBallSide(v) => set(&mut self.ball_side, v),
            A::SetCandidateSize(v) => set(&mut self.candidate_size, v),
            A::SetFullWidthPunct(v) => set(&mut self.full_width_punct, v),
            A::SetSpaceCommitsFirst(v) => set(&mut self.space_commits_first, v),
            A::SetFuzzy(p, v) => set(&mut self.fuzzy[p.index()], v),
            A::SetShuangpin(v) => set(&mut self.shuangpin, v),
            A::SetVoiceEngine(v) => set(&mut self.voice_engine, v),
            A::ResetDoubaoExe => set(&mut self.doubao_exe, String::new()),
            A::SetClipHistory(v) => set(&mut self.clip_history, v),
            A::SetClipLimit(v) => set(&mut self.clip_limit, v),
            A::SetClipSkipPasswords(v) => set(&mut self.clip_skip_passwords, v),
            A::UnpinClip(id) => {
                let n = self.pinned_clips.len();
                self.pinned_clips.retain(|c| c.id != id);
                n != self.pinned_clips.len()
            }
            A::SetAutoUpdate(v) => set(&mut self.auto_update, v),
            _ => false,
        }
    }
}
