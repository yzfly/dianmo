//! Voice input through a third-party engine (TODO #27 / #29, `docs/research/voice.md`).
//!
//! 点墨 has no speech recognizer of its own. The microphone key and the voice ball press the voice
//! hotkey of an engine the user already has, and watch whether it really started listening:
//!
//! | engine | start / stop (same chord, one `SendInput`) | listening = | result |
//! |---|---|---|---|
//! | [`VoiceEngine::WeType`] 微信输入法 | `LCtrl↓ LWin↓ LShift↓ LShift↑ LWin↑ LCtrl↑` | mic in use by `…#WeType#<ver>#wetype_update.exe` | typed by WeType when it is the target window's IME; otherwise WeType copies it to the clipboard and **点墨 types it** (clipboard fallback below) |
//! | [`VoiceEngine::DoubaoIme`] 豆包输入法 (ByteDance, official) | `voice.voiceShortcutMode` from `%APPDATA%\DoubaoIme\conf\config.json`: `right_alt_space` (default) = `RAlt↓ Space↓ Space↑ RAlt↑` | mic in use by `…#DoubaoIME#…#ImeService.exe` (or the target app) | typed by 豆包输入法; clipboard fallback like WeType (unverified, see below) |
//! | [`VoiceEngine::DoubaoVoice`] 豆包语音小工具 (third-party, not the 豆包输入法) | `HotKey` from `%APPDATA%\DouBaoVoice\config.json` (F6) | mic in use by `…#DouBaoVoice*.exe` | typed by the tool itself |
//! | [`VoiceEngine::System`] | `LWin↓ H↓ H↑ LWin↑` (Win+H) | not tracked | typed by Windows |
//!
//! "Mic in use" is read from `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\
//! ConsentStore\microphone\NonPackaged\<exe path with \ as #>`: `LastUsedTimeStop == 0` while the
//! app holds the microphone (Win10 1903+).
//!
//! If the selected engine is not available, or it does not start listening within 1.5 s after
//! the hotkey (3 s while its voice window is already visible), [`Voice`] reports
//! [`VoiceState::Failed`] with the reason. With [`Voice::set_fallback`]`(true)` (the default; 点墨
//! turns it off since 2026-10-06: the user doesn't want Windows voice typing) it also starts
//! Win+H, and after two failures in a row skips the engine (straight to Win+H) until
//! [`Voice::set_engine`].
//!
//! **Elevated windows.** The engines run un-elevated. While an elevated (administrator) window is
//! in the foreground, Windows (UIPI) keeps the injected hotkey from them — whoever injects it:
//! tested 2026-10-06 on the Surface with WeType 2.1.3.18 (`wetype_update.exe` at medium
//! integrity): elevated or un-elevated `SendInput` with a normal Notepad in front → listening
//! after ~0.3 s; with an administrator Notepad in front → nothing from either. So [`Voice::start`]
//! checks the foreground window first and fails at once ("管理员窗口…") instead of waiting 1.5 s,
//! and an un-elevated helper process would not help (not built).
//!
//! **Clipboard fallback (WeType).** [`Voice::start`] records `GetClipboardSequenceNumber` and a
//! copy of the clipboard (all memory-based formats, up to 32 MB; GDI-handle formats such as
//! `CF_BITMAP` / `CF_ENHMETAFILE` are not copied, but Windows synthesizes the DIB variants, which
//! are). After stopping, once the microphone is released, it waits up to 2.5 s: if the sequence
//! number changed and the clipboard is WeType's (owner process `wetype*`, or no owner window while
//! WeType's voice/tip window is visible: WeType copies without an owner), it reads the text, types it
//! with [`dianmo_win::send_text`] and puts the saved clipboard back (marked
//! `CanIncludeInClipboardHistory=0`, so Win+V history doesn't get a duplicate). WeType's own
//! clipboard-history entry stays (Windows offers no way to remove a single entry). If the
//! clipboard was changed by another process (the user copied something meanwhile) nothing is
//! typed and the clipboard is left alone.
//!
//! **Threading / CPU.** One [`Voice`] lives on the UI thread. Nothing runs while idle: the host
//! calls [`Voice::poll`] from a timer (about every 300 ms) only while [`Voice::needs_poll`].
//!
//! **Integration notes (app.rs).**
//! - Before `start`/`stop`, release 点墨's own latched modifiers (#22 Ctrl/Shift lock) and commit or
//!   clear the pending composition, or the chord changes meaning / the IME eats it.
//! - Esc ([`Voice::cancel`]) is only sent while WeType is confirmed listening; it also reaches the
//!   target app if WeType doesn't swallow it.
//! - **Conflict: 电脑键 `Ctrl+Win+←/→`** (switch virtual desktop). WeType's push-to-talk is
//!   "hold Ctrl+Win", so pressing Ctrl and Win before the arrow may start WeType voice. When the
//!   layout sends such chords, press **Win first, then Ctrl** (`LWin↓ LCtrl↓ ←↓ ←↑ LCtrl↑ LWin↑`),
//!   or keep the gap between modifiers and key at zero (one `SendInput`, as `send_chord` does).
//!   Not verified on the device yet.
//! - **Elevated target windows.** WeType's hotkey is caught by WeType's own processes, which run
//!   un-elevated when started normally (by the first app that activates the IME). While an
//!   elevated window is in the foreground they don't see the chord (UIPI), WeType doesn't start
//!   and [`Voice`] falls back to Win+H after 1.5 s (tested 2026-10-06; during the research the
//!   WeType processes happened to run elevated, so it worked there).
//! - Push-to-talk (`LCtrl↓ LWin↓` … `vkE8↓↑ LWin↑ LCtrl↑`) is not offered here on purpose: holding
//!   real modifiers down across touches is fragile; the ball uses tap-to-toggle.
//! - DoubaoVoice: when not running it can be launched (via `explorer.exe`, so it runs un-elevated
//!   even from the elevated 点墨). 点墨 only reads `HotKey` and `IsConfigured` from its config,
//!   never the token. It types the result itself, about 1–3 s after stopping ("优化中…"). It pastes its result with the clipboard + Ctrl+V and
//!   leaves the clipboard replaced (tested 2026-10-06), so [`Voice`] saves the clipboard at
//!   `start` too and puts it back 1 s after DouBaoVoice's change. Its Ctrl+V can't reach elevated
//!   windows (it runs un-elevated).
#![allow(dead_code)] // reports, fingerprints and some setters are only used by examples/voice_probe.rs

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_VALUE_TYPE, RegCloseKey,
    RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    HWND_MESSAGE, IsWindowVisible, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{BOOL, PCWSTR, PWSTR, w};

/// Which product does the speech recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceEngine {
    /// 微信输入法 (WeType), default.
    WeType,
    /// 豆包输入法 Windows 版 (ByteDance's IME, `C:\Program Files\DoubaoIME`).
    DoubaoIme,
    /// 豆包语音输入 Windows 版 (third-party `DouBaoVoice*.exe`, Volcengine streaming ASR).
    DoubaoVoice,
    /// Windows voice typing (Win+H).
    System,
}

impl VoiceEngine {
    pub const ALL: [VoiceEngine; 4] =
        [VoiceEngine::WeType, VoiceEngine::DoubaoIme, VoiceEngine::DoubaoVoice, VoiceEngine::System];

