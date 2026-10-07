//! The application behind the keyboard window: input controller + settings + system glue.

use std::any::Any;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dianmo_core::{Action, Engine, InputController};
use dianmo_ui::settings::{InputMode, LayoutChoice, Page, SettingsModel, Status, ThemeChoice, UpdateState};
use dianmo_ui::{InputState, KeyboardView, Response, ThemeKind, UiAction, View};
use dianmo_win::tabtip::SystemKeyboardSettings;
use dianmo_win::{
    App, BallEdge, BallEvent, BallState, FieldKind, FocusEvent, FocusWatcher, HostControl, HostProxy,
    SendInputSink, TrayItem, WindowId, now_ms, start_focus_watcher,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

#[cfg(feature = "rime")]
use crate::basic::BasicEngine;
use crate::bubble;
use crate::clipboard::{self, ClipEvent, ClipStore, ClipboardWatcher};
use crate::engine::AnyEngine;
use crate::log;
use crate::platform;
use crate::prefs;
use crate::settings::Settings;
use crate::sound::KeySound;
use crate::voice::{Voice, VoiceEngine, VoiceState};

mod rime_ui;
mod settings_ui;

/// The floating ball shows while the keyboard is hidden: the 显示悬浮球 setting, and always in
/// voice mode (it is the voice button).
pub fn ball_wanted(s: &Settings) -> bool {
    (s.show_ball || s.input_mode == InputMode::VoiceBall) && !disabled("ball")
}

pub struct DianmoApp {
    ctl: InputController<AnyEngine, SendInputSink>,
    settings: Settings,
    settings_path: PathBuf,
    status: EngineStatus,
    /// librime options; taken by `on_start`, which starts librime on a background thread.
    #[cfg(feature = "rime")]
    rime_opts: Option<dianmo_rime::Options>,
    /// A started librime waiting for the current composition (in the stand-in engine) to end.
    #[cfg(feature = "rime")]
    pending_rime: Option<dianmo_rime::RimeEngine>,
    /// Debounce for auto-hide: a pending hide only fires if no newer focus event arrived.
    focus_seq: u64,
    focus: Option<FocusWatcher>,
    /// The number pad was opened because a numeric field got focus.
    numbers_for_field: bool,
    /// The next "shown" notification is the result of a focus event (not a manual show).
    showing_for_focus: bool,
    manual_show_at: Option<Instant>,
    tray_shown: Vec<TrayItem>,
    /// Clipboard history (TODO #32); pinned entries live in `clips_path`.
    clips: ClipStore,
    clips_path: PathBuf,
    clip_watch: Option<ClipboardWatcher>,
    /// Clipboard changes seen so far (to tell whether a 复制 copied anything).
    clip_updates: u64,
    /// A password field has focus: clipboard changes are not recorded.
    password_focus: bool,
    /// Voice input through the selected engine (TODO #27 / #29).
    voice: Voice,
    /// The voice state last shown (mic keys, ball, failure toast).
    voice_shown: VoiceState,
    /// The ball currently shows the listening halo.
    ball_listening: bool,
    /// The 300 ms poll timer is armed (only while `voice.needs_poll()`).
    voice_timer: bool,
    /// Clipboard changes until then are the voice engine's result or our restore of the user's
    /// clipboard: not recorded in the history.
    voice_clip_quiet_until: Option<Instant>,
    /// Why each engine (`VoiceEngine::ALL` order) can't be used; checked in the background.
    voice_unavailable: [Option<String>; ENGINES],
    /// The voice-mode hint (「点一下说话，长按展开键盘」) was shown in this run.
    voice_hint_shown: bool,
    /// The engine availability above was checked at least once (else 「正在检测…」).
    voice_checked: bool,
    /// The settings window and the first-run onboarding, while open (TODO #33).
    pub(crate) settings_win: Option<WindowId>,
    pub(crate) onboarding_win: Option<WindowId>,
    /// The model last sent to those windows (sent again only when it changes).
    shown_model: Option<SettingsModel>,
    /// 管理员窗口支持 (the scheduled task), checked in the background while settings are open.
    admin_task: Status,
    /// 检查更新 (P6).
    pub(crate) update: UpdateState,
    /// The newer release found by the last check (for 立即更新).
    release: Option<crate::update::Release>,
    /// Windows apps use the dark theme (for 主题「跟随系统」).
    system_dark: bool,
    /// 按键音: the audio thread, only while the setting is on (`sound.rs`).
    sound: Option<KeySound>,
    /// librime options (user directory, 双拼方案) for 模糊音 and engine reloads (`rime_ui.rs`).
    #[cfg(feature = "rime")]
    rime_options: Option<dianmo_rime::Options>,
    /// The engine is away on a job thread (`rime_ui.rs`).
    #[cfg(feature = "rime")]
    rime_busy: bool,
    /// `now_ms()` of the last keyboard input (a word count waits for a pause, rime_ui).
    last_input_ms: u64,
    /// A delayed retry of a waiting word count is on its way (rime_ui).
    #[cfg(feature = "rime")]
    rime_idle_check: bool,
    #[cfg(feature = "rime")]
    rime_jobs: std::collections::VecDeque<rime_ui::RimeJob>,
    /// `dianmo.exe --deploy-user` is running; `fuzzy_again`: 模糊音 changed meanwhile.
    #[cfg(feature = "rime")]
    fuzzy_deploying: bool,
    #[cfg(feature = "rime")]
    fuzzy_again: bool,
    /// What the settings page says about 模糊音 and the user dictionary.
    fuzzy_status: Status,
    user_words: Option<u32>,
    user_dict_status: Status,
    /// The tray icon's 「新版本」 red dot (settings_ui).
    update_badge: crate::update::Badge,
}

const ENGINES: usize = VoiceEngine::ALL.len();

/// Voice engine availability, from the background check.
struct VoiceAvail([Option<String>; ENGINES]);

/// Fired by the voice poll timer (a one-shot `SetTimer` on the keyboard window).
struct VoiceTick;

/// Posted to ourselves when the ball appears in voice mode: keep it out, show the hint.
struct VoiceIntro;

const VOICE_POLL_MS: u32 = 300;
const VOICE_TIMER_ID: usize = 0xD1A0;
/// After a voice session, clipboard changes are attributed to it for this long (WeType copies its
/// result up to ~2.5 s after stopping; DoubaoVoice pastes 1–3 s after; we restore the user's
/// clipboard after that).
const VOICE_CLIP_QUIET: Duration = Duration::from_secs(3);
/// Voice mode: how long the ball stays fully out of the screen edge after entering voice mode,
/// and how long the hint next to it stays.
const VOICE_INTRO_MS: u32 = 10_000;
const VOICE_HINT: &str = "点一下说话，长按展开键盘";
/// A failure message next to the ball (keyboard hidden).
const VOICE_FAIL_BUBBLE_MS: u32 = 5_000;

thread_local! {
    static VOICE_TIMER_PROXY: Cell<Option<HostProxy>> = const { Cell::new(None) };
}

unsafe extern "system" fn voice_timer_proc(hwnd: HWND, _msg: u32, id: usize, _time: u32) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
    if let Some(p) = VOICE_TIMER_PROXY.with(|p| p.get()) {
        p.post(VoiceTick);
    }
}

