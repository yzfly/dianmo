//! The settings window, the about page and the first-run onboarding on the app side (TODO #33,
//! PRODUCT.md P3 / P4 / P5): opening them (single instance), keeping their model in sync with
//! `settings.ini` and the system, and carrying out what they ask.
//!
//! The windows' views send `UiAction::Settings` to [`DianmoApp::window_action`], which posts it
//! to ourselves ([`SettingsCmd`]) so that it is handled in `on_event` with the keyboard view at
//! hand (most changes apply to the keyboard at once). After every callback
//! [`DianmoApp::sync_windows`] sends the open windows the current model if it changed — also
//! when the change came from the tray or the keyboard.

use std::any::Any;
use std::path::PathBuf;

use dianmo_ui::settings::{Level, Page, SettingsAction, SettingsModel, Side, Status, VoiceEngineChoice, VoiceEngines};
use dianmo_ui::{KeyboardView, OnboardingView, Response, SettingsView, ThemeKind, View};
use dianmo_win::{BallEdge, BallPos, HostControl, WindowId, WindowOptions, now_ms};

use super::{DianmoApp, EngineStatus, ShowAgain, ball_wanted, disabled, merge};
use crate::sound::KeySound;
use dianmo_ui::settings::ShuangpinScheme;
use crate::prefs::{self, SysState};
use crate::update::{self, CheckOutcome, UpdateEvent};
use crate::voice::VoiceEngine;
use crate::{diag, elevate, log, platform, shell};
use dianmo_ui::settings::UpdateState;

/// A settings action from window `.0`, handled in `on_event` (see the module docs).
struct SettingsCmd(WindowId, SettingsAction);

/// 管理员窗口支持, checked on a background thread.
struct AdminStatus(Status);

/// The scheduled task was (re-)registered, or why not.
struct TaskFixed(Result<(), String>);

/// The file picker for 豆包语音's program closed.
struct DoubaoPicked(Option<PathBuf>);

/// 导出诊断包 finished (the zip, or why not).
struct DiagExported(Result<PathBuf, String>);

pub(super) const SETTINGS_TITLE: &str = "点墨设置";
const ONBOARDING_TITLE: &str = "欢迎使用点墨";

fn settings_view(v: &mut dyn View) -> Option<&mut SettingsView> {
    v.as_any_mut().and_then(|a| a.downcast_mut::<SettingsView>())
}

fn onboarding_view(v: &mut dyn View) -> Option<&mut OnboardingView> {
    v.as_any_mut().and_then(|a| a.downcast_mut::<OnboardingView>())
}

fn keyboard_view(v: &mut dyn View) -> Option<&mut KeyboardView> {
    v.as_any_mut().and_then(|a| a.downcast_mut::<KeyboardView>())
}

impl DianmoApp {
    // -----------------------------------------------------------------------------------------
    // Model
    // -----------------------------------------------------------------------------------------

    fn dictionary_status(&self) -> Status {
        match &self.status {
            EngineStatus::Rime => Status::ok("雾凇拼音词库已加载"),
            EngineStatus::Loading => Status::new(Level::Unknown, "正在加载词库…"),
            EngineStatus::Off(why) => Status::error(format!("词库没有加载（{why}），现在只能输入字母")),
        }
    }

    fn engines(&self) -> VoiceEngines {
        let status = |c: VoiceEngineChoice| {
            let check = self.voice_checked.then(|| {
                VoiceEngine::from_setting(c.key())
                    .and_then(|e| VoiceEngine::ALL.iter().position(|&x| x == e))
                    .and_then(|i| self.voice_unavailable[i].as_deref())
            });
            prefs::engine_status(c, check)
        };
        VoiceEngines {
            wetype: status(VoiceEngineChoice::WeType),
            doubao_ime: status(VoiceEngineChoice::DoubaoIme),
            doubao: status(VoiceEngineChoice::Doubao),
            system: status(VoiceEngineChoice::System),
        }
    }