    /// Value in `settings.ini` (`voice_engine=wetype|doubao|system`).
    pub fn as_setting(self) -> &'static str {
        match self {
            VoiceEngine::WeType => "wetype",
            VoiceEngine::DoubaoIme => "doubao_ime",
            VoiceEngine::DoubaoVoice => "doubao",
            VoiceEngine::System => "system",
        }
    }

    pub fn from_setting(s: &str) -> Option<VoiceEngine> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wetype" | "weixin" | "wechat" => Some(VoiceEngine::WeType),
            "doubao_ime" | "doubaoime" => Some(VoiceEngine::DoubaoIme),
            "doubao" | "doubao_voice" | "doubaovoice" => Some(VoiceEngine::DoubaoVoice),
            "system" | "win+h" | "windows" => Some(VoiceEngine::System),
            _ => None,
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            VoiceEngine::WeType => "微信输入法",
            VoiceEngine::DoubaoIme => "豆包输入法",
            VoiceEngine::DoubaoVoice => "豆包语音（第三方）",
            VoiceEngine::System => "系统语音（Win+H）",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What the voice ball shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceState {
    /// Not recording. Also the state after Win+H (Windows' own panel is not tracked).
    Idle,
    /// Hotkey sent (or DoubaoVoice being launched), waiting for the microphone.
    Starting,
    /// The engine holds the microphone.
    Listening,
    /// Stop sent; waiting for the microphone to be released and the result to arrive.
    Finishing,
    /// The selected engine could not be used; Win+H was started instead (unless the message says
    /// otherwise). Stays until the next `start`/`toggle`/`cancel`; behaves like `Idle`.
    Failed(String),
}

impl VoiceState {
    /// Recording or about to (the ball animates).
    pub fn is_active(&self) -> bool {
        matches!(self, VoiceState::Starting | VoiceState::Listening | VoiceState::Finishing)
    }
}

/// What happened in the last session (for logs and the probe; never contains recognized text).
#[derive(Debug, Clone, Default)]
pub struct VoiceReport {
    pub engine: Option<VoiceEngine>,
    /// Win+H was used instead of the selected engine.
    pub fell_back: bool,
    /// Milliseconds from the hotkey until the microphone was in use.
    pub listen_after_ms: Option<u64>,
    /// Exe name of the process that changed the clipboard during the session.
    pub clipboard_owner: Option<String>,
    /// Characters typed by the clipboard fallback.
    pub typed_chars: usize,
    /// `Some(true)` restored, `Some(false)` restore failed, `None` the clipboard was not touched.
    pub clipboard_restored: Option<bool>,
    /// Formats put back / formats that could not be saved (GDI handles, over the size cap).
    pub restored_formats: usize,
    pub skipped_formats: usize,
}

const START_TIMEOUT: Duration = Duration::from_millis(1500);
/// Start timeout while the engine's voice window is already up (slow microphone start).
const START_TIMEOUT_WINDOW: Duration = Duration::from_millis(3000);
/// After stop: how long the microphone may stay in use before we assume the stop was missed.
const STOP_MIC_TIMEOUT: Duration = Duration::from_millis(4000);
/// After the microphone is released: how long to wait for WeType's clipboard copy.
const CLIPBOARD_WAIT: Duration = Duration::from_millis(2500);
/// After DoubaoVoice released the microphone it shows "优化中…" (LLM clean-up), then pastes the
/// result through the clipboard: about 1–3 s.
const DOUBAO_RESULT_WAIT: Duration = Duration::from_secs(8);
/// After DoubaoVoice's clipboard change, before the user's clipboard is put back.
const DOUBAO_PASTE_GRACE: Duration = Duration::from_millis(1000);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
/// After a launched engine's window appears, before its hotkey is registered.
const LAUNCH_SETTLE: Duration = Duration::from_millis(700);
const CLIPBOARD_CAP: usize = 32 << 20;

const WETYPE_TIP: &str = "{86598FB9-66A2-463E-B9C2-AEB906D477AD}";
const WETYPE_MIC_EXE: &str = "wetype_update.exe";
const WETYPE_VOICE_CLASS: &str = "wetype.flutter.setting";
const WETYPE_VOICE_TITLE: &str = "语音输入";
const DOUBAO_PREFIX: &str = "doubaovoice";
/// 豆包输入法's TIP (text service) CLSID and install directory (as it appears, lower case, in
/// the microphone consent store's `#`-separated paths).
const DOUBAO_IME_TIP: &str = "{9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}";
const DOUBAO_IME_DIR: &str = "#doubaoime#";
const DOUBAO_IME_SERVICE: &str = "imeservice.exe";

const VK_LSHIFT: u16 = 0xA0;
const VK_LCONTROL: u16 = 0xA2;
const VK_LMENU: u16 = 0xA4;
const VK_LWIN: u16 = 0x5B;
const VK_RMENU: u16 = 0xA5;
const VK_SPACE: u16 = 0x20;
const VK_ESCAPE: u16 = 0x1B;
const VK_H: u16 = b'H' as u16;
const VK_F6: u16 = 0x75;

#[derive(Debug)]
enum Phase {
    Idle,
    /// The engine's process was launched; the hotkey goes out once its window is up.
    Launching { since: Instant, window_at: Option<Instant> },
    Starting { sent: Instant, filetime: u64 },
    Listening,
    /// `pasted`: when DouBaoVoice put its result on the clipboard (it pastes with Ctrl+V and
    /// leaves the clipboard replaced).
    Finishing { stop_sent: Instant, mic_released: Option<Instant>, pasted: Option<Instant> },
    /// Cancelled (reported as `Idle`), but WeType may still copy a result to the clipboard
    /// (it does after Esc when the target IME isn't WeType): watch it and put the user's back.
    Discarding { since: Instant, mic_released: Option<Instant> },
    Failed(String),
}

/// The voice controller. Create one, call [`toggle`](Voice::toggle) on the mic key / voice ball,
/// and [`poll`](Voice::poll) from a timer while [`needs_poll`](Voice::needs_poll).
pub struct Voice {
    engine: VoiceEngine,
    phase: Phase,
    /// Consecutive start failures per engine; with `fallback`, at 2 the engine is skipped until
    /// `set_engine`.
    failures: [u8; 4],
    /// The foreground process's exe (lower case) when the session started: 豆包输入法 may capture
    /// the microphone inside the target app.
    target_exe: Option<String>,
    /// FILETIME when the start hotkey went out.
    session_filetime: u64,
    doubao_exe: Option<PathBuf>,
    doubao_launch: bool,
    /// Fall back to Win+H when the engine can't be used (default true).
    fallback: bool,
    /// Clipboard sequence number and contents when the WeType session started.
    clip_seq: u32,
    clip_saved: Option<ClipSnapshot>,
    report: VoiceReport,
    log: Option<fn(&str)>,
}

impl Voice {
    pub fn new(engine: VoiceEngine) -> Self {
        Voice {
            engine,
            phase: Phase::Idle,
            failures: [0; 4],
            target_exe: None,
            session_filetime: 0,
            doubao_exe: None,
            doubao_launch: true,
            fallback: true,
            clip_seq: 0,
            clip_saved: None,
            report: VoiceReport::default(),
            log: None,
        }
    }

    pub fn engine(&self) -> VoiceEngine {
        self.engine
    }