/// Test hook: `DIANMO_NO=clipboard,ball,focus` leaves those parts off (memory measurements).
pub fn disabled(part: &str) -> bool {
    std::env::var("DIANMO_NO").is_ok_and(|v| v.split(',').any(|p| p.trim() == part))
}

/// Posted to ourselves [`COPY_CHECK_MS`] after a 复制 button: the clipboard update count then.
struct CopyCheck(u64);

/// After 复制, wait this long for the clipboard to change before deciding nothing was selected.
const COPY_CHECK_MS: u64 = 350;

/// librime starts this long after the window (see `start_rime`).
#[cfg(feature = "rime")]
const RIME_START_DELAY_MS: u64 = 250;

/// Posted to ourselves after the auto-hide delay.
struct HideLater(u64);

/// How librime will be used.
pub enum RimeSetup {
    #[cfg(feature = "rime")]
    Start(dianmo_rime::Options),
    /// Not used; the reason is shown in the tray menu.
    Unavailable(String),
}

#[cfg_attr(not(feature = "rime"), allow(dead_code))]
enum EngineStatus {
    Loading,
    Rime,
    Off(String),
}

/// From the librime start thread: the engine (or error) and how long it took.
#[cfg(feature = "rime")]
struct RimeReady(Result<dianmo_rime::RimeEngine, dianmo_rime::Error>, u128);

impl DianmoApp {
    /// `rime`: librime options to start in the background (the window comes up with the stand-in
    /// engine first; librime takes 150–700 ms to start), or why it can't be used.
    pub fn new(engine: AnyEngine, rime: RimeSetup, settings: Settings, settings_path: PathBuf) -> Self {
        #[cfg(feature = "rime")]
        let rime = match rime {
            RimeSetup::Start(mut o) => {
                o.shuangpin = rime_ui::rime_scheme(settings.shuangpin);
                RimeSetup::Start(o)
            }
            other => other,
        };
        // The data directory (`--instance` aware): clips.txt lives next to settings.ini.
        let clips_path = settings_path.with_file_name("clips.txt");
        let mut ctl = InputController::new(engine, SendInputSink::new());
        ctl.set_space_commits_first(settings.space_commits_first);
        ctl.set_full_width_punct(settings.full_width_punct);
        ctl.set_semicolon_input(settings.shuangpin.uses_semicolon());
        ctl.handle(Action::SetSchema(settings.schema));
        if !settings.chinese {
            ctl.handle(Action::ToggleChinese);
        }
        let mut voice = Voice::new(VoiceEngine::from_setting(&settings.voice_engine).unwrap_or(VoiceEngine::WeType));
        voice.set_log(log::write);
        // A failing engine only says why: Windows voice typing (Win+H) is the user's own choice
        // in the tray, never a fallback (its panel kept popping up, 2026-10-06).
        voice.set_fallback(false);
        voice.set_doubao_exe(Some(PathBuf::from(&settings.voice_doubao_exe)));
        Self {
            ctl,
            settings,
            settings_path,
            status: match &rime {
                RimeSetup::Unavailable(why) => EngineStatus::Off(why.clone()),
                #[cfg(feature = "rime")]
                RimeSetup::Start(_) => EngineStatus::Loading,
            },
            #[cfg(feature = "rime")]
            rime_opts: match rime {
                RimeSetup::Start(o) => Some(o),
                RimeSetup::Unavailable(_) => None,
            },
            #[cfg(feature = "rime")]
            pending_rime: None,
            focus_seq: 0,
            focus: None,
            numbers_for_field: false,
            showing_for_focus: false,
            manual_show_at: None,
            tray_shown: Vec::new(),
            clips: ClipStore::load(&clips_path),
            clips_path,
            clip_watch: None,
            clip_updates: 0,
            password_focus: false,
            voice,
            voice_shown: VoiceState::Idle,
            ball_listening: false,
            voice_timer: false,
            voice_clip_quiet_until: None,
            voice_unavailable: Default::default(),
            voice_hint_shown: false,
            voice_checked: false,
            settings_win: None,
            onboarding_win: None,
            shown_model: None,
            admin_task: Status::default(),
            update: UpdateState::Unknown,
            release: None,
            system_dark: dianmo_win::system_dark_mode(),
            sound: None,
            #[cfg(feature = "rime")]
            rime_options: None,
            #[cfg(feature = "rime")]
            rime_busy: false,
            last_input_ms: 0,
            #[cfg(feature = "rime")]
            rime_idle_check: false,
            #[cfg(feature = "rime")]
            rime_jobs: Default::default(),
            #[cfg(feature = "rime")]
            fuzzy_deploying: false,
            #[cfg(feature = "rime")]
            fuzzy_again: false,
            fuzzy_status: Status::default(),
            user_words: None,
            user_dict_status: Status::default(),
            update_badge: Default::default(),
        }
    }

    fn save_clips(&self) {
        if let Err(e) = self.clips.save_pinned(&self.clips_path) {
            log!("saving pinned clips failed: {e}");
        }
    }

