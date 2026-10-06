//! The application behind the keyboard window: input controller + settings + system glue.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use dianmo_core::{Action, Engine, InputController, Schema};
use dianmo_ui::{InputState, KeyboardView, Response, ThemeKind, UiAction, View};
use dianmo_win::tabtip::SystemKeyboardSettings;
use dianmo_win::{
    App, FieldKind, FocusEvent, FocusWatcher, HostControl, SendInputSink, TrayItem, start_focus_watcher,
    start_voice_typing,
};

#[cfg(feature = "rime")]
use crate::basic::BasicEngine;
use crate::engine::AnyEngine;
use crate::log;
use crate::platform;
use crate::settings::Settings;

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
}

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
        let mut ctl = InputController::new(engine, SendInputSink::new());
        ctl.handle(Action::SetSchema(settings.schema));
        if !settings.chinese {
            ctl.handle(Action::ToggleChinese);
        }
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
        self.ctl.handle(action);
        let mut r = view.set_input_state(self.input_state());
        self.sync_mode();
        let swapped = self.swap_in_rime(view);
        r.repaint |= swapped.repaint;
        r.actions.extend(swapped.actions);
        r
    }

    fn set_theme(&mut self, kind: ThemeKind, view: &mut dyn View) -> Response {
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            kv.set_theme(kind);
        }
        if self.settings.theme != kind {
            self.settings.theme = kind;
            self.save();
        }
        Response::repaint()
    }

    fn set_schema(&mut self, schema: Schema, view: &mut dyn View) -> Response {
        if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
            kv.show_letters();
        }
        let mut r = Response::repaint();
        if self.ctl.schema() != schema {
            r.actions.extend(self.input(Action::SetSchema(schema), view).actions);
        }
        if !self.ctl.is_chinese() {
            r.actions.extend(self.input(Action::ToggleChinese, view).actions);
        }
        r
    }

    fn set_autostart(&mut self, on: bool) {
        match platform::set_autostart(on) {
            Ok(()) => {
                self.settings.autostart = on;
                self.save();
            }
            Err(e) => log!("autostart {on}: {e}"),
        }
    }

    fn engine_label(&self) -> String {
        match &self.status {
            EngineStatus::Rime => "词库：雾凇拼音（Rime）".to_owned(),
            EngineStatus::Loading => "词库加载中".to_owned(),
            EngineStatus::Off(p) => format!("词库未加载（{p}）"),
        }
    }

    #[cfg(feature = "rime")]
    fn start_rime(&mut self, host: &mut HostControl) {
        let Some(opts) = self.rime_opts.take() else { return };
        let schema = self.ctl.schema();
        let proxy = host.proxy();
        let spawned = std::thread::Builder::new().name("dianmo-rime-start".into()).spawn(move || {
            // Let the window come up first: loading rime.dll holds the loader lock, which would
            // stall the UI thread's Direct2D/DirectWrite DLL loads.
            std::thread::sleep(std::time::Duration::from_millis(RIME_START_DELAY_MS));
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
        let Some(engine) = self.pending_rime.take() else { return Response::none() };
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
    fn on_start(&mut self, _view: &mut dyn View, host: &mut HostControl) -> Response {
        platform::set_proxy(host.proxy());
        #[cfg(feature = "rime")]
        self.start_rime(host);
        self.set_focus_watch(self.settings.auto_show, host);
        self.refresh_tray(host);
        Response::none()
    }

    fn on_action(&mut self, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = self.action(action, view, host);
        self.refresh_tray(host);
        r
    }

    fn on_event(&mut self, event: Box<dyn Any + Send>, view: &mut dyn View, host: &mut HostControl) -> Response {
        let r = self.event(event, view, host);
        self.refresh_tray(host);
        r
    }

    fn on_visibility_changed(&mut self, visible: bool, view: &mut dyn View, host: &mut HostControl) -> Response {
        if visible {
            dump_keymap(view);
            self.manual_show_at = if std::mem::take(&mut self.showing_for_focus) { None } else { Some(Instant::now()) };
        } else {
            // A manual hide cancels a pending auto-hide; nothing else to do.
            self.focus_seq += 1;
        }
        self.refresh_tray(host);
        if !visible && self.ctl.is_composing() {
            // The target is probably gone; don't leave a stale composition behind.
            return self.input(Action::ClearComposition, view);
        }
        Response::none()
    }

    fn on_tray_command(&mut self, id: u32, view: &mut dyn View, host: &mut HostControl) -> Response {
        self.tray_command(id, view, host)
    }
}

impl DianmoApp {
    fn event(&mut self, event: Box<dyn Any + Send>, view: &mut dyn View, host: &mut HostControl) -> Response {
        if let Some(ev) = event.downcast_ref::<FocusEvent>() {
            return self.on_focus(*ev, view, host);
        }
        #[cfg(feature = "rime")]
        let event = match event.downcast::<RimeReady>() {
            Ok(ready) => return self.on_rime_ready(*ready, view),
            Err(e) => e,
        };
        if let Some(h) = event.downcast_ref::<HideLater>() {
            if h.0 == self.focus_seq && host.is_visible() {
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
            UiAction::Voice => {
                if !start_voice_typing() {
                    log!("voice typing (Win+H) could not be sent");
                }
                Response::none()
            }
            UiAction::Hide => {
                host.hide();
                Response::none()
            }
            UiAction::ThemeChanged(kind) => {
                if self.settings.theme != kind {
                    self.settings.theme = kind;
                    self.save();
                }
                Response::none()
            }
        }
    }
}

/// Test hook: with `DIANMO_KEYMAP=<file>`, writes `name x y` (DIPs, window coordinates) for the
/// visible keys each time the keyboard is shown, so GUI tests on the Surface can tap real keys.
fn dump_keymap(view: &mut dyn View) {
    let Some(path) = std::env::var_os("DIANMO_KEYMAP") else { return };
    let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) else { return };
    let mut out = String::new();
    let names = ["shift", "backspace", "enter", "space", "layout", "toggle", "expand", "voice", "hide", "123", "符号", "，", ",", "。"];
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
        self.focus = None;
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
    pub const DARK: u32 = 10;
    pub const AUTO_SHOW: u32 = 11;
    pub const AUTOSTART: u32 = 12;
    pub const HEIGHT_BASE: u32 = 20; // + index into HEIGHTS
    pub const ABOUT: u32 = 30;
}

const HEIGHTS: [(f32, &str); 4] = [(0.85, "矮"), (1.0, "标准"), (1.15, "高"), (1.3, "更高")];

/// Posted to ourselves to show the keyboard again after a height change (re-docks the window).
struct ShowAgain;

impl DianmoApp {
    fn tray_items(&self) -> Vec<TrayItem> {
        let cmd = |id: u32, label: &str, checked: bool| TrayItem::Command { id, label: label.to_owned(), checked };
        let schema = self.ctl.schema();
        let heights = HEIGHTS
            .iter()
            .enumerate()
            .map(|(i, (h, label))| cmd(tray_id::HEIGHT_BASE + i as u32, label, (self.settings.height - h).abs() < 0.01))
            .collect();
        vec![
            cmd(tray_id::PINYIN, "全拼", schema == Schema::Pinyin),
            cmd(tray_id::SHUANGPIN, "小鹤双拼", schema == Schema::Shuangpin),
            cmd(tray_id::T9, "九宫格", schema == Schema::T9),
            TrayItem::Separator,
            cmd(tray_id::DARK, "深色主题", self.settings.theme == ThemeKind::Dark),
            TrayItem::Submenu { label: "键盘高度".to_owned(), items: heights },
            cmd(tray_id::AUTO_SHOW, "点输入框时自动弹出", self.settings.auto_show),
            cmd(tray_id::AUTOSTART, "开机自动启动", self.settings.autostart),
            TrayItem::Separator,
            cmd(tray_id::ABOUT, &format!("关于点墨 {} · {}", env!("CARGO_PKG_VERSION"), self.engine_label()), false),
            TrayItem::Separator,
        ]
    }

    /// Pushes the tray menu to the host if anything it shows changed.
    fn refresh_tray(&mut self, host: &mut HostControl) {
        if host.appbar_enabled() != self.settings.appbar {
            self.settings.appbar = host.appbar_enabled();
            self.save();
        }
        let items = self.tray_items();
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
                if by_touch && !host.is_visible() {
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
            tray_id::PINYIN => self.set_schema(Schema::Pinyin, view),
            tray_id::SHUANGPIN => self.set_schema(Schema::Shuangpin, view),
            tray_id::T9 => self.set_schema(Schema::T9, view),
            tray_id::DARK => {
                let next = if self.settings.theme == ThemeKind::Dark { ThemeKind::Light } else { ThemeKind::Dark };
                self.set_theme(next, view)
            }
            tray_id::AUTO_SHOW => {
                self.settings.auto_show = !self.settings.auto_show;
                self.save();
                self.set_focus_watch(self.settings.auto_show, host);
                Response::none()
            }
            tray_id::AUTOSTART => {
                self.set_autostart(!self.settings.autostart);
                Response::none()
            }
            id if (tray_id::HEIGHT_BASE..tray_id::HEIGHT_BASE + HEIGHTS.len() as u32).contains(&id) => {
                let h = HEIGHTS[(id - tray_id::HEIGHT_BASE) as usize].0;
                if let Some(kv) = view.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>()) {
                    kv.set_height_scale(h);
                }
                self.settings.height = h;
                self.save();
                if host.is_visible() {
                    // The host docks the window when it is shown: hide and show again to re-dock.
                    host.hide();
                    host.proxy().post(ShowAgain);
                }
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