    /// Selects the engine (also re-enables one that failed twice). Cancels a running session.
    pub fn set_engine(&mut self, e: VoiceEngine) {
        if self.phase_active() {
            self.cancel();
        }
        self.engine = e;
        self.failures = [0; 4];
        if matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Idle;
        }
    }

    /// `voice_doubao_exe` from the settings (None = look in the known places).
    pub fn set_doubao_exe(&mut self, exe: Option<PathBuf>) {
        self.doubao_exe = exe.filter(|p| !p.as_os_str().is_empty());
    }

    /// Whether to launch DouBaoVoice when it is not running (default true).
    pub fn set_doubao_launch(&mut self, launch: bool) {
        self.doubao_launch = launch;
    }

    /// Whether to start Win+H when the selected engine can't be used (default true).
    pub fn set_fallback(&mut self, fallback: bool) {
        self.fallback = fallback;
    }

    /// Receives one-line diagnostics (never recognized text or clipboard contents).
    pub fn set_log(&mut self, log: fn(&str)) {
        self.log = Some(log);
    }

    pub fn last_report(&self) -> &VoiceReport {
        &self.report
    }

    /// Installed and usable right now (cheap: a few registry reads and one process snapshot).
    /// WeType: TIP registered and enabled in the user's input methods, `wetype_server.exe` and
    /// `wetype_update.exe` running.
    /// DoubaoVoice: `config.json` has `IsConfigured=true` and the tool is running or its exe found
    /// (in `%USERPROFILE%\Downloads`, `%LOCALAPPDATA%\Programs\DouBaoVoice`, …).
    /// System: always.
    pub fn available(e: VoiceEngine) -> bool {
        unavailable_reason(e, None, true).is_none()
    }

    /// Why `e` can't be used right now (None = usable), with the default DoubaoVoice lookup.
    /// Safe to call from any thread.
    pub fn why_unavailable(e: VoiceEngine) -> Option<String> {
        unavailable_reason(e, None, true)
    }

    /// Like [`available`](Voice::available) for the selected engine with this instance's
    /// DoubaoVoice settings; `Some(reason)` when it is not.
    pub fn unavailable_reason(&self) -> Option<String> {
        unavailable_reason(self.engine, self.doubao_exe.as_deref(), self.doubao_launch)
    }

    pub fn state(&self) -> VoiceState {
        match &self.phase {
            Phase::Idle | Phase::Discarding { .. } => VoiceState::Idle,
            Phase::Launching { .. } | Phase::Starting { .. } => VoiceState::Starting,
            Phase::Listening => VoiceState::Listening,
            Phase::Finishing { .. } => VoiceState::Finishing,
            Phase::Failed(m) => VoiceState::Failed(m.clone()),
        }
    }

    /// The host should call [`poll`](Voice::poll) about every 300 ms while this is true. It can
    /// stay true for up to ~2.5 s after [`cancel`](Voice::cancel) while `state()` is already `Idle`
    /// (watching for a late WeType clipboard copy to undo).
    pub fn needs_poll(&self) -> bool {
        self.phase_active()
    }

    fn phase_active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Launching { .. }
                | Phase::Starting { .. }
                | Phase::Listening
                | Phase::Finishing { .. }
                | Phase::Discarding { .. }
        )
    }

    /// One tap on the mic key / voice ball: start when idle, stop when starting or listening.
    /// While finishing, a tap starts a new session.
    pub fn toggle(&mut self) -> VoiceState {
        match self.phase {
            Phase::Launching { .. } | Phase::Starting { .. } | Phase::Listening => self.stop(),
            _ => self.start(),
        }
    }

    pub fn start(&mut self) -> VoiceState {
        if matches!(self.phase, Phase::Launching { .. } | Phase::Starting { .. } | Phase::Listening) {
            return self.state();
        }
        if matches!(self.phase, Phase::Finishing { .. } | Phase::Discarding { .. }) {
            // Still waiting for the previous result; give up on it (keep the user's clipboard).
            self.finish_clipboard(false);
        }
        self.report = VoiceReport { engine: Some(self.engine), ..VoiceReport::default() };
        self.clip_saved = None;
        let engine = self.engine;
        if engine == VoiceEngine::System {
            return self.system_voice();
        }
        if self.fallback && self.failures[engine.index()] >= 2 {
            return self.fail(format!("{}多次没有响应", engine.label()));
        }
        if let Some(why) = unavailable_reason(engine, self.doubao_exe.as_deref(), self.doubao_launch) {
            return self.fail(why);
        }
        if let Some(why) = blocked_by_elevated_foreground(engine) {
            // The hotkey can't reach the engine (UIPI); not the engine's fault: no failure count.
            return self.fail(why);
        }
        self.target_exe = foreground_pid().and_then(process_path).and_then(|p| {
            p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase())
        });
        // Both engines may replace the clipboard with the result: save it to put it back.
        self.clip_seq = unsafe { GetClipboardSequenceNumber() };
        let (snap, ok) = ClipSnapshot::take();
        self.report.skipped_formats = snap.as_ref().map_or(0, |s| s.skipped);
        if !ok {
            self.logf(format_args!("voice: clipboard could not be saved (busy); no restore this time"));
        }
        self.clip_saved = snap;
        match engine {
            VoiceEngine::WeType => {
                if find_process(|n| n == WETYPE_MIC_EXE).is_none() {
                    // Its voice process died (it can crash); start it again, hotkey once it's up.
                    let Some(exe) = wetype_update_exe() else {
                        return self.fail("微信输入法的语音组件没有在运行".into());
                    };
                    return self.launch(&exe);
                }
                self.send_hotkey(engine);
            }
            VoiceEngine::DoubaoIme => self.send_hotkey(engine),
            VoiceEngine::DoubaoVoice => {
                if find_process(|n| n.starts_with(DOUBAO_PREFIX)).is_none() {
                    let Some(exe) = doubao_exe(self.doubao_exe.as_deref()) else {
                        return self.fail("找不到豆包语音程序".into());
                    };
                    return self.launch(&exe);
                }
                self.send_hotkey(engine);
            }
            VoiceEngine::System => unreachable!(),
        }
        self.state()
    }

    /// Starts the engine's process (WeType's voice process, DouBaoVoice) through `explorer.exe`,
    /// so it gets the shell's un-elevated token even though 点墨 runs elevated. The hotkey goes
    /// out from `poll` once its window exists.
    fn launch(&mut self, exe: &Path) -> VoiceState {
        if std::process::Command::new("explorer.exe").arg(exe).spawn().is_err() {
            return self.fail(format!("{}启动失败", self.engine.label()));
        }
        self.logf(format_args!("voice: launching {}", exe.file_name().unwrap_or_default().to_string_lossy()));
        self.phase = Phase::Launching { since: Instant::now(), window_at: None };
        self.state()
    }

    /// Sends the engine's toggle hotkey and enters `Starting`.
    fn send_hotkey(&mut self, engine: VoiceEngine) {
        let filetime = filetime_now();
        self.session_filetime = filetime;
        let ok = send_keys(&toggle_chord(engine));
        self.logf(format_args!("voice: {} start hotkey sent (ok={ok})", engine.as_setting()));
        self.phase = Phase::Starting { sent: Instant::now(), filetime };
    }

    /// Second tap: stop listening; the result arrives while `Finishing`.
    pub fn stop(&mut self) -> VoiceState {
        let engine = self.engine;
        match self.phase {
            Phase::Launching { .. } => {
                self.phase = Phase::Idle;
                self.clip_saved = None;
            }
            Phase::Starting { .. } => {
                // Only send the toggle again if the engine actually reacted, or it would *start*.
                let reacted = mic_in_use(engine, 0, self.target()) || (engine == VoiceEngine::WeType && wetype_voice_window());
                if reacted {
                    send_keys(&toggle_chord(engine));
                    self.phase = Phase::Finishing { stop_sent: Instant::now(), mic_released: None, pasted: None };
                } else {
                    self.phase = Phase::Idle;
                    self.clip_saved = None;
                }
            }
            Phase::Listening => {
                send_keys(&toggle_chord(engine));
                self.logf(format_args!("voice: {} stop hotkey sent", engine.as_setting()));
                self.phase = Phase::Finishing { stop_sent: Instant::now(), mic_released: None, pasted: None };
            }
            _ => {}
        }
        self.state()
    }

    /// Abort without a result. WeType: Esc (only while it is confirmed listening); the result is
    /// dropped, and if WeType copies it to the clipboard anyway (it does after Esc when the target
    /// IME isn't WeType) the user's clipboard is put back during the next ~2.5 s of `poll`s.
    /// DouBaoVoice has no cancel: this stops it like [`stop`](Voice::stop) (it pastes what it
    /// heard; the clipboard is still restored afterwards).
    pub fn cancel(&mut self) {
        let now = Instant::now();
        match (self.engine, &self.phase) {
            (VoiceEngine::DoubaoVoice, Phase::Starting { .. } | Phase::Listening) => {
                self.stop();
                return;
            }
            (VoiceEngine::DoubaoVoice, Phase::Finishing { .. }) | (_, Phase::Discarding { .. }) => return,
            (VoiceEngine::WeType | VoiceEngine::DoubaoIme, Phase::Starting { .. } | Phase::Listening) => {
                if mic_in_use(self.engine, 0, self.target()) {
                    send_keys(&[(VK_ESCAPE, false), (VK_ESCAPE, true)]);
                }
            }
            (_, Phase::Finishing { mic_released, .. }) => {
                let mic_released = *mic_released;
                if self.clipboard_changed() {
                    self.finish_clipboard(false);
                } else if self.clip_saved.is_some() {
                    self.phase = Phase::Discarding { since: now, mic_released };
                    return;
                }
            }
            _ => {}
        }
        if matches!(self.engine, VoiceEngine::WeType | VoiceEngine::DoubaoIme) && self.clip_saved.is_some() {
            self.phase = Phase::Discarding { since: now, mic_released: None };
        } else {
            self.clip_saved = None;
            self.phase = Phase::Idle;
        }
    }

    /// Advances the state machine; call about every 300 ms while [`needs_poll`](Voice::needs_poll).
    pub fn poll(&mut self) -> VoiceState {
        let engine = self.engine;
        let now = Instant::now();
        match self.phase {
            Phase::Launching { since, window_at } => {
                let up = match engine {
                    // Its (hidden) voice window exists once it is ready.
                    VoiceEngine::WeType => find_process(|n| n == WETYPE_MIC_EXE)
                        .is_some_and(|pid| process_has_window(pid, |class, _| class == WETYPE_VOICE_CLASS)),
                    _ => find_process(|n| n.starts_with(DOUBAO_PREFIX))
                        .is_some_and(|pid| process_has_window(pid, |_, visible| visible)),
                };
                match (up, window_at) {
                    (true, None) => self.phase = Phase::Launching { since, window_at: Some(now) },
                    (true, Some(t)) if now - t >= LAUNCH_SETTLE => self.send_hotkey(engine),
                    _ if now - since > LAUNCH_TIMEOUT => {
                        return self.fail(format!("{}启动超时", engine.label()));
                    }
                    _ => {}
                }
            }
            Phase::Starting { sent, filetime } => {
                if mic_in_use(engine, filetime, self.target()) {
                    self.failures[engine.index()] = 0;
                    self.report.listen_after_ms = Some((now - sent).as_millis() as u64);
                    self.logf(format_args!("voice: {} listening after {} ms", engine.as_setting(), (now - sent).as_millis()));
                    self.phase = Phase::Listening;
                } else {
                    let window = engine == VoiceEngine::WeType && wetype_voice_window();
                    let limit = if window { START_TIMEOUT_WINDOW } else { START_TIMEOUT };
                    if now - sent > limit {
                        self.failures[engine.index()] += 1;
                        if window {
                            // Its window is up but it never got the microphone: close it.
                            send_keys(&[(VK_ESCAPE, false), (VK_ESCAPE, true)]);
                        }
                        self.clip_saved = None;
                        return self.fail(format!("{}没有开始收音", engine.label()));
                    }
                }
            }
            Phase::Listening => {
                if !mic_in_use(engine, 0, self.target()) {
                    // Stopped from the engine's own UI, or it ended on silence.
                    self.logf(format_args!("voice: {} stopped by itself", engine.as_setting()));
                    self.phase = Phase::Finishing { stop_sent: now, mic_released: Some(now), pasted: None };
                }
            }
            Phase::Finishing { stop_sent, mic_released, pasted } => {
                if matches!(engine, VoiceEngine::WeType | VoiceEngine::DoubaoIme)
                    && self.clipboard_changed()
                    && self.try_clipboard_result()
                {
                    self.phase = Phase::Idle;
                    return self.state();
                }
                if engine == VoiceEngine::DoubaoVoice && self.clip_saved.is_some() {
                    match pasted {
                        None if self.clipboard_changed() => {
                            let owner = clipboard_owner_exe();
                            self.report.clipboard_owner = Some(owner.clone().unwrap_or_else(|| "<no owner>".into()));
                            if clipboard_from_engine(engine, owner.as_deref()) {
                                self.phase = Phase::Finishing { stop_sent, mic_released, pasted: Some(now) };
                            } else {
                                self.clip_saved = None; // the user copied something: leave it
                            }
                            return self.state();
                        }
                        // Give the target time to process DouBaoVoice's Ctrl+V before the
                        // clipboard changes back.
                        Some(t) if now - t >= DOUBAO_PASTE_GRACE => {
                            self.finish_clipboard(true);
                            self.phase = Phase::Idle;
                            return self.state();
                        }
                        Some(_) => return self.state(),
                        None => {}
                    }
                }
                match mic_released {
                    None => {
                        if !mic_in_use(engine, 0, self.target()) {
                            self.phase = Phase::Finishing { stop_sent, mic_released: Some(now), pasted };
                        } else if now - stop_sent > STOP_MIC_TIMEOUT {
                            // The stop chord was not taken; the engine is still listening.
                            self.logf(format_args!("voice: {} still listening after stop", engine.as_setting()));
                            self.phase = Phase::Listening;
                        }
                    }
                    Some(t) => {
                        let wait = if engine == VoiceEngine::WeType { CLIPBOARD_WAIT } else { DOUBAO_RESULT_WAIT };
                        if now - t > wait {
                            self.finish_clipboard(false);
                            self.phase = Phase::Idle;
                        }
                    }
                }
            }
            Phase::Discarding { since, mic_released } => {
                if self.clipboard_changed() {
                    self.finish_clipboard(false); // restores it if it is WeType's copy
                    self.phase = Phase::Idle;
                } else {
                    let released = mic_released.or_else(|| (!mic_in_use(engine, 0, self.target())).then_some(now));
                    let done = released.is_some_and(|t| now - t > CLIPBOARD_WAIT)
                        || now - since > STOP_MIC_TIMEOUT + CLIPBOARD_WAIT;
                    self.phase = if done {
                        self.clip_saved = None;
                        Phase::Idle
                    } else {
                        Phase::Discarding { since, mic_released: released }
                    };
                }
            }
            Phase::Idle | Phase::Failed(_) => {}
        }
        self.state()
    }

    /// Win+H (the System engine).
    fn system_voice(&mut self) -> VoiceState {
        self.phase = if dianmo_win::start_voice_typing() {
            Phase::Idle
        } else {
            Phase::Failed("系统语音输入（Win+H）发送失败".into())
        };
        self.state()
    }

    /// The selected engine can't be used: fall back to Win+H (unless disabled) and report why.
    fn fail(&mut self, why: String) -> VoiceState {
        let msg = if self.fallback {
            self.report.fell_back = true;
            if dianmo_win::start_voice_typing() {
                format!("{why}，已改用系统语音输入")
            } else {
                format!("{why}（系统语音输入 Win+H 也发送失败）")
            }
        } else {
            why
        };
        self.logf(format_args!("voice: {msg}"));
        self.phase = Phase::Failed(msg);
        self.state()
    }

    fn clipboard_changed(&self) -> bool {
        let seq = unsafe { GetClipboardSequenceNumber() };
        seq != self.clip_seq
    }

    /// The clipboard changed during a WeType session: if WeType put the result there, type it
    /// and restore the user's clipboard. Returns true when the session is done.
    fn try_clipboard_result(&mut self) -> bool {
        let owner = clipboard_owner_exe();
        let from_wetype = clipboard_from_engine(self.engine, owner.as_deref());
        self.logf(format_args!(
            "voice: clipboard changed (owner={}, wetype window={}, from wetype={from_wetype})",
            owner.as_deref().unwrap_or("-"),
            wetype_voice_window()
        ));
        self.report.clipboard_owner = Some(owner.unwrap_or_else(|| "<no owner>".into()));
        if !from_wetype {
            // Someone else (the user) copied something: leave it, don't type it.
            self.logf(format_args!(
                "voice: clipboard changed by {}, not WeType; left alone",
                self.report.clipboard_owner.as_deref().unwrap_or("?")
            ));
            self.clip_saved = None;
            return true;
        }
        let Some(text) = read_clipboard_text() else {
            return false; // not rendered yet; try again on the next poll
        };
        if text.is_empty() {
            return false;
        }
        self.report.typed_chars = text.chars().count();
        let typed = dianmo_win::send_text(&text);
        drop(text);
        self.logf(format_args!("voice: typed {} chars from WeType's clipboard copy (ok={typed})", self.report.typed_chars));
        self.finish_clipboard(true);
        true
    }

    /// Ends the clipboard part of a session: restores the saved clipboard if WeType replaced it
    /// (or `force` after typing its result).
    fn finish_clipboard(&mut self, force: bool) {
        let Some(snap) = self.clip_saved.take() else {
            return;
        };
        let changed = self.clipboard_changed();
        if !(force || changed && clipboard_from_engine(self.engine, clipboard_owner_exe().as_deref())) {
            return;
        }
        let restored = snap.restore();
        self.report.clipboard_restored = Some(restored.is_some());
        self.report.restored_formats = restored.unwrap_or(0);
        self.logf(format_args!("voice: clipboard restored={} formats={}", restored.is_some(), self.report.restored_formats));
    }

    /// The target app for [`mic_in_use`] (豆包输入法 only).
    fn target(&self) -> Option<(&str, u64)> {
        if self.engine != VoiceEngine::DoubaoIme {
            return None;
        }
        self.target_exe.as_deref().map(|t| (t, self.session_filetime))
    }

    fn logf(&self, args: std::fmt::Arguments) {
        if let Some(f) = self.log {
            f(&args.to_string());
        }
    }
}