    fn push_clips(&self, view: &mut dyn View) -> Response {
        let changed = view
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<KeyboardView>())
            .is_some_and(|kv| kv.set_clips(self.clips.items().to_vec()));
        if changed { Response::repaint() } else { Response::none() }
    }

    fn on_clip_event(&mut self, ev: ClipEvent, view: &mut dyn View, host: &mut HostControl) -> Response {
        if !matches!(ev, ClipEvent::PasteFailed(_)) && self.voice_clip_quiet() {
            // The engine's result going through the clipboard, or our restore of the user's
            // clipboard afterwards: neither belongs in the history (and the restored content
            // is what the paste preview already shows).
            log!("clipboard change during voice input: not recorded");
            return Response::none();
        }
        let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) else { return Response::none() };
        match ev {
            ClipEvent::Text(text) => {
                self.clip_updates += 1;
                if self.password_focus && self.settings.clip_skip_passwords {
                    // Copied from / while in a password field: don't keep or show it.
                    kv.set_paste_preview(None);
                    return Response::repaint();
                }
                kv.set_paste_preview(Some(&text));
                if !self.settings.clip_history {
                    // 记录剪贴板历史 off: the paste key still shows what is on the clipboard.
                    return Response::repaint();
                }
                if self.clips.add(text) {
                    kv.set_clips(self.clips.items().to_vec());
                }
                if host.is_visible() {
                    return kv.notify_copied(now_ms());
                }
                Response::repaint()
            }
            ClipEvent::NoText | ClipEvent::Private => {
                self.clip_updates += 1;
                if kv.set_paste_preview(None) { Response::repaint() } else { Response::none() }
            }
            ClipEvent::PasteFailed(text) => {
                log!("pasting through the clipboard failed; typing {} chars", text.chars().count());
                self.input(Action::Text(text), view)
            }
        }
    }

    /// Pastes a clipboard entry: typed with Unicode `SendInput` (the clipboard is untouched);
    /// long or multi-line text goes through the clipboard and Ctrl+V (typing a newline would press
    /// Enter, which sends the message in chat apps).
    fn paste(&mut self, text: String, view: &mut dyn View) -> Response {
        let long = text.chars().count() > clipboard::TYPE_MAX_CHARS || text.contains('\n');
        match &self.clip_watch {
            Some(w) if long => {
                w.paste(text);
                Response::none()
            }
            _ => self.input(Action::Text(text), view),
        }
    }

    fn start_clipboard(&mut self, host: &mut HostControl) {
        match clipboard::start(host.proxy()) {
            Ok(w) => self.clip_watch = Some(w),
            Err(e) => log!("clipboard watcher failed to start (no clipboard history): {e}"),
        }
    }

    fn input_state(&self) -> InputState {
        InputState { snapshot: self.ctl.state().clone(), chinese: self.ctl.is_chinese(), schema: self.ctl.schema() }
    }

    fn save(&self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            log!("saving settings failed: {e}");
        }
    }

    /// Persists schema / 中英 after the controller changed them.
    fn sync_mode(&mut self) {
        let (schema, chinese) = (self.ctl.schema(), self.ctl.is_chinese());
        if schema != self.settings.schema || chinese != self.settings.chinese {
            self.settings.schema = schema;
            self.settings.chinese = chinese;
            self.save();
        }
    }

    fn input(&mut self, action: Action, view: &mut dyn View) -> Response {
        self.last_input_ms = now_ms();
        self.ctl.handle(action);
        let mut r = view.set_input_state(self.input_state());
        self.sync_mode();
        let swapped = self.swap_in_rime(view);
        r.repaint |= swapped.repaint;
        r.actions.extend(swapped.actions);
        r
    }

    /// The keyboard theme for the 主题 setting (跟随系统 resolves to the Windows app mode).
    pub(crate) fn theme_kind(&self) -> ThemeKind {
        prefs::effective_theme(self.settings.theme, self.system_dark)
    }

    fn voice_mode(&self) -> bool {
        self.settings.input_mode == InputMode::VoiceBall
    }

    /// Switches the keyboard between the letter layouts (`LayoutChoice::English` = 英文) and
    /// leaves the 电脑键盘 (the layout implies the keyboard mode; voice mode stays).
    fn set_layout(&mut self, layout: LayoutChoice, view: &mut dyn View, host: &mut HostControl) -> Response {
        let mut r = Response::repaint();
        if self.settings.input_mode == InputMode::PcKeyboard {
            r = merge(r, self.set_input_mode(InputMode::Keyboard, view, host));
        }
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            kv.show_letters();
        }
        let (schema, chinese) = prefs::layout_schema(layout, self.ctl.schema());
        if self.ctl.schema() != schema {
            r.actions.extend(self.input(Action::SetSchema(schema), view).actions);
        }
        if self.ctl.is_chinese() != chinese {
            r.actions.extend(self.input(Action::ToggleChinese, view).actions);
        }
        r
    }

    /// 键盘 / 语音球 / 电脑键盘 (settings, tray, the keyboard's 语音球 / 电脑键盘 tiles).
    fn set_input_mode(&mut self, mode: InputMode, view: &mut dyn View, host: &mut HostControl) -> Response {
        let was = self.settings.input_mode;
        let pc = mode == InputMode::PcKeyboard;
        let mut r = Response::none();
        if pc && self.ctl.is_composing() {
            r = self.input(Action::Space, view);
        }
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            r = merge(r, kv.release_locked_mods());
            r.repaint |= kv.set_pc_keyboard(pc);
            r.repaint |= kv.set_voice_mode(mode == InputMode::VoiceBall);
        }
        if was != mode {
            self.settings.input_mode = mode;
            self.save();
            log!("input mode {mode:?}");
        }
        host.set_ball_enabled(ball_wanted(&self.settings));
        if mode == InputMode::VoiceBall {
            if was != mode {
                // Each time voice mode is turned on, the ball explains itself once more.
                self.voice_hint_shown = false;
            }
            if host.is_visible() {
                // Shrink into the voice ball (the hint follows in `on_visibility_changed`).
                host.hide();
            } else {
                host.proxy().post(VoiceIntro);
            }
        } else {
            bubble::hide();
        }
        r
    }

    #[cfg(feature = "rime")]
    fn start_rime(&mut self, host: &mut HostControl) {
        let Some(opts) = self.rime_opts.take() else { return };
        self.rime_options = Some(opts.clone());
        let schema = self.ctl.schema();
        let proxy = host.proxy();
        let fuzzy = self.settings.fuzzy;
        let spawned = std::thread::Builder::new().name("dianmo-rime-start".into()).spawn(move || {
            // Let the window come up first: loading rime.dll holds the loader lock, which would
            // stall the UI thread's Direct2D/DirectWrite DLL loads.
            std::thread::sleep(std::time::Duration::from_millis(RIME_START_DELAY_MS));
            // 模糊音: settings.ini is the source of truth; write the customization before
            // librime starts so that a matching user build is used right away (a missing one is
            // deployed after the start, `apply_fuzzy`).
            if let Err(e) = dianmo_rime::set_fuzzy(&opts, fuzzy) {
                log!("fuzzy: writing the customization failed: {e}");
            }
            let t = Instant::now();
            let r = dianmo_rime::RimeEngine::start(&opts, schema);
            proxy.post(RimeReady(r, t.elapsed().as_millis()));
        });
        if let Err(e) = spawned {
            log!("can't spawn the librime start thread: {e}");
            self.engine_failed("无法启动".into());
        }
    }

    #[cfg(feature = "rime")]
    fn on_rime_ready(&mut self, ready: RimeReady, view: &mut dyn View) -> Response {
        match ready.0 {
            Ok(engine) => {
                log!("librime {} started in {} ms", engine.rime_version(), ready.1);
                log_memory("librime started");
                self.pending_rime = Some(engine);
                self.swap_in_rime(view)
            }
            Err(e) => {
                log!("librime failed, staying on the built-in engine: {e}");
                let short = match e {
                    dianmo_rime::Error::Load(_) => "rime.dll 加载失败",
                    dianmo_rime::Error::Rime(_) => "Rime 数据有误",
                };
                self.engine_failed(short.into());
                Response::none()
            }
        }
    }

    /// Replaces the stand-in engine once nothing is being composed in it.
    #[cfg(feature = "rime")]
    fn swap_in_rime(&mut self, view: &mut dyn View) -> Response {
        if self.ctl.is_composing() {
            return Response::none();
        }
        let Some(mut engine) = self.pending_rime.take() else { return Response::none() };
        // The 双拼方案 may have changed while the engine was starting or away on a job.
        engine.set_shuangpin(rime_ui::rime_scheme(self.settings.shuangpin));
        let schema = self.ctl.schema();
        let needs_schema = engine.schema() != schema;
        *self.ctl.engine_mut() = AnyEngine::Rime(engine);
        self.status = EngineStatus::Rime;
        if needs_schema {
            // The user switched schemes while librime was starting.
            self.ctl.handle(Action::SetSchema(schema));
        }
        view.set_input_state(self.input_state())
    }

    #[cfg(not(feature = "rime"))]
    fn swap_in_rime(&mut self, _view: &mut dyn View) -> Response {
        Response::none()
    }

    #[cfg(feature = "rime")]
    fn engine_failed(&mut self, why: String) {
        if let AnyEngine::Basic(b) = self.ctl.engine_mut() {
            b.set_note(crate::basic::NO_DICT);
        }
        self.status = EngineStatus::Off(why);
    }
}