    /// What the settings window, the about page and onboarding show now.
    pub(crate) fn model(&self) -> SettingsModel {
        let sys = SysState {
            admin_task: self.admin_task.clone(),
            engines: self.engines(),
            dictionary: self.dictionary_status(),
            clip_count: self.clips.items().len(),
            pinned_clips: self.clips.items().iter().filter(|c| c.pinned).cloned().collect(),
            update: self.update.clone(),
            fuzzy_supported: matches!(self.status, EngineStatus::Rime),
            fuzzy_status: self.fuzzy_status.clone(),
            user_words: self.user_words,
            user_dict_status: self.user_dict_status.clone(),
        };
        prefs::model(&self.settings, &sys)
    }

    /// Sends the open windows the current model if it changed since the last time.
    pub(super) fn sync_windows(&mut self, host: &mut HostControl) {
        let open: Vec<WindowId> = [self.settings_win, self.onboarding_win].into_iter().flatten().collect();
        if open.is_empty() {
            self.shown_model = None;
            return;
        }
        let model = self.model();
        if self.shown_model.as_ref() == Some(&model) {
            return;
        }
        for id in open {
            let m = model.clone();
            host.update_window(id, move |v| {
                let changed = if settings_view_check(v) {
                    settings_view(v).is_some_and(|s| s.set_model(m))
                } else {
                    onboarding_view(v).is_some_and(|o| o.set_model(m))
                };
                repaint_if(changed)
            });
        }
        self.shown_model = Some(model);
    }