// ---------------------------------------------------------------- availability

fn unavailable_reason(e: VoiceEngine, doubao_exe_setting: Option<&Path>, may_launch: bool) -> Option<String> {
    match e {
        VoiceEngine::System => None,
        VoiceEngine::WeType => {
            let tip = format!(r"SOFTWARE\Microsoft\CTF\TIP\{WETYPE_TIP}");
            if !reg_key_exists(HKEY_LOCAL_MACHINE, &tip) {
                return Some("没有安装微信输入法".into());
            }
            if !tip_enabled(WETYPE_TIP) {
                return Some("微信输入法不在输入法列表里".into());
            }
            if find_process(|n| n == "wetype_server.exe").is_none() {
                return Some("微信输入法没有在运行".into());
            }
            // wetype_update.exe hosts the voice window, the microphone and the voice hotkey. It
            // can crash (seen 2026-10-06: access violation in GraphicsCapture.dll right after a
            // session) and WeType doesn't restart it, so the hotkey would do nothing; `start`
            // launches it again (no window shows; tested).
            if find_process(|n| n == WETYPE_MIC_EXE).is_none() && wetype_update_exe().is_none() {
                return Some("微信输入法的语音组件（wetype_update.exe）没有在运行".into());
            }
            None
        }
        VoiceEngine::DoubaoIme => {
            if !reg_key_exists(HKEY_LOCAL_MACHINE, &format!(r"SOFTWARE\Microsoft\CTF\TIP\{DOUBAO_IME_TIP}")) {
                return Some("没有安装豆包输入法".into());
            }
            if !tip_enabled(DOUBAO_IME_TIP) {
                return Some("豆包输入法不在输入法列表里".into());
            }
            if doubao_ime_service().is_none() {
                return Some("豆包输入法没有在运行".into());
            }
            // Measured on the Surface (2026-10-07, 豆包输入法 0.9.1.22): it ignores injected
            // keys. Neither its 「免按模式」 (right Alt+Space) nor its 「长按模式」 (hold right Alt)
            // reacted to SendInput, with 豆包输入法 as the target window's input method, with
            // `enableGlobalVoiceShortcut` on (its settings window has no switch for it) or off;
            // the system menu opened instead. So it can't be driven until it accepts them.
            let cfg = doubao_ime_config().unwrap_or_default();
            if !cfg.shortcut {
                return Some("豆包输入法关闭了语音快捷键（豆包输入法设置 → 语音输入）".into());
            }
            Some("豆包输入法不响应模拟按键，点墨暂时调不起它的语音；请先用微信输入法".into())
        }
        VoiceEngine::DoubaoVoice => {
            let Some(cfg) = doubao_config() else {
                return Some("豆包语音没有配置".into());
            };
            if !cfg.configured {
                return Some("豆包语音没有配置".into());
            }
            if find_process(|n| n.starts_with(DOUBAO_PREFIX)).is_some() {
                return None;
            }
            if !may_launch {
                return Some("豆包语音没有在运行".into());
            }
            if doubao_exe(doubao_exe_setting).is_none() {
                return Some("找不到豆包语音程序".into());
            }
            None
        }
    }
}