impl App for DianmoApp {
    fn on_start(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        platform::set_proxy(host.proxy());
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            kv.set_edit_area(self.settings.edit_area);
            kv.set_pc_keyboard(self.settings.input_mode == InputMode::PcKeyboard);
            kv.set_voice_mode(self.voice_mode());
            kv.set_key_popup(self.settings.key_popup);
            kv.set_long_press_ms(self.settings.long_press.millis());
            kv.set_candidate_size(self.settings.candidate_size);
            kv.set_full_width_punct(self.settings.full_width_punct);
            kv.set_shuangpin(self.settings.shuangpin);
        }
        self.apply_key_sound(view);
        self.clips.set_limit(self.settings.clip_limit as usize);
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            kv.set_clips(self.clips.items().to_vec());
        }
        if !disabled("clipboard") {
            self.start_clipboard(host);
        }
        #[cfg(feature = "rime")]
        self.start_rime(host);
        self.set_focus_watch(self.settings.auto_show && !disabled("focus"), host);
        VOICE_TIMER_PROXY.with(|p| p.set(Some(host.proxy())));
        self.check_voice_engines(host);
        log!("voice engine {}, input mode {:?}", self.voice.engine().as_setting(), self.settings.input_mode);
        if self.voice_mode() && !host.is_visible() {
            host.proxy().post(VoiceIntro);
        }
        if !self.settings.onboarded && !disabled("onboarding") {
            self.open_onboarding(host);
        }
        self.start_updates(host);
        self.refresh_tray(host);
        log_memory("started");
        Response::repaint()
    }

    fn on_action(&mut self, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        if let UiAction::KeyClick(click) = action {
            // Every key press with 按键音 on: just wake the audio thread.
            if let Some(s) = &self.sound {
                s.play(click);
            }
            return Response::none();
        }
        let r = self.action(action, view, host);
        #[cfg(feature = "rime")]
        self.next_rime_job(host);
        self.refresh_tray(host);
        self.sync_windows(host);
        if keymap_wanted() && host.is_visible() {
            dump_keymap(view);
        }
        r
    }

    fn on_event(&mut self, event: Box<dyn Any + Send>, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = self.event(event, view, host);
        #[cfg(feature = "rime")]
        self.next_rime_job(host);
        self.refresh_tray(host);
        self.sync_windows(host);
        if keymap_wanted() && host.is_visible() {
            dump_keymap(view);
        }
        r
    }

    fn on_visibility_changed(&mut self, visible: bool, view: &mut dyn View, host: &mut HostControl) -> Response {
        let mut voice = Response::none();
        if !visible && !self.voice_mode() && self.voice_running() {
            // The mic key belongs to the keyboard: collapsing it ends the session, no result.
            // (In voice mode the ball runs voice input while the keyboard is hidden.)
            self.voice.cancel();
            voice = self.voice_sync(view, host);
        }
        log!("keyboard {}", if visible { "shown" } else { "hidden" });
        if visible {
            bubble::hide();
        } else if self.voice_mode() && !self.voice_hint_shown {
            // The ball is appearing: the first time in voice mode in this run, keep it out of the
            // edge and say how it works. Posted: the ball's window reaches its place only after
            // the host has finished hiding the keyboard.
            host.proxy().post(VoiceIntro);
        }
        if visible {
            static FIRST: std::sync::Once = std::sync::Once::new();
            FIRST.call_once(|| log_memory("keyboard shown"));
            dump_keymap(view);
            self.manual_show_at = if std::mem::take(&mut self.showing_for_focus) { None } else { Some(Instant::now()) };
        } else {
            // A manual hide cancels a pending auto-hide.
            self.focus_seq += 1;
            // Selection mode, the clipboard bar and toasts belong to the last text field.
            if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
                kv.reset_transient();
                voice = merge(voice, kv.release_locked_mods());
            }
        }
        self.refresh_tray(host);
        if !visible && self.ctl.is_composing() {
            // The target is probably gone; don't leave a stale composition behind.
            voice = merge(voice, self.input(Action::ClearComposition, view));
        }
        // A word count waits for the keyboard to hide (rime_ui).
        #[cfg(feature = "rime")]
        self.next_rime_job(host);
        voice
    }

    fn on_tray_command(&mut self, id: u32, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = self.tray_command(id, view, host);
        self.sync_windows(host);
        r
    }

    fn on_window_action(&mut self, id: WindowId, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        self.window_action(id, action, view, host)
    }

    fn on_window_closed(&mut self, id: WindowId) {
        self.window_closed(id);
    }

    fn on_system_theme_changed(&mut self, dark: bool, view: &mut dyn View, host: &mut HostControl) -> Response {
        if dark == self.system_dark {
            return Response::none();
        }
        self.system_dark = dark;
        log!("Windows app theme: {}", if dark { "dark" } else { "light" });
        if self.settings.theme == ThemeChoice::System { self.apply_theme(view, host) } else { Response::none() }
    }

    fn on_ball(&mut self, event: BallEvent, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = match event {
            // Voice mode: the ball is the voice button.
            BallEvent::Tap if self.voice_mode() => self.voice_toggle(view, host),
            BallEvent::Tap | BallEvent::LongPress => {
                bubble::hide();
                host.show();
                Response::none()
            }
            BallEvent::Moved(p) => {
                self.settings.ball = Some((p.edge == BallEdge::Right, p.y_frac));
                self.save();
                Response::none()
            }
        };
        self.refresh_tray(host);
        self.sync_windows(host);
        r
    }
}