    /// A short message in the settings window (if it is open).
    pub(super) fn toast(&self, host: &mut HostControl, text: impl Into<String>) {
        let text = text.into();
        log!("settings: {text}");
        if let Some(id) = self.settings_win {
            host.update_window(id, move |v| settings_view(v).map_or_else(Response::none, |s| s.show_toast(&text, now_ms())));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Opening and closing
    // -----------------------------------------------------------------------------------------

    /// Opens the settings window (at `page`), or brings the open one to the front.
    pub(crate) fn open_settings(&mut self, page: Option<Page>, host: &mut HostControl) {
        if let Some(id) = self.settings_win.filter(|&id| host.window_open(id)) {
            host.focus_window(id);
            if let Some(p) = page {
                host.update_window(id, move |v| {
                    if settings_view(v).is_some_and(|s| s.set_page(p)) { Response::repaint() } else { Response::none() }
                });
                self.page_shown(p, host);
            }
            return;
        }
        let mut view = SettingsView::new(self.model(), self.theme_kind());
        // Test hook (GUI automation on the Surface): the visible elements, after every paint.
        view.set_element_dump(std::env::var_os("DIANMO_SETTINGSMAP").map(PathBuf::from));
        if let Some(p) = page {
            view.set_page(p);
        }
        let opts = WindowOptions {
            title: SETTINGS_TITLE.to_owned(),
            width: 960.0,
            height: 680.0,
            min_width: 420.0,
            min_height: 420.0,
            dark: prefs::window_dark(self.settings.theme),
            ..WindowOptions::default()
        };
        crate::memdiag::report("before settings");
        let id = host.open_window(Box::new(view), opts);
        log!("settings window opened ({page:?})");
        self.settings_win = Some(id);
        self.shown_model = None;
        self.page_shown(page.unwrap_or_default(), host);
        if disabled("settings-checks") {
            return;
        }
        // Fresh system state for the window.
        self.check_admin_task(host);
        self.check_voice_engines(host);
        self.refresh_user_words(host);
    }

    /// Opens the first-run onboarding (or brings it to the front).
    pub(crate) fn open_onboarding(&mut self, host: &mut HostControl) {
        if let Some(id) = self.onboarding_win.filter(|&id| host.window_open(id)) {
            host.focus_window(id);
            return;
        }
        let opts = WindowOptions {
            title: ONBOARDING_TITLE.to_owned(),
            width: 760.0,
            height: 560.0,
            resizable: false,
            dark: prefs::window_dark(self.settings.theme),
            ..WindowOptions::default()
        };
        let id = host.open_window(Box::new(OnboardingView::new(self.model(), self.theme_kind())), opts);
        log!("onboarding opened");
        self.onboarding_win = Some(id);
        self.shown_model = None;
        self.check_voice_engines(host);
    }

    pub(super) fn window_action(&mut self, id: WindowId, action: dianmo_ui::UiAction, _view: &mut dyn View, host: &mut HostControl) -> Response {
        match action {
            dianmo_ui::UiAction::Settings(SettingsAction::Close) => host.close_window(id),
            // Handled with the keyboard view at hand.
            dianmo_ui::UiAction::Settings(a) => {
                host.proxy().post(SettingsCmd(id, a));
            }
            _ => {}
        }
        Response::none()
    }

    pub(super) fn window_closed(&mut self, id: WindowId) {
        if self.settings_win == Some(id) {
            self.settings_win = None;
            self.update_badge.set_about_shown(false);
            log!("settings window closed");
            if crate::memdiag::enabled() {
                crate::memdiag::report("settings closed");
                std::thread::spawn(|| {
                    for s in [3, 10] {
                        std::thread::sleep(std::time::Duration::from_secs(if s == 3 { 3 } else { 7 }));
                        crate::memdiag::report(&format!("settings closed +{s}s"));
                    }
                });
            }
        }
        if self.onboarding_win == Some(id) {
            self.onboarding_win = None;
            // Closing it counts as skipping: it shows only once.
            if !self.settings.onboarded {
                self.settings.onboarded = true;
                self.save();
            }
            log!("onboarding closed");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Events and actions
    // -----------------------------------------------------------------------------------------

    /// Settings-related events posted to ourselves; anything else is handed back.
    pub(super) fn settings_event(
        &mut self,
        event: Box<dyn Any + Send>,
        view: &mut dyn View,
        host: &mut HostControl,
    ) -> Result<Response, Box<dyn Any + Send>> {
        let event = match event.downcast::<SettingsCmd>() {
            Ok(c) => {
                let SettingsCmd(id, a) = *c;
                let r = self.settings_command(id, a, view, host);
                // The view already changed its own copy; send ours even if it looks unchanged
                // (e.g. a switch that could not be applied flips back).
                self.shown_model = None;
                return Ok(r);
            }
            Err(e) => e,
        };
        if event.is::<platform::OpenSettings>() {
            self.open_settings(None, host);
            return Ok(Response::none());
        }
        let event = match event.downcast::<AdminStatus>() {
            Ok(s) => {
                self.admin_task = s.0;
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        let event = match event.downcast::<TaskFixed>() {
            Ok(r) => {
                match r.0 {
                    Ok(()) => self.toast(host, "已开启管理员窗口支持"),
                    Err(e) => self.toast(host, format!("没有完成：{e}")),
                }
                self.check_admin_task(host);
                self.check_autostart();
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        let event = match event.downcast::<DoubaoPicked>() {
            Ok(p) => {
                if let Some(path) = p.0 {
                    self.settings.voice_doubao_exe = path.to_string_lossy().into_owned();
                    self.save();
                    self.voice.set_doubao_exe(Some(path));
                    self.check_voice_engines(host);
                }
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        let event = match event.downcast::<DiagExported>() {
            Ok(r) => {
                match r.0 {
                    Ok(path) => {
                        self.toast(host, "诊断包已保存到桌面，反馈问题时可以附上它");
                        diag::reveal(&path);
                    }
                    Err(e) => self.toast(host, e),
                }
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        match event.downcast::<UpdateEvent>() {
            Ok(u) => {
                self.on_update_event(*u, host);
                Ok(Response::none())
            }
            Err(e) => Err(e),
        }
    }

    /// Carries out one settings action. `view` is the keyboard's; the returned response is for it.
    fn settings_command(&mut self, from: WindowId, a: SettingsAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        use SettingsAction as A;
        log!("settings: {a:?}");
        let stored = prefs::apply(&mut self.settings, &a);
        if stored {
            self.save();
        }
        match a {
            A::SetAutostart(on) => {
                if let Err(e) = platform::set_autostart(on) {
                    log!("autostart {on}: {e}");
                    self.toast(host, if elevate::is_elevated() { format!("没有设置成功：{}", e.message()) } else { "需要管理员权限：请先在上面开启「管理员窗口支持」".to_owned() });
                }
                self.check_autostart();
                Response::none()
            }
            A::SetAutoShow(on) => {
                self.set_focus_watch(on && !disabled("focus"), host);
                Response::none()
            }
            A::SetInputMode(mode) => self.set_input_mode(mode, view, host),
            A::SetTheme(_) => self.apply_theme(view, host),
            A::SetReserveSpace(on) => {
                host.set_appbar(on);
                Response::none()
            }
            A::FixAdminTask => {
                self.fix_admin_task(host);
                Response::none()
            }
            A::ResetDefaults => {
                self.settings.reset_preferences();
                self.save();
                let r = self.apply_all(view, host);
                self.toast(host, "已恢复默认设置");
                r
            }
            A::SetLayout(l) => self.set_layout(l, view, host),
            A::SetKeyboardHeight(h) => self.apply_height(h, view, host),
            A::SetEditArea(on) => repaint_if(keyboard_view(view).is_some_and(|kv| kv.set_edit_area(on))),
            A::SetKeyPopup(on) => {
                if let Some(kv) = keyboard_view(view) {
                    kv.set_key_popup(on);
                }
                Response::none()
            }
            A::SetLongPress(lp) => {
                if let Some(kv) = keyboard_view(view) {
                    kv.set_long_press_ms(lp.millis());
                }
                Response::none()
            }
            A::SetKeySound(_) | A::SetKeySoundVolume(_) | A::SetKeySoundStyle(_) => {
                self.apply_key_sound(view);
                // Let the user hear what they picked.
                if let Some(s) = &self.sound {
                    s.play(dianmo_ui::KeyClick::Char);
                }
                Response::none()
            }
            A::SetBall(_) => {
                host.set_ball_enabled(ball_wanted(&self.settings));
                Response::none()
            }
            A::SetBallSide(side) => {
                let edge = if side == Side::Right { BallEdge::Right } else { BallEdge::Left };
                let y_frac = self.settings.ball.map_or(prefs::DEFAULT_BALL_Y, |(_, y)| y);
                host.set_ball_pos(BallPos { edge, y_frac });
                Response::none()
            }
            A::SetCandidateSize(size) => repaint_if(keyboard_view(view).is_some_and(|kv| kv.set_candidate_size(size))),
            A::SetFullWidthPunct(on) => {
                self.ctl.set_full_width_punct(on);
                repaint_if(keyboard_view(view).is_some_and(|kv| kv.set_full_width_punct(on)))
            }
            A::SetSpaceCommitsFirst(on) => {
                self.ctl.set_space_commits_first(on);
                Response::none()
            }
            A::SetFuzzy(..) => {
                self.apply_fuzzy(host);
                Response::none()
            }
            A::SetShuangpin(scheme) => self.apply_shuangpin(scheme, view),
            A::ExportUserDict => {
                self.pick_dict_file(super::rime_ui::DictFile::Export, host);
                Response::none()
            }
            A::ImportUserDict => {
                self.pick_dict_file(super::rime_ui::DictFile::Import, host);
                Response::none()
            }
            A::ClearUserDict => {
                self.clear_user_dict(host);
                Response::none()
            }
            A::SetVoiceEngine(c) => match VoiceEngine::from_setting(c.key()) {
                Some(e) => self.set_voice_engine(e, view, host),
                None => Response::none(),
            },
            A::PickDoubaoExe => {
                let proxy = host.proxy();
                let start = dirs_downloads();
                let _ = std::thread::Builder::new().name("dianmo-pick".into()).spawn(move || {
                    proxy.post(DoubaoPicked(shell::pick_exe("选择豆包语音程序（DouBaoVoice*.exe）", start.as_deref())));
                });
                Response::none()
            }
            A::ResetDoubaoExe => {
                self.voice.set_doubao_exe(None);
                self.check_voice_engines(host);
                Response::none()
            }
            A::SetClipHistory(_) | A::SetClipSkipPasswords(_) => Response::none(),
            A::SetClipLimit(n) => {
                self.clips.set_limit(n as usize);
                self.push_clips(view)
            }
            A::ClearClipboardHistory => {
                self.clips.clear_unpinned();
                self.toast(host, "已清空剪贴板历史");
                self.push_clips(view)
            }
            A::UnpinClip(id) => {
                if self.clips.pin(id, false) {
                    self.save_clips();
                }
                self.push_clips(view)
            }
            A::SetAutoUpdate(on) => {
                self.set_auto_update(on);
                Response::none()
            }
            A::CheckUpdate => {
                self.check_update(host);
                Response::none()
            }
            A::InstallUpdate => {
                self.install_update(host);
                self.refresh_tray_badge(host);
                Response::none()
            }
            A::ReportIssue => {
                diag::open_url(&diag::report_issue_url());
                Response::none()
            }
            A::ExportDiagnostics => {
                self.export_diagnostics(host);
                Response::none()
            }
            A::OpenLogDir => {
                let dir = platform::data_dir();
                let _ = std::fs::create_dir_all(&dir);
                diag::open_url(&dir.to_string_lossy());
                Response::none()
            }
            A::OpenUrl(url) => {
                diag::open_url(&url);
                Response::none()
            }
            A::ShowOnboarding => {
                self.open_onboarding(host);
                Response::none()
            }
            A::FinishOnboarding => {
                if !self.settings.onboarded {
                    self.settings.onboarded = true;
                    self.save();
                }
                if let Some(id) = self.onboarding_win.filter(|&id| id == from || host.window_open(id)) {
                    host.close_window(id);
                }
                log!("onboarding finished");
                Response::none()
            }
            A::Close => {
                host.close_window(from);
                Response::none()
            }
            A::PageShown(p) => {
                self.page_shown(p, host);
                Response::none()
            }
        }
    }

    /// The settings window now shows `page`: seeing 关于 clears the tray icon's red dot (the
    /// navigation keeps its dot while the update is available).
    fn page_shown(&mut self, page: Page, host: &mut HostControl) {
        self.update_badge.set_about_shown(page == Page::About);
        self.refresh_tray_badge(host);
    }

    /// The tray icon's 「新版本」 red dot (`update::Badge`).
    fn refresh_tray_badge(&mut self, host: &mut HostControl) {
        let available = match &self.update {
            UpdateState::Available { version, .. } => Some(version.as_str()),
            _ => None,
        };
        host.set_tray_badge(self.update_badge.update(available));
    }

    // -----------------------------------------------------------------------------------------
    // Applying settings
    // -----------------------------------------------------------------------------------------

    /// The keyboard and the open windows take the theme of the 主题 setting.
    pub(crate) fn apply_theme(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        let kind: ThemeKind = self.theme_kind();
        let mut r = Response::none();
        if let Some(kv) = keyboard_view(view)
            && kv.theme() != kind
        {
            kv.set_theme(kind);
            r.repaint = true;
        }
        let dark = prefs::window_dark(self.settings.theme);
        for id in [self.settings_win, self.onboarding_win].into_iter().flatten() {
            host.set_window_dark(id, dark);
            host.update_window(id, move |v| {
                let changed = if settings_view_check(v) {
                    settings_view(v).is_some_and(|s| s.set_theme(kind))
                } else {
                    onboarding_view(v).is_some_and(|o| o.set_theme(kind))
                };
                repaint_if(changed)
            });
        }
        r
    }

    fn apply_height(&mut self, h: f32, view: &mut dyn View, host: &mut HostControl) -> Response {
        if let Some(kv) = keyboard_view(view) {
            kv.set_height_scale(h);
        }
        if host.is_visible() {
            // The host docks the window when it is shown: hide and show again to re-dock.
            host.hide();
            host.proxy().post(ShowAgain);
        }
        Response::none()
    }

    /// 按键音: starts / stops the audio thread and tells the keyboard whether to send clicks.
    pub(super) fn apply_key_sound(&mut self, view: &mut dyn View) {
        let s = &self.settings;
        let on = s.key_sound && !disabled("sound");
        if let Some(kv) = keyboard_view(view) {
            kv.set_key_sound(on);
        }
        match (&self.sound, on) {
            (None, true) => {
                self.sound = KeySound::start(s.key_sound_style, s.key_sound_volume.gain());
                log!("key sound on ({:?}, {:?})", s.key_sound_style, s.key_sound_volume);
            }
            (Some(snd), true) => {
                snd.set_style(s.key_sound_style);
                snd.set_gain(s.key_sound_volume.gain());
            }
            (Some(_), false) => {
                self.sound = None;
                log!("key sound off");
            }
            (None, false) => {}
        }
    }

    /// 双拼方案: key faces, the `；` final, and the engine's schema.
    fn apply_shuangpin(&mut self, scheme: ShuangpinScheme, view: &mut dyn View) -> Response {
        self.ctl.set_semicolon_input(scheme.uses_semicolon());
        let r = repaint_if(keyboard_view(view).is_some_and(|kv| kv.set_shuangpin(scheme)));
        merge(r, self.set_engine_shuangpin(scheme, view))
    }

    /// After 恢复默认设置: everything the settings affect, at once.
    fn apply_all(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        let mut r = self.apply_theme(view, host);
        if let Some(kv) = keyboard_view(view) {
            r.repaint |= kv.set_edit_area(self.settings.edit_area);
            kv.set_key_popup(self.settings.key_popup);
            kv.set_long_press_ms(self.settings.long_press.millis());
            r.repaint |= kv.set_candidate_size(self.settings.candidate_size);
            r.repaint |= kv.set_full_width_punct(self.settings.full_width_punct);
        }
        self.ctl.set_full_width_punct(self.settings.full_width_punct);
        self.ctl.set_space_commits_first(self.settings.space_commits_first);
        self.apply_key_sound(view);
        r = merge(r, self.apply_shuangpin(self.settings.shuangpin, view));
        self.apply_fuzzy(host);
        host.set_appbar(self.settings.appbar);
        self.set_focus_watch(self.settings.auto_show && !disabled("focus"), host);
        r = merge(r, self.set_input_mode(self.settings.input_mode, view, host));
        let layout = prefs::layout_choice(&self.settings);
        r = merge(r, self.set_layout(layout, view, host));
        if let Some(e) = VoiceEngine::from_setting(&self.settings.voice_engine)
            && e != self.voice.engine()
        {
            r = merge(r, self.set_voice_engine(e, view, host));
        }
        self.voice.set_doubao_exe(None);
        self.clips.set_limit(self.settings.clip_limit as usize);
        r = merge(r, self.push_clips(view));
        host.set_ball_enabled(ball_wanted(&self.settings));
        if let Some((right, y_frac)) = self.settings.ball {
            host.set_ball_pos(BallPos { edge: if right { BallEdge::Right } else { BallEdge::Left }, y_frac });
        }
        self.set_auto_update(self.settings.auto_update);
        merge(r, self.apply_height(self.settings.height, view, host))
    }

    // -----------------------------------------------------------------------------------------
    // System state
    // -----------------------------------------------------------------------------------------

    /// Reads autostart back from the system (the task's logon trigger / the Run entry).
    fn check_autostart(&mut self) {
        let on = platform::autostart_enabled();
        if on != self.settings.autostart {
            self.settings.autostart = on;
            self.save();
        }
    }

    /// 管理员窗口支持: is there a scheduled task for this exe? (COM; on a short-lived thread.)
    fn check_admin_task(&mut self, host: &mut HostControl) {
        let proxy = host.proxy();
        let elevated = elevate::is_elevated();
        if self.admin_task.text.is_empty() {
            self.admin_task = Status::new(Level::Unknown, "正在检测…");
        }
        let _ = std::thread::Builder::new().name("dianmo-task-check".into()).stack_size(256 * 1024).spawn(move || {
            let status = match platform::own_task() {
                Some(_) if elevated => Status::ok("已开启，在以管理员身份运行的窗口里也能打字"),
                Some(_) => Status::ok("已开启，下次启动点墨时生效"),
                None => Status::warn("没有开启：在以管理员身份运行的窗口里不能打字"),
            };
            proxy.post(AdminStatus(status));
        });
    }

    /// 一键修复: registers the scheduled task for this exe (directly when elevated, else through a
    /// UAC prompt for `dianmo.exe --register-task`).
    fn fix_admin_task(&mut self, host: &mut HostControl) {
        let proxy = host.proxy();
        let exe = std::env::current_exe().unwrap_or_default();
        let instance = platform::instance_name();
        let elevated = elevate::is_elevated();
        self.admin_task = Status::new(Level::Unknown, if elevated { "正在开启…" } else { "请在弹出的窗口里点「是」…" });
        let _ = std::thread::Builder::new().name("dianmo-task-fix".into()).spawn(move || {
            let suffix = if instance.is_empty() { String::new() } else { format!(" --instance {instance}") };
            let result = if elevated {
                let args = format!("{}{suffix}", elevate::TASK_ARG);
                elevate::register(&platform::task_name(), &exe, &args, None).map_err(|e| e.message())
            } else {
                match shell::run_elevated(&exe, &format!("--register-task{suffix}")) {
                    Ok(0) => Ok(()),
                    Ok(code) => Err(format!("注册计划任务失败（{code}），详情见日志")),
                    Err(_) => Err("需要管理员权限才能开启".to_owned()),
                }
            };
            log!("fixing the admin task: {result:?}");
            proxy.post(TaskFixed(result));
        });
    }

    // -----------------------------------------------------------------------------------------
    // Update and diagnostics (P6 / P7)
    // -----------------------------------------------------------------------------------------

    /// At start: the daily background check (first one a minute after start).
    pub(super) fn start_updates(&mut self, host: &mut HostControl) {
        if disabled("update") {
            return;
        }
        update::set_auto_check(self.settings.auto_update);
        update::start_auto_check(host.proxy(), self.settings.last_update_check);
    }

    fn set_auto_update(&mut self, on: bool) {
        update::set_auto_check(on);
    }

    fn check_update(&mut self, host: &mut HostControl) {
        self.update = UpdateState::Checking;
        update::check_async(host.proxy());
    }

    fn install_update(&mut self, host: &mut HostControl) {
        match self.release.clone() {
            Some(r) if r.url.is_empty() => diag::open_url(&r.page),
            Some(r) => {
                self.update = UpdateState::Downloading(0.0);
                update::download_and_install(r, host.proxy());
            }
            None => self.check_update(host),
        }
    }

    fn on_update_event(&mut self, ev: UpdateEvent, host: &mut HostControl) {
        match ev {
            UpdateEvent::Checked { auto, at, outcome } => {
                if at != 0 && self.settings.last_update_check != at {
                    self.settings.last_update_check = at;
                    self.save();
                }
                match outcome {
                    Ok(CheckOutcome::Available(r)) => {
                        log!("update {} available", r.version);
                        // Red dot on the tray icon until 关于 is seen (`refresh_tray_badge`).
                        self.update = UpdateState::Available { version: r.version.clone(), notes: r.notes.clone() };
                        self.release = Some(r);
                    }
                    Ok(CheckOutcome::UpToDate { .. } | CheckOutcome::NoRelease) => {
                        self.release = None;
                        self.update = UpdateState::UpToDate;
                    }
                    // A failed background check stays quiet.
                    Err(e) if !auto => self.update = UpdateState::Failed(e),
                    Err(_) => {
                        if self.update == UpdateState::Checking {
                            self.update = UpdateState::Unknown;
                        }
                    }
                }
            }
            UpdateEvent::Progress(f) => self.update = UpdateState::Downloading(f),
            UpdateEvent::Installing => {
                self.update = UpdateState::Downloading(1.0);
                self.toast(host, "正在安装新版本，点墨马上会自动重新启动");
            }
            UpdateEvent::Failed(e) => self.update = UpdateState::Failed(e),
        }
        self.refresh_tray_badge(host);
    }

    fn export_diagnostics(&mut self, host: &mut HostControl) {
        let proxy = host.proxy();
        self.toast(host, "正在导出诊断包…");
        let _ = std::thread::Builder::new().name("dianmo-diag".into()).spawn(move || {
            proxy.post(DiagExported(diag::export()));
        });
    }
}

fn repaint_if(changed: bool) -> Response {
    if changed { Response::repaint() } else { Response::none() }
}

/// Whether `v` is the settings window's view (else onboarding).
fn settings_view_check(v: &mut dyn View) -> bool {
    settings_view(v).is_some()
}

fn dirs_downloads() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads"))
}