/// An input method's profile is in one of `HKCU\Control Panel\International\User Profile\<lang>`
/// as a value named `0804:{CLSID}{PROFILE}`.
fn tip_enabled(tip: &str) -> bool {
    let root = r"Control Panel\International\User Profile";
    reg_subkeys(HKEY_CURRENT_USER, root).iter().any(|lang| {
        reg_value_names(HKEY_CURRENT_USER, &format!(r"{root}\{lang}"))
            .iter()
            .any(|v| v.to_ascii_uppercase().contains(tip))
    })
}

/// 豆包输入法's `ImeService.exe` (the name is generic: check its directory).
fn doubao_ime_service() -> Option<u32> {
    find_processes(|n| n == DOUBAO_IME_SERVICE).into_iter().find(|&pid| {
        process_path(pid).is_some_and(|p| p.to_string_lossy().to_ascii_lowercase().contains(r"\doubaoime\"))
    })
}

#[derive(Default)]
struct DoubaoImeConfig {
    /// `voice.enableVoiceShortcut`
    shortcut: bool,
    /// `voice.enableGlobalVoiceShortcut`
    global: bool,
    /// `voice.voiceShortcutMode` (`right_alt_space`, …)
    mode: String,
}

/// Reads the voice shortcut settings from `%APPDATA%\DoubaoIme\conf\config.json` (nothing else).
fn doubao_ime_config() -> Option<DoubaoImeConfig> {
    let path = PathBuf::from(std::env::var_os("APPDATA")?).join(r"DoubaoIme\conf\config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    Some(DoubaoImeConfig {
        shortcut: json_field(text, "enableVoiceShortcut").is_none_or(|v| v == "true"),
        global: json_field(text, "enableGlobalVoiceShortcut").is_some_and(|v| v == "true"),
        mode: json_field(text, "voiceShortcutMode").unwrap_or("right_alt_space").to_owned(),
    })
}

/// The toggle chord of `engine` as (vk, up) pairs, for one `SendInput`.
fn toggle_chord(engine: VoiceEngine) -> Vec<(u16, bool)> {
    match engine {
        // Shift between Win down/up: no Start menu even if WeType doesn't take it.
        VoiceEngine::WeType => vec![
            (VK_LCONTROL, false),
            (VK_LWIN, false),
            (VK_LSHIFT, false),
            (VK_LSHIFT, true),
            (VK_LWIN, true),
            (VK_LCONTROL, true),
        ],
        // `right_alt_space` (豆包输入法's default 「免按模式」 shortcut). Right Alt is an extended key.
        VoiceEngine::DoubaoIme => chord(&[VK_RMENU, VK_SPACE]),
        VoiceEngine::DoubaoVoice => {
            let keys = doubao_config().and_then(|c| parse_hotkey(&c.hotkey)).unwrap_or_else(|| vec![VK_F6]);
            chord(&keys)
        }
        VoiceEngine::System => chord(&[VK_LWIN, VK_H]),
    }
}

/// Keys down in order, up in reverse.
fn chord(keys: &[u16]) -> Vec<(u16, bool)> {
    keys.iter().map(|&k| (k, false)).chain(keys.iter().rev().map(|&k| (k, true))).collect()
}

/// `"F6"`, `"Ctrl+Alt+V"`, `"Shift + F9"` → virtual keys (modifiers first as written).
fn parse_hotkey(s: &str) -> Option<Vec<u16>> {
    let mut out = Vec::new();
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let p = part.to_ascii_lowercase();
        let vk = match p.as_str() {
            "ctrl" | "control" | "lctrl" => VK_LCONTROL,
            "shift" | "lshift" => VK_LSHIFT,
            "alt" | "menu" | "lalt" => VK_LMENU,
            "win" | "windows" | "lwin" | "meta" => VK_LWIN,
            "space" => 0x20,
            "esc" | "escape" => VK_ESCAPE,
            "enter" | "return" => 0x0D,
            "tab" => 0x09,
            "capslock" | "capital" => 0x14,
            "insert" => 0x2D,
            "home" => 0x24,
            "end" => 0x23,
            "pause" => 0x13,
            "scroll" | "scrolllock" => 0x91,
            _ => {
                let b = p.as_bytes();
                if b.len() == 1 && (b[0].is_ascii_alphanumeric()) {
                    b[0].to_ascii_uppercase() as u16
                } else if let Some(n) = p.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
                    (1..=24).contains(&n).then_some(0x6F + n)?
                } else {
                    // .NET `Key.D1` style digits.
                    let d = p.strip_prefix('d').filter(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit())?;
                    d.as_bytes()[0] as u16
                }
            }
        };
        out.push(vk);
    }
    (!out.is_empty()).then_some(out)
}