/// Combines two responses (the later timer request wins).
fn merge(mut a: Response, b: Response) -> Response {
    a.repaint |= b.repaint;
    a.actions.extend(b.actions);
    if b.timer_ms.is_some() {
        a.timer_ms = b.timer_ms;
    }
    a
}

/// Logs the process's private bytes (commit charge) — for the memory budget (DESIGN §5).
pub fn log_memory(stage: &str) {
    // The kernel32 export (psapi.dll's GetProcessMemoryInfo would add an import).
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut _, c.cb) }.as_bool();
    if ok {
        log!(
            "memory ({stage}): private {:.1} MB, working set {:.1} MB",
            c.PrivateUsage as f64 / 1048576.0,
            c.WorkingSetSize as f64 / 1048576.0
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Voice input (TODO #27 / #29): mic key, voice ball, engine selection
// ---------------------------------------------------------------------------------------------

impl DianmoApp {
    /// A session is running or finishing (the engine may still touch the clipboard).
    fn voice_running(&self) -> bool {
        self.voice.needs_poll() || self.voice.state().is_active()
    }

    fn voice_clip_quiet(&self) -> bool {
        self.voice_running() || self.voice_clip_quiet_until.is_some_and(|t| Instant::now() < t)
    }

    /// The mic key or the voice-mode ball: start or stop voice input.
    fn voice_toggle(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        // The engine types into the target app: nothing of ours may be half-composed there.
        // (The view already released its latched modifiers.)
        let r = if self.ctl.is_composing() { self.input(Action::ClearComposition, view) } else { Response::none() };
        bubble::hide();
        // A repeated failure gets its toast again.
        self.voice_shown = VoiceState::Idle;
        let st = self.voice.toggle();
        log!("voice toggle ({}) -> {}", self.voice.engine().as_setting(), voice_state_name(&st));
        let r = merge(r, self.voice_sync(view, host));
        if !self.voice.state().is_active() {
            // Ended (or failed): availability may have changed (engine closed, crashed).
            self.check_voice_engines(host);
        }
        r
    }

    /// Shows the voice state on the mic keys and the ball, a toast for a new failure, and keeps
    /// the poll timer running while the engine needs it.
    fn voice_sync(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        let st = self.voice.state();
        let listening = matches!(st, VoiceState::Starting | VoiceState::Listening);
        if listening != self.ball_listening {
            self.ball_listening = listening;
            host.set_ball_state(if listening { BallState::Listening } else { BallState::Idle });
        }
        let mut r = Response::none();
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            r.repaint = kv.set_voice_active(listening);
            if let VoiceState::Failed(msg) = &st
                && self.voice_shown != st
            {
                if host.is_visible() {
                    r = merge(r, kv.show_toast(msg, now_ms()));
                } else {
                    // Voice ball: the reason goes next to the ball (once it is out of the edge).
                    host.reveal_ball(VOICE_FAIL_BUBBLE_MS);
                    host.proxy().post(ShowBubble(msg.clone(), VOICE_FAIL_BUBBLE_MS));
                }
            }
        }
        if self.voice.needs_poll() {
            self.arm_voice_timer(host);
        }
        self.voice_clip_quiet_until = Some(Instant::now() + VOICE_CLIP_QUIET);
        if st != self.voice_shown && !matches!(st, VoiceState::Failed(_)) {
            log!("voice: {}", voice_state_name(&st));
        }
        self.voice_shown = st;
        r
    }

    fn arm_voice_timer(&mut self, host: &mut HostControl) {
        if self.voice_timer {
            return;
        }
        let hwnd = HWND(host.proxy().hwnd() as *mut _);
        let id = unsafe { SetTimer(Some(hwnd), VOICE_TIMER_ID, VOICE_POLL_MS, Some(voice_timer_proc)) };
        self.voice_timer = id != 0;
        if id == 0 {
            log!("voice: SetTimer failed; polling stops");
        }
    }

    fn on_voice_tick(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        self.voice_timer = false;
        let was_active = self.voice.state().is_active();
        self.voice.poll();
        let r = self.voice_sync(view, host);
        if was_active && !self.voice.state().is_active() {
            self.check_voice_engines(host);
        }
        r
    }

    fn set_voice_engine(&mut self, e: VoiceEngine, view: &mut dyn View, host: &mut HostControl) -> Response {
        self.voice.set_engine(e); // cancels a running session
        if self.settings.voice_engine != e.as_setting() {
            self.settings.voice_engine = e.as_setting().to_owned();
            self.save();
        }
        log!("voice engine -> {}", e.as_setting());
        self.check_voice_engines(host);
        self.voice_sync(view, host)
    }

    /// Turns voice mode on (and shrinks into the voice ball) or off (back to the keyboard, or the
    /// 电脑键盘 if that is what the keyboard shows; the ball goes back to showing the keyboard).
    fn set_voice_mode(&mut self, on: bool, view: &mut dyn View, host: &mut HostControl) -> Response {
        let mode = if on {
            InputMode::VoiceBall
        } else if view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()).is_some_and(|kv| kv.pc_keyboard()) {
            InputMode::PcKeyboard
        } else {
            InputMode::Keyboard
        };
        self.set_input_mode(mode, view, host)
    }

    /// Voice mode with the ball showing: keep the ball out of the screen edge for a while and say
    /// how it works (once per run, and again each time voice mode is turned on). The hint is
    /// posted so that it is placed after the ball came out.
    fn voice_intro(&mut self, host: &mut HostControl) {
        if self.voice_hint_shown {
            return;
        }
        host.reveal_ball(VOICE_INTRO_MS);
        host.proxy().post(ShowBubble(VOICE_HINT.to_owned(), VOICE_INTRO_MS));
        log!("voice mode hint");
        self.voice_hint_shown = true;
    }

    /// Checks which engines are usable on a short-lived thread (registry reads and a process
    /// snapshot); the result updates the tray menu.
    fn check_voice_engines(&mut self, host: &mut HostControl) {
        let proxy = host.proxy();
        let _ = std::thread::Builder::new().name("dianmo-voice-check".into()).stack_size(256 * 1024).spawn(move || {
            proxy.post(VoiceAvail(VoiceEngine::ALL.map(Voice::why_unavailable)));
        });
    }
}

fn voice_state_name(s: &VoiceState) -> &'static str {
    match s {
        VoiceState::Idle => "idle",
        VoiceState::Starting => "starting",
        VoiceState::Listening => "listening",
        VoiceState::Finishing => "finishing",
        VoiceState::Failed(_) => "failed",
    }
}

impl DianmoApp {
    fn event(&mut self, event: Box<dyn Any + Send>, view: &mut dyn View, host: &mut HostControl) -> Response {
        if let Some(ev) = event.downcast_ref::<FocusEvent>() {
            // A touch in an app: a locked Ctrl / Alt / Win must not stay held down there.
            let touched = matches!(ev, FocusEvent::Editable { by_touch: true, .. } | FocusEvent::NotEditable { by_touch: true });
            let lift = match view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
                Some(kv) if touched => kv.release_locked_mods(),
                _ => Response::none(),
            };
            return merge(lift, self.on_focus(*ev, view, host));
        }
        let event = match event.downcast::<ClipEvent>() {
            Ok(ev) => return self.on_clip_event(*ev, view, host),
            Err(e) => e,
        };
        if event.is::<VoiceTick>() {
            return self.on_voice_tick(view, host);
        }
        if event.is::<VoiceIntro>() {
            if self.voice_mode() && !host.is_visible() {
                self.voice_intro(host);
            }
            return Response::none();
        }
        let event = match event.downcast::<ShowBubble>() {
            Ok(b) => {
                if !host.is_visible() {
                    bubble::show(&b.0, b.1);
                }
                return Response::none();
            }
            Err(e) => e,
        };
        let event = match event.downcast::<VoiceAvail>() {
            Ok(a) => {
                self.voice_unavailable = a.0;
                self.voice_checked = true;
                return Response::none();
            }
            Err(e) => e,
        };
        let event = match self.settings_event(event, view, host) {
            Ok(r) => return r,
            Err(e) => e,
        };
        if let Some(CopyCheck(n)) = event.downcast_ref::<CopyCheck>() {
            // 复制 did not change the clipboard: nothing was selected, so offer selection mode.
            if *n == self.clip_updates && self.clip_watch.is_some() && host.is_visible()
                && let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>())
                    && kv.enter_select_mode() {
                        return Response::repaint();
                    }
            return Response::none();
        }
        #[cfg(feature = "rime")]
        let event = match event.downcast::<RimeReady>() {
            Ok(ready) => {
                let r = self.on_rime_ready(*ready, view);
                // 模糊音: a customization without a matching user build (first start after an
                // update, or changed while 点墨 was not running) is built now.
                self.apply_fuzzy(host);
                self.refresh_user_words(host);
                return r;
            }
            Err(e) => e,
        };
        let event = match self.rime_event(event, view, host) {
            Ok(r) => return r,
            Err(e) => e,
        };
        if let Some(h) = event.downcast_ref::<HideLater>() {
            if h.0 == self.focus_seq && host.is_visible() {
                log!("auto-hide: touched outside a text field");
                host.hide();
            }
        } else if event.is::<ShowAgain>() {
            host.show();
        }
        Response::none()
    }

    fn action(&mut self, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        match action {
            UiAction::Input(a) => self.input(a, view),
            UiAction::WantMoreCandidates { start, count } => {
                let more = self.ctl.engine_mut().candidates(start, count);
                view.set_more_candidates(start, more)
            }
            UiAction::WantT9Spellings => {
                let spellings = self.ctl.engine_mut().t9_spellings();
                view.set_t9_spellings(spellings)
            }
            UiAction::Voice => self.voice_toggle(view, host),
            // The 语音球 tile, or 「退出语音模式」 / 「退出语音球」 while in voice mode.
            UiAction::VoiceBall => {
                let on = !self.voice_mode();
                self.set_voice_mode(on, view, host)
            }
            UiAction::Hide => {
                host.hide();
                Response::none()
            }
            UiAction::ThemeChanged(kind) => {
                // Chosen on the keyboard: an explicit light / dark (no longer 跟随系统).
                let choice = if kind == ThemeKind::Dark { ThemeChoice::Dark } else { ThemeChoice::Light };
                if self.settings.theme != choice {
                    self.settings.theme = choice;
                    self.save();
                    return self.apply_theme(view, host);
                }
                Response::none()
            }
            UiAction::PcKeyboard(on) => {
                let mode = if on { InputMode::PcKeyboard } else { InputMode::Keyboard };
                self.set_input_mode(mode, view, host)
            }
            UiAction::OpenSettings => {
                self.open_settings(None, host);
                Response::none()
            }
            // Only app windows send these (`on_window_action`).
            UiAction::Settings(_) => Response::none(),
            // Handled in `on_action`.
            UiAction::KeyClick(_) => Response::none(),
            UiAction::Paste(text) => self.paste(text, view),
            UiAction::PinClip { id, pinned } => {
                if self.clips.pin(id, pinned) {
                    self.save_clips();
                }
                self.push_clips(view)
            }
            UiAction::DeleteClip(id) => {
                if self.clips.delete(id) {
                    self.save_clips();
                }
                self.push_clips(view)
            }
            UiAction::ClearClips => {
                self.clips.clear_unpinned();
                self.push_clips(view)
            }
            UiAction::CheckCopied => {
                let n = self.clip_updates;
                let proxy = host.proxy();
                let _ = std::thread::Builder::new().name("dianmo-copycheck".into()).stack_size(64 * 1024).spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(COPY_CHECK_MS));
                    proxy.post(CopyCheck(n));
                });
                Response::none()
            }
        }
    }
}

fn keymap_wanted() -> bool {
    static WANTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WANTED.get_or_init(|| std::env::var_os("DIANMO_KEYMAP").is_some())
}