struct DoubaoConfig {
    configured: bool,
    hotkey: String,
}

/// Reads only `IsConfigured` and `HotKey` from `%APPDATA%\DouBaoVoice\config.json`.
fn doubao_config() -> Option<DoubaoConfig> {
    let path = PathBuf::from(std::env::var_os("APPDATA")?).join(r"DouBaoVoice\config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    Some(DoubaoConfig {
        configured: json_field(text, "IsConfigured").is_some_and(|v| v.eq_ignore_ascii_case("true")),
        hotkey: json_field(text, "HotKey").unwrap_or("F6").to_string(),
    })
}

/// The raw value of a top-level scalar field (`"key": "v"` → `v`, `"key": true` → `true`).
fn json_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = text[from..].find(&pat) {
        let rest = text[from + i + pat.len()..].trim_start();
        from += i + pat.len();
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let rest = rest.trim_start();
        return if let Some(s) = rest.strip_prefix('"') {
            s.find('"').map(|e| &s[..e])
        } else {
            let end = rest.find([',', '}', '\n', '\r']).unwrap_or(rest.len());
            Some(rest[..end].trim())
        };
    }
    None
}

/// The DouBaoVoice exe: the setting, the running process, or the newest `DouBaoVoice*.exe` in
/// the known download / install folders.
fn doubao_exe(setting: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = setting {
        return p.is_file().then(|| p.to_path_buf());
    }
    if let Some(p) = find_process(|n| n.starts_with(DOUBAO_PREFIX)).and_then(process_path) {
        return Some(p);
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(home) = env("USERPROFILE") {
        dirs.push(home.join("Downloads"));
        dirs.push(home.join("Desktop"));
    }
    if let Some(l) = env("LOCALAPPDATA") {
        dirs.push(l.join(r"Programs\DouBaoVoice"));
        dirs.push(l.join("DouBaoVoice"));
    }
    if let Some(a) = env("APPDATA") {
        dirs.push(a.join("DouBaoVoice"));
    }
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !(name.starts_with(DOUBAO_PREFIX) && name.ends_with(".exe")) {
                continue;
            }
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                best = Some((t, e.path()));
            }
        }
    }
    best.map(|(_, p)| p)
}

// ---------------------------------------------------------------- microphone

/// Whether `engine`'s process holds the microphone, according to the consent store. With
/// `since_filetime != 0`, only a use that started after it (minus 1 s) counts, so a stale entry
/// (crashed process) doesn't look like a fresh start.
/// `target`: the target app's exe and the session's start (豆包输入法 may record inside it; only a
/// use that started with this session counts, not another app's long recording).
fn mic_in_use(engine: VoiceEngine, since_filetime: u64, target: Option<(&str, u64)>) -> bool {
    let root = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged";
    let Some(key) = RegKey::open(HKEY_CURRENT_USER, root) else { return false };
    key.subkeys().into_iter().any(|name| {
        let lower = name.to_ascii_lowercase();
        let exe = lower.rsplit('#').next().unwrap_or("");
        let mut since = since_filetime;
        let matches = match engine {
            VoiceEngine::WeType => exe == WETYPE_MIC_EXE,
            VoiceEngine::DoubaoVoice => exe.starts_with(DOUBAO_PREFIX),
            // Its service, or the target app (if it records inside its text service).
            VoiceEngine::DoubaoIme => {
                lower.contains(DOUBAO_IME_DIR)
                    || target.is_some_and(|(t, start)| {
                        since = since.max(start);
                        t == exe
                    })
            }
            VoiceEngine::System => false,
        };
        if !matches {
            return false;
        }
        let Some(sub) = RegKey::open(key.0, &name) else { return false };
        let start = sub.qword("LastUsedTimeStart").unwrap_or(0);
        let stop = sub.qword("LastUsedTimeStop").unwrap_or(1);
        stop == 0 && start != 0 && start + 10_000_000 >= since
    })
}

/// FILETIME (100 ns since 1601) of now, comparable with `LastUsedTimeStart`.
fn filetime_now() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    116_444_736_000_000_000 + (d.as_nanos() / 100) as u64
}

/// WeType's voice window (also shown with the "已复制" tip, so only a hint).
fn wetype_voice_window() -> bool {
    unsafe extern "system" fn cb(h: HWND, found: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(h).as_bool() {
                let mut c = [0u16; 64];
                let n = GetClassNameW(h, &mut c) as usize;
                if String::from_utf16_lossy(&c[..n]) == WETYPE_VOICE_CLASS {
                    let mut t = [0u16; 64];
                    let n = GetWindowTextW(h, &mut t) as usize;
                    if String::from_utf16_lossy(&t[..n]) == WETYPE_VOICE_TITLE {
                        *(found.0 as *mut bool) = true;
                        return BOOL(0);
                    }
                }
            }
            BOOL(1)
        }
    }
    let mut found = false;
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut found as *mut bool as isize)) };
    found
}

/// Whether process `pid` has a top-level window for which `pred(class, visible)` holds.
fn process_has_window(pid: u32, pred: fn(&str, bool) -> bool) -> bool {
    struct St {
        pid: u32,
        pred: fn(&str, bool) -> bool,
        found: bool,
    }
    unsafe extern "system" fn cb(h: HWND, p: LPARAM) -> BOOL {
        unsafe {
            let st = &mut *(p.0 as *mut St);
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == st.pid {
                let mut c = [0u16; 64];
                let n = GetClassNameW(h, &mut c) as usize;
                if (st.pred)(&String::from_utf16_lossy(&c[..n]), IsWindowVisible(h).as_bool()) {
                    st.found = true;
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
    }
    let mut st = St { pid, pred, found: false };
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut st as *mut St as isize)) };
    st.found
}

/// `wetype_update.exe` next to the running `wetype_server.exe` (same version directory).
fn wetype_update_exe() -> Option<PathBuf> {
    let server = find_process(|n| n == "wetype_server.exe").and_then(process_path)?;
    let exe = server.with_file_name(WETYPE_MIC_EXE);
    exe.is_file().then_some(exe)
}

// ---------------------------------------------------------------- processes

/// All processes whose lower-case exe name matches.
fn find_processes(pred: impl Fn(&str) -> bool) -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if pred(&String::from_utf16_lossy(&e.szExeFile[..n]).to_ascii_lowercase()) {
                out.push(e.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// The process of the foreground window.
fn foreground_pid() -> Option<u32> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// Mandatory integrity level RID of a process (0x2000 medium, 0x3000 high), if it can be read.
fn integrity_level(pid: u32) -> Option<u32> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(h, TOKEN_QUERY, &mut token).is_ok();
        let _ = CloseHandle(h);
        if !opened {
            return None;
        }
        let mut buf = [0u64; 16]; // TOKEN_MANDATORY_LABEL + SID, aligned
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenIntegrityLevel, Some(buf.as_mut_ptr().cast()), size_of_val(&buf) as u32, &mut len).is_ok();
        let _ = CloseHandle(token);
        if !ok {
            return None;
        }
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, count as u32 - 1))
    }
}

/// The engine's (un-elevated) process can't get the hotkey while a window of higher integrity
/// (an administrator window) is in front: Windows drops injected input aimed at it for lower
/// processes' hooks (tested 2026-10-06, see the module docs). Returns the message to show.
fn blocked_by_elevated_foreground(engine: VoiceEngine) -> Option<String> {
    let engine_pid = match engine {
        VoiceEngine::WeType => find_process(|n| n == WETYPE_MIC_EXE),
        VoiceEngine::DoubaoIme => doubao_ime_service(),
        VoiceEngine::DoubaoVoice => find_process(|n| n.starts_with(DOUBAO_PREFIX)),
        VoiceEngine::System => None,
    }?;
    let fg_pid = foreground_pid()?;
    // Our own window in front (right after the tray menu, which activates its hidden window):
    // no text field to type into, and not an administrator window either.
    if fg_pid == std::process::id() {
        return Some("先点一下要输入文字的地方，再点麦克风".to_owned());
    }
    let fg = integrity_level(fg_pid)?;
    let eng = integrity_level(engine_pid)?;
    (fg > eng).then(|| format!("{}收不到管理员窗口里的语音快捷键（Windows 权限隔离），请在普通窗口里使用", engine.label()))
}

/// First process whose lower-case exe name matches.
fn find_process(pred: impl Fn(&str) -> bool) -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if pred(&String::from_utf16_lossy(&e.szExeFile[..n]).to_ascii_lowercase()) {
                found = Some(e.th32ProcessID);
                break;
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}

fn process_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])))
    }
}

/// Whether the current clipboard content was put there by `engine`. WeType sets the clipboard
/// without an owner window (tested 2026-10-06), so "no owner while WeType's voice / tip window is
/// up" counts too. A copy owned by any other process (the user copied something meanwhile) does
/// not.
fn clipboard_from_engine(engine: VoiceEngine, owner_exe: Option<&str>) -> bool {
    match (engine, owner_exe) {
        (VoiceEngine::WeType, Some(n)) => n.starts_with("wetype"),
        (VoiceEngine::WeType, None) => wetype_voice_window(),
        (VoiceEngine::DoubaoIme, Some(n)) => n == DOUBAO_IME_SERVICE,
        (VoiceEngine::DoubaoIme, None) => false,
        (VoiceEngine::DoubaoVoice, Some(n)) => n.starts_with(DOUBAO_PREFIX),
        (VoiceEngine::DoubaoVoice, None) => true,
        (VoiceEngine::System, _) => false,
    }
}

/// Lower-case exe name of the clipboard owner's process (None: no owner window).
fn clipboard_owner_exe() -> Option<String> {
    let owner = unsafe { GetClipboardOwner() }.ok()?;
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(owner, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let p = process_path(pid)?;
    Some(p.file_name()?.to_string_lossy().to_ascii_lowercase())
}

// ---------------------------------------------------------------- input

fn send_keys(keys: &[(u16, bool)]) -> bool {
    let events: Vec<INPUT> = keys
        .iter()
        .map(|&(vk, up)| {
            let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            let mut flags = KEYBD_EVENT_FLAGS(0);
            // Win, right Ctrl/Alt and the navigation block are extended keys.
            if matches!(vk, 0x5B | 0x5C | 0xA3 | 0xA5 | 0x21..=0x2E) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if up {
                flags |= KEYEVENTF_KEYUP;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
            }
        })
        .collect();
    let n = unsafe { SendInput(&events, size_of::<INPUT>() as i32) };
    n as usize == events.len()
}

// ---------------------------------------------------------------- clipboard

/// A copy of the clipboard's memory-based formats, in their original order.
pub struct ClipSnapshot {
    formats: Vec<(u32, Vec<u8>)>,
    /// Formats that could not be copied (GDI handles, over the size cap).
    pub skipped: usize,
}

/// Formats whose clipboard handle is not an HGLOBAL (GDI objects, metafiles, owner display).
fn is_handle_format(f: u32) -> bool {
    matches!(f, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E) || (0x300..=0x3FF).contains(&f)
}

/// Opens the clipboard, retrying briefly if another process holds it.
fn open_clipboard(owner: Option<HWND>) -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    false
}