/// Test hook: with `DIANMO_KEYMAP=<file>`, writes `name x y` (DIPs, window coordinates) for the
/// visible keys each time the keyboard is shown or changes, so GUI tests on the Surface can tap
/// real keys.
fn dump_keymap(view: &mut dyn View) {
    let Some(path) = std::env::var_os("DIANMO_KEYMAP") else { return };
    let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) else { return };
    let mut out = String::new();
    let names = [
        "shift", "backspace", "enter", "space", "layout", "toggle", "expand", "voice", "hide", "123", "符号", "，", ",", "。",
        "ctrl", "alt", "win", "fn", "caps", "esc", "tab", "del", "left", "right", "up", "down", "home", "end", "undo",
        "redo", "selectall", "copy", "paste", "cut", "delword", "clear", "pc", "symbols", "numbers", "pcmode", "pcback",
        "prtsc", "select", "clipboard", "clipclose", "clearclips", "back", "sel_left", "sel_right", "sel_wordleft",
        "sel_wordright", "sel_up", "sel_down", "sel_home", "sel_end", "sel_copy", "sel_cut", "sel_paste", "sel_delete",
        "sel_done", "voiceball", "exitvoice", "settings", "t9_1", "t9_2", "t9_3", "t9_4", "t9_5", "t9_6", "t9_7", "t9_8", "t9_9", "clip0", "clip1", "clip2", "clip3", "F1", "F4", "F5", "`", "-", "=", "[", "]", "\\", ";", "'", "/",
        ".",
    ];
    let letters: Vec<String> = ('a'..='z').chain('0'..='9').map(String::from).collect();
    for name in names.iter().copied().chain(letters.iter().map(String::as_str)) {
        if let Some((x, y)) = kv.key_center(name) {
            out.push_str(&format!("{name} {x:.1} {y:.1}\n"));
        }
    }
    let _ = std::fs::write(path, out);
}

impl Drop for DianmoApp {
    fn drop(&mut self) {
        self.sound = None;
        if self.voice.state().is_active() {
            // Don't leave the engine listening after 点墨 is gone.
            self.voice.cancel();
        }
        self.focus = None;
        self.clip_watch = None;
        #[cfg(feature = "rime")]
        {
            // Close the librime session, then let librime flush the user dictionary.
            *self.ctl.engine_mut() = AnyEngine::Basic(BasicEngine::new(self.settings.schema));
            self.pending_rime = None;
            dianmo_rime::shutdown();
        }
        if restore_system_keyboard() {
            self.settings.saved_tabtip = None;
        }
        self.save();
        log!("exit");
    }
}

// ---------------------------------------------------------------------------------------------
// Auto show/hide (dianmo-win `focus`) and the tray menu
// ---------------------------------------------------------------------------------------------

/// Delay before hiding after a touch outside any text field: focus often passes through a
/// non-editable element on its way to the next field.
const HIDE_DELAY_MS: u64 = 350;
/// After the user shows the keyboard by hand (tray, edge handle, second launch), ignore
/// "not editable" focus changes for this long: tapping the tray icon focuses the taskbar.
const MANUAL_SHOW_GRACE_MS: u64 = 1500;

mod tray_id {
    pub const PINYIN: u32 = 1;
    pub const SHUANGPIN: u32 = 2;
    pub const T9: u32 = 3;
    pub const ENGLISH: u32 = 5;
    pub const TOGGLE: u32 = 10;
    pub const ABOUT: u32 = 30;
    pub const SETTINGS: u32 = 31;
    pub const QUIT: u32 = 32;
    pub const VOICE_ENGINE_BASE: u32 = 40; // + index into VoiceEngine::ALL
    pub const MODE_BASE: u32 = 60; // + index into InputMode::ALL
}

/// Posted to ourselves to show the keyboard again after a height change (re-docks the window).
struct ShowAgain;

/// Posted to ourselves: show this hint next to the ball for this many ms (after the ball came out
/// of the screen edge).
struct ShowBubble(String, u32);