fn global_bytes(h: HANDLE) -> Option<Vec<u8>> {
    let g = HGLOBAL(h.0);
    unsafe {
        let size = GlobalSize(g);
        if size == 0 {
            return None;
        }
        let p = GlobalLock(g) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, size).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }
}

impl ClipSnapshot {
    /// Copies the clipboard. Returns (snapshot, ok); `ok` is false if the clipboard could not be
    /// opened (then there is nothing to restore).
    pub fn take() -> (Option<ClipSnapshot>, bool) {
        if !open_clipboard(None) {
            return (None, false);
        }
        let skip_marks = [history_format(), cloud_format()];
        let mut snap = ClipSnapshot { formats: Vec::new(), skipped: 0 };
        let mut total = 0usize;
        let mut f = 0u32;
        loop {
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break;
            }
            if skip_marks.contains(&f) {
                continue;
            }
            if is_handle_format(f) {
                snap.skipped += 1;
                continue;
            }
            let Some(bytes) = unsafe { GetClipboardData(f) }.ok().and_then(global_bytes) else {
                snap.skipped += 1;
                continue;
            };
            if total + bytes.len() > CLIPBOARD_CAP && f != 13 {
                snap.skipped += 1;
                continue;
            }
            total += bytes.len();
            snap.formats.push((f, bytes));
        }
        let _ = unsafe { CloseClipboard() };
        (Some(snap), true)
    }

    /// Puts the saved formats back (an empty snapshot empties the clipboard). Returns the number
    /// of formats restored, None if the clipboard could not be opened.
    pub fn restore(&self) -> Option<usize> {
        // SetClipboardData needs an owner window (docs: with a NULL owner it may fail).
        let hwnd = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), PCWSTR::null(), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None)
        }
        .ok();
        let mut n = None;
        if open_clipboard(hwnd) {
            let mut count = 0;
            if unsafe { EmptyClipboard() }.is_ok() {
                for (f, bytes) in &self.formats {
                    if set_bytes(*f, bytes) {
                        count += 1;
                    }
                }
                if !self.formats.is_empty() {
                    // Keep Win+V history / cloud clipboard from getting the same item again.
                    let zero = 0u32.to_le_bytes();
                    set_bytes(history_format(), &zero);
                    set_bytes(cloud_format(), &zero);
                }
                n = Some(count);
            }
            let _ = unsafe { CloseClipboard() };
        }
        if let Some(h) = hwnd {
            let _ = unsafe { DestroyWindow(h) };
        }
        n
    }

    /// FNV-1a over (format, bytes): compares two snapshots without looking at the content.
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        let mut eat = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        };
        for (f, bytes) in &self.formats {
            f.to_le_bytes().into_iter().for_each(&mut eat);
            bytes.iter().copied().for_each(&mut eat);
        }
        h
    }

    pub fn format_count(&self) -> usize {
        self.formats.len()
    }
}

fn history_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")) }
}

fn cloud_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")) }
}

/// Replaces the clipboard with `text` (tests / the probe).
pub fn put_clipboard_text(text: &str) -> bool {
    let units: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    ClipSnapshot { formats: vec![(13, units)], skipped: 0 }.restore().is_some()
}

/// SetClipboardData with a fresh HGLOBAL copy (the clipboard must be open and owned).
fn set_bytes(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else { return false };
        let p = GlobalLock(g) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        if SetClipboardData(format, Some(HANDLE(g.0))).is_ok() {
            true
        } else {
            let _ = GlobalFree(Some(g));
            false
        }
    }
}

/// `CF_UNICODETEXT` of the clipboard, if any.
fn read_clipboard_text() -> Option<String> {
    if !open_clipboard(None) {
        return None;
    }
    let text = unsafe { GetClipboardData(13) }.ok().and_then(global_bytes).map(|b| {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let n = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..n])
    });
    let _ = unsafe { CloseClipboard() };
    text
}

// ---------------------------------------------------------------- registry

struct RegKey(HKEY);

impl RegKey {
    fn open(root: HKEY, path: &str) -> Option<RegKey> {
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let mut h = HKEY::default();
        let r = unsafe { RegOpenKeyExW(root, PCWSTR(wide.as_ptr()), None, KEY_READ | KEY_WOW64_64KEY, &mut h) };
        r.is_ok().then_some(RegKey(h))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut n = buf.len() as u32;
            let r = unsafe { RegEnumKeyExW(self.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut n, None, None, None, None) };
            if r.is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..n as usize]));
        }
        out
    }

    fn value_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut n = buf.len() as u32;
            let r = unsafe { RegEnumValueW(self.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut n, None, None, None, None) };
            if r.is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..n as usize]));
        }
        out
    }

    fn qword(&self, name: &str) -> Option<u64> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut v = 0u64;
        let mut size = 8u32;
        let mut ty = REG_VALUE_TYPE::default();
        let r = unsafe {
            RegQueryValueExW(self.0, PCWSTR(wide.as_ptr()), None, Some(&mut ty), Some(&mut v as *mut u64 as *mut u8), Some(&mut size))
        };
        (r.is_ok() && size == 8).then_some(v)
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn reg_key_exists(root: HKEY, path: &str) -> bool {
    RegKey::open(root, path).is_some()
}

fn reg_subkeys(root: HKEY, path: &str) -> Vec<String> {
    RegKey::open(root, path).map(|k| k.subkeys()).unwrap_or_default()
}

fn reg_value_names(root: HKEY, path: &str) -> Vec<String> {
    RegKey::open(root, path).map(|k| k.value_names()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_parse() {
        assert_eq!(parse_hotkey("F6"), Some(vec![VK_F6]));
        assert_eq!(parse_hotkey("Ctrl + Alt + V"), Some(vec![VK_LCONTROL, VK_LMENU, b'V' as u16]));
        assert_eq!(parse_hotkey("Shift+D1"), Some(vec![VK_LSHIFT, b'1' as u16]));
        assert_eq!(parse_hotkey("F25"), None);
        assert_eq!(parse_hotkey(""), None);
    }

    #[test]
    fn json_fields() {
        let j = "{\n  \"AppId\": \"x\",\n  \"HotKey\" : \"F6\",\n  \"IsConfigured\": true\n}";
        assert_eq!(json_field(j, "HotKey"), Some("F6"));
        assert_eq!(json_field(j, "IsConfigured"), Some("true"));
        assert_eq!(json_field(j, "Missing"), None);
    }

    #[test]
    fn wetype_chord_keeps_win_inside_shift() {
        let c = toggle_chord(VoiceEngine::WeType);
        assert_eq!(c.len(), 6);
        assert_eq!(c[2], (VK_LSHIFT, false));
        assert_eq!(c[3], (VK_LSHIFT, true));
        assert_eq!(c[4], (VK_LWIN, true));
    }

    #[test]
    fn doubao_ime_chord_is_right_alt_space() {
        let c = toggle_chord(VoiceEngine::DoubaoIme);
        assert_eq!(c, vec![(VK_RMENU, false), (VK_SPACE, false), (VK_SPACE, true), (VK_RMENU, true)]);
    }

    #[test]
    fn settings_roundtrip() {
        for e in VoiceEngine::ALL {
            assert_eq!(VoiceEngine::from_setting(e.as_setting()), Some(e));
        }
        assert_eq!(VoiceEngine::from_setting("bogus"), None);
    }
}