impl DianmoApp {
    /// The tray menu (PRODUCT.md P8): 显示/隐藏键盘、布局 ▸、语音引擎 ▸、输入模式 ▸、设置…、关于点墨、
    /// 退出. Everything else is in the settings window.
    fn tray_items(&self, visible: bool) -> Vec<TrayItem> {
        let cmd = |id: u32, label: &str, checked: bool| TrayItem::Command { id, label: label.to_owned(), checked };
        let layout = prefs::layout_choice(&self.settings);
        let keyboard = self.settings.input_mode != InputMode::PcKeyboard;
        let layouts = [
            (tray_id::PINYIN, "全拼", LayoutChoice::Pinyin),
            (tray_id::SHUANGPIN, self.settings.shuangpin.name(), LayoutChoice::Shuangpin),
            (tray_id::T9, "九宫格", LayoutChoice::T9),
            (tray_id::ENGLISH, "English", LayoutChoice::English),
        ]
        .iter()
        .map(|&(id, label, l)| cmd(id, label, keyboard && layout == l))
        .collect();
        let voice_engines = VoiceEngine::ALL
            .iter()
            .enumerate()
            .map(|(i, &e)| {
                let name = e.label();
                let label = match &self.voice_unavailable[i] {
                    None => name.to_owned(),
                    Some(why) if ["没有安装", "找不到", "没有配置"].iter().any(|w| why.contains(w)) => format!("{name}（未安装）"),
                    Some(_) => format!("{name}（未就绪）"),
                };
                cmd(tray_id::VOICE_ENGINE_BASE + i as u32, &label, self.voice.engine() == e)
            })
            .collect();
        let modes = InputMode::ALL
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                let label = match m {
                    InputMode::Keyboard => "键盘",
                    InputMode::VoiceBall => "语音球（点一下说话）",
                    InputMode::PcKeyboard => "电脑键盘（按键直通）",
                };
                cmd(tray_id::MODE_BASE + i as u32, label, self.settings.input_mode == m)
            })
            .collect();
        vec![
            cmd(tray_id::TOGGLE, if visible { "隐藏键盘" } else { "显示键盘" }, false),
            TrayItem::Separator,
            TrayItem::Submenu { label: "布局".to_owned(), items: layouts },
            TrayItem::Submenu { label: "语音引擎".to_owned(), items: voice_engines },
            TrayItem::Submenu { label: "输入模式".to_owned(), items: modes },
            TrayItem::Separator,
            cmd(tray_id::SETTINGS, "设置…", false),
            match &self.update {
                UpdateState::Available { version, .. } => cmd(tray_id::ABOUT, &format!("关于点墨（新版本 {version}）"), false),
                _ => cmd(tray_id::ABOUT, "关于点墨", false),
            },
            TrayItem::Separator,
            cmd(tray_id::QUIT, "退出点墨", false),
        ]
    }

    /// Pushes the tray menu to the host if anything it shows changed.
    fn refresh_tray(&mut self, host: &mut HostControl) {
        if host.appbar_enabled() != self.settings.appbar {
            self.settings.appbar = host.appbar_enabled();
            self.save();
        }
        let items = self.tray_items(host.is_visible());
        if items != self.tray_shown {
            host.set_tray_menu(items.clone());
            self.tray_shown = items;
        }
    }

    fn set_focus_watch(&mut self, on: bool, host: &mut HostControl) {
        if !on {
            self.focus = None;
            return;
        }
        if self.focus.is_some() {
            return;
        }
        match start_focus_watcher(host.proxy()) {
            Ok(w) => self.focus = Some(w),
            Err(e) => log!("focus watcher failed to start (no auto show/hide): {e}"),
        }
    }

    fn hide_later(&mut self, host: &mut HostControl) {
        let seq = self.focus_seq;
        let proxy = host.proxy();
        let spawned = std::thread::Builder::new().name("dianmo-hide".into()).stack_size(64 * 1024).spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(HIDE_DELAY_MS));
            proxy.post(HideLater(seq));
        });
        if spawned.is_err() {
            host.hide();
        }
    }

    fn on_focus(&mut self, ev: FocusEvent, view: &mut dyn View, host: &mut HostControl) -> Response {
        // No clipboard history while a password field has focus (needs the focus watcher, i.e.
        // auto show on).
        self.password_focus = matches!(ev, FocusEvent::Editable { kind: FieldKind::Password, .. });
        if !self.settings.auto_show {
            return Response::none();
        }
        match ev {
            FocusEvent::Editable { kind, by_touch } => {
                if !by_touch && !host.is_visible() {
                    return Response::none();
                }
                self.focus_seq += 1;
                let mut repaint = false;
                if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
                    if kind == FieldKind::Number {
                        repaint = kv.show_numbers();
                        self.numbers_for_field = true;
                    } else if self.numbers_for_field {
                        repaint = kv.show_letters();
                        self.numbers_for_field = false;
                    }
                }
                // Voice mode: only the ball; the keyboard comes from a long press or the tray.
                if by_touch && !host.is_visible() && !self.voice_mode() {
                    self.showing_for_focus = true;
                    host.show();
                }
                Response { repaint, ..Response::none() }
            }
            FocusEvent::NotEditable { by_touch } => {
                let grace = self.manual_show_at.is_some_and(|t| t.elapsed().as_millis() < MANUAL_SHOW_GRACE_MS as u128);
                if by_touch && host.is_visible() && !grace {
                    self.focus_seq += 1;
                    self.hide_later(host);
                }
                Response::none()
            }
        }
    }

    fn tray_command(&mut self, id: u32, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = match id {
            tray_id::TOGGLE => {
                host.toggle();
                Response::none()
            }
            tray_id::PINYIN => self.set_layout(LayoutChoice::Pinyin, view, host),
            tray_id::SHUANGPIN => self.set_layout(LayoutChoice::Shuangpin, view, host),
            tray_id::T9 => self.set_layout(LayoutChoice::T9, view, host),
            tray_id::ENGLISH => self.set_layout(LayoutChoice::English, view, host),
            id if (tray_id::MODE_BASE..tray_id::MODE_BASE + InputMode::ALL.len() as u32).contains(&id) => {
                let mode = InputMode::ALL[(id - tray_id::MODE_BASE) as usize];
                self.set_input_mode(mode, view, host)
            }
            id if (tray_id::VOICE_ENGINE_BASE..tray_id::VOICE_ENGINE_BASE + ENGINES as u32).contains(&id) => {
                let e = VoiceEngine::ALL[(id - tray_id::VOICE_ENGINE_BASE) as usize];
                self.set_voice_engine(e, view, host)
            }
            tray_id::SETTINGS => {
                self.open_settings(None, host);
                Response::none()
            }
            tray_id::ABOUT => {
                // The new version has been seen: the tray icon's red dot goes.
                host.set_tray_badge(false);
                self.open_settings(Some(Page::About), host);
                Response::none()
            }
            tray_id::QUIT => {
                log!("quit from the tray menu");
                host.quit();
                Response::none()
            }
            _ => Response::none(),
        };
        self.refresh_tray(host);
        r
    }
}

// ---------------------------------------------------------------------------------------------
// System touch keyboard auto-invoke (restored on exit, panic, and after a crash on next start)
// ---------------------------------------------------------------------------------------------

static ORIGINAL_TABTIP: Mutex<Option<SystemKeyboardSettings>> = Mutex::new(None);

/// Turns off the system touch keyboard's auto-invoke. The original values are written to the
/// settings file *before* the registry changes; if a previous run crashed, the values saved then
/// are the originals (the registry still holds ours).
pub fn take_over_system_keyboard(settings: &mut Settings, path: &std::path::Path) {
    let original = match settings.saved_tabtip {
        Some((a, b)) => {
            log!("restoring after an unclean exit: system keyboard settings were {a:?}/{b:?}");
            SystemKeyboardSettings { desktop_mode_auto_invoke: a, tap_invoke: b }
        }
        None => match SystemKeyboardSettings::read() {
            Ok(s) => s,
            Err(e) => {
                log!("reading system keyboard settings failed, leaving them alone: {e}");
                return;
            }
        },
    };
    settings.saved_tabtip = Some((original.desktop_mode_auto_invoke, original.tap_invoke));
    if let Err(e) = settings.save(path) {
        log!("can't save settings, leaving the system keyboard alone: {e}");
        return;
    }
    let off = SystemKeyboardSettings { desktop_mode_auto_invoke: Some(0), tap_invoke: Some(0) };
    match off.apply() {
        Ok(()) => {
            if let Ok(mut g) = ORIGINAL_TABTIP.lock() {
                *g = Some(original);
            }
        }
        Err(e) => {
            log!("disabling the system keyboard auto-invoke failed: {e}");
            let _ = original.apply();
        }
    }
}

/// Puts the system keyboard settings back (idempotent; also called from the panic hook).
/// False only if there was something to restore and writing it failed.
pub fn restore_system_keyboard() -> bool {
    let original = match ORIGINAL_TABTIP.try_lock() {
        Ok(mut g) => g.take(),
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().take(),
        Err(std::sync::TryLockError::WouldBlock) => None,
    };
    match original.map(|o| o.apply()) {
        Some(Err(e)) => {
            log!("restoring system keyboard settings failed: {e}");
            false
        }
        _ => true,
    }
}
