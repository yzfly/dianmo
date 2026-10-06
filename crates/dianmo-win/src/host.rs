//! The keyboard window (DESIGN.md §3 界面): docked at the bottom of the screen, full width, never
//! activated, multi-touch through `WM_POINTER`, painted on demand with Direct2D.
//!
//! Threading and re-entrancy: everything runs on the thread that called [`run`]. The host state
//! lives in a thread-local `RefCell`. Calls into the view/app happen while it is borrowed, so the
//! app can't move windows directly: [`HostControl`] only records requests, which the host applies
//! after the borrow ends. Win32 calls that may dispatch messages back to us (`SetWindowPos`,
//! `SHAppBarMessage`, `TrackPopupMenu`) are also made outside the borrow.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use dianmo_ui::{PointerEvent, PointerPhase, Response, UiAction, View};
use windows::Win32::Foundation::{FALSE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
    PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{FEEDBACK_TYPE, SetWindowFeedbackSetting};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows::Win32::UI::Input::Pointer::{EnableMouseInPointer, GetPointerType};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW,
    GetWindowLongPtrW, HWND_BOTTOM, HWND_TOPMOST, KillTimer, MA_NOACTIVATE, MSG, PA_NOACTIVATE, POINTER_INPUT_TYPE, PT_MOUSE,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SPI_SETWORKAREA, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_ENDSESSION, WM_MOUSEACTIVATE, WM_PAINT, WM_POINTERACTIVATE, WM_POINTERCAPTURECHANGED, WM_POINTERDOWN,
    WM_POINTERUP, WM_POINTERUPDATE, WM_SETTINGCHANGE, WM_SIZE, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use crate::appbar::{ABN_FULLSCREENAPP, ABN_POSCHANGED, ABN_STATECHANGE, AppBar};
use crate::canvas::Renderer;
use crate::clock::now_ms;
use crate::handle::{BallEvent, BallPos, BallState, EdgeHandle};
use crate::tray::{self, Choice, Tray, TrayItem};
use crate::window::{self, AppWindow, PendingWindow, WindowId, WindowOptions};

pub(crate) const WM_APP_CMD: u32 = WM_APP + 1;
const WM_APP_EVENT: u32 = WM_APP + 2;
const WM_APP_APPBAR: u32 = WM_APP + 3;
const WM_APP_TRAY: u32 = WM_APP + 4;
const WM_APP_TRAY_CMD: u32 = WM_APP + 5;
const WM_APP_FULLSCREEN: u32 = WM_APP + 6;
/// From the floating ball: wParam = event code, lParam = packed position (`handle::decode`).
pub(crate) const WM_APP_BALL: u32 = WM_APP + 7;

pub(crate) const CMD_SHOW: usize = 1;
const CMD_HIDE: usize = 2;
const CMD_TOGGLE: usize = 3;
const CMD_QUIT: usize = 4;
const CMD_LAYOUT: usize = 5;
const CMD_SYNC_SIZE: usize = 6;
const CMD_APPBAR_TOGGLE: usize = 7;
const CMD_REPAINT: usize = 8;
/// Drop app windows destroyed while the host was busy (`window::reap`).
const CMD_REAP: usize = 9;

const TIMER_ID: usize = 1;
/// Fires once the keyboard has been hidden for [`TRIM_AFTER_MS`]: the renderer's device and
/// surfaces are released (memory) until it is shown again.
const TIMER_TRIM: usize = 2;
const TRIM_AFTER_MS: u32 = 5000;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_TABLET_QUERYSYSTEMGESTURESTATUS: u32 = 0x02CC;
const POINTER_MESSAGE_FLAG_INCONTACT: usize = 0x4;
const POINTER_MESSAGE_FLAG_CANCELED: usize = 0x8000;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// The keyboard window (for [`post_reap`]); 0 when there is none.
static KEYBOARD: AtomicIsize = AtomicIsize::new(0);

pub(crate) fn post_reap() {
    let hwnd = KEYBOARD.load(Ordering::Relaxed);
    if hwnd != 0 {
        post_cmd(HWND(hwnd as *mut _), CMD_REAP);
    }
}

/// Host configuration.
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// Show the keyboard right away.
    pub start_visible: bool,
    /// Reserve the screen strip under the keyboard (AppBar) while visible. Toggleable at runtime
    /// from the tray menu or [`HostControl::set_appbar`].
    pub appbar: bool,
    /// Notification-area icon (tap = show/hide, menu = AppBar switch and 退出).
    pub tray: bool,
    /// Floating ball on a screen edge while the keyboard is hidden ([`App::on_ball`]; by default
    /// tapping it shows the keyboard). The field keeps its old name (it was an edge tab).
    pub edge_handle: bool,
    /// Where the floating ball starts (saved from [`BallEvent::Moved`]); `None` = left edge,
    /// a bit below the middle.
    pub ball_pos: Option<BallPos>,
    pub tray_tip: String,
    /// Upper bound of the keyboard height as a fraction of the monitor height.
    pub max_height_fraction: f32,
    /// Render on the GPU driver instead of WARP. Off by default: on the Surface the driver costs
    /// ~47 MB private memory for no measurable CPU gain. Env `DIANMO_D3D=hardware|warp` overrides.
    pub hardware_gpu: bool,
    /// App items for the tray menu, shown above the built-in ones. Change at runtime with
    /// [`HostControl::set_tray_menu`]; choices arrive in [`App::on_tray_command`].
    pub tray_menu: Vec<TrayItem>,
    /// Extra directory searched for `<name>.png` by `Canvas::image`, after the exe's RCDATA
    /// resources and `res\` next to the exe.
    pub image_dir: Option<PathBuf>,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            start_visible: true,
            appbar: true,
            tray: true,
            edge_handle: true,
            tray_tip: "点墨 · 触屏输入法".to_owned(),
            max_height_fraction: 0.5,
            hardware_gpu: false,
            tray_menu: Vec::new(),
            ball_pos: None,
            image_dir: None,
        }
    }
}

/// The application behind the keyboard: turns [`UiAction`]s into input (e.g. through
/// `dianmo_core::InputController` with a [`crate::SendInputSink`]) and tells the host what to do.
pub trait App {
    /// Called once after the window, tray icon and edge handle exist, before the keyboard is
    /// first shown (e.g. to start [`crate::start_focus_watcher`] with `host.proxy()`).
    fn on_start(&mut self, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = (view, host);
        Response::none()
    }

    /// Handles one action from the view. The returned response is merged like a view response
    /// (repaint / timer / nested actions, which come back here).
    fn on_action(&mut self, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response;

    /// A value posted with [`HostProxy::post`] (e.g. from a focus-watcher thread).
    fn on_event(&mut self, event: Box<dyn Any + Send>, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = (event, view, host);
        Response::none()
    }

    /// The keyboard was shown or hidden (by the app, the tray, the edge handle or a proxy).
    fn on_visibility_changed(&mut self, visible: bool, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = (visible, view, host);
        Response::none()
    }

    /// The floating ball was tapped, long-pressed or moved. Default: tap and long press show the
    /// keyboard; save the position of `Moved` to pass it back in [`HostOptions::ball_pos`].
    fn on_ball(&mut self, event: BallEvent, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = view;
        if matches!(event, BallEvent::Tap | BallEvent::LongPress) {
            host.show();
        }
        Response::none()
    }

    /// An app item of the tray menu ([`TrayItem::Command`]) was chosen.
    fn on_tray_command(&mut self, id: u32, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = (id, view, host);
        Response::none()
    }

    /// Handles one action from the view of app window `id` (opened with
    /// [`HostControl::open_window`]); `view` is that window's view. The returned response
    /// applies to that window (repaint / timer / nested actions, which come back here). To change
    /// the keyboard view from here use [`HostControl::update_keyboard`].
    fn on_window_action(&mut self, id: WindowId, action: UiAction, view: &mut dyn View, host: &mut HostControl) -> Response {
        let _ = (id, action, view, host);
        Response::none()
    }

    /// App window `id` is gone: closed by the user (title-bar ×, Alt+F4), by
    /// [`HostControl::close_window`], or it could not be created. Its view was already dropped.
    fn on_window_closed(&mut self, id: WindowId) {
        let _ = id;
    }
}

/// Deferred work on a view (see [`HostControl::update_keyboard`]).
type ViewUpdate = Box<dyn FnOnce(&mut dyn View) -> Response>;

#[derive(Default)]
pub(crate) struct Requests {
    visible: Option<bool>,
    appbar: Option<bool>,
    quit: bool,
    ball_state: Option<BallState>,
    /// Applied by `Host::process` itself (no window operations involved).
    tray_menu: Option<Vec<TrayItem>>,
    keyboard_updates: Vec<ViewUpdate>,
    window_updates: Vec<(WindowId, ViewUpdate)>,
    /// App windows (applied outside the borrow, in this order).
    opens: Vec<PendingWindow>,
    closes: Vec<WindowId>,
    titles: Vec<(WindowId, String)>,
    dark: Vec<(WindowId, Option<bool>)>,
    focus: Vec<WindowId>,
}

impl Requests {
    fn is_empty(&self) -> bool {
        self.visible.is_none()
            && self.appbar.is_none()
            && !self.quit
            && self.ball_state.is_none()
            && self.opens.is_empty()
            && self.closes.is_empty()
            && self.titles.is_empty()
            && self.dark.is_empty()
            && self.focus.is_empty()
    }
}

/// What the app may ask of the host during a callback. Requests take effect right after the
/// callback returns.
pub struct HostControl {
    visible: bool,
    appbar: bool,
    fullscreen: bool,
    req: Requests,
    proxy: HostProxy,
    /// App windows open as of this callback (including ones opened in it, minus closed ones).
    windows: Vec<WindowId>,
}

impl HostControl {
    pub fn show(&mut self) {
        self.visible = true;
        self.req.visible = Some(true);
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.req.visible = Some(false);
    }

    pub fn toggle(&mut self) {
        if self.visible { self.hide() } else { self.show() }
    }

    /// Visibility including requests made in this callback.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Turns the AppBar (screen space reservation) on or off.
    pub fn set_appbar(&mut self, on: bool) {
        self.appbar = on;
        self.req.appbar = Some(on);
    }

    pub fn appbar_enabled(&self) -> bool {
        self.appbar
    }

    /// Closes the keyboard window and makes [`run`] return.
    pub fn quit(&mut self) {
        self.req.quit = true;
    }

    /// A handle other threads can use to post work to the UI thread.
    pub fn proxy(&self) -> HostProxy {
        self.proxy
    }

    /// Replaces the app's tray menu items (shown above the built-in ones).
    pub fn set_tray_menu(&mut self, items: Vec<TrayItem>) {
        self.req.tray_menu = Some(items);
    }

    /// A full-screen app (video, game, F11 browser, slideshow) is in the foreground, as reported
    /// by the shell (`ABN_FULLSCREENAPP`). The keyboard then drops out of the topmost band and
    /// the edge handle hides; showing the keyboard still puts it on top.
    pub fn fullscreen_app(&self) -> bool {
        self.fullscreen
    }

    /// How the floating ball looks: [`BallState::Listening`] draws a breathing halo (voice
    /// input running; the only animation, and only in that state).
    pub fn set_ball_state(&mut self, state: BallState) {
        self.req.ball_state = Some(state);
    }

    /// Opens an app window (settings, about, onboarding) showing `view`, centred on the monitor
    /// of the last pointer position, activated and in front. The window is created right after
    /// the callback returns; the id is valid at once (for [`Self::window_open`],
    /// [`Self::update_window`], ...). Its view's actions go to [`App::on_window_action`].
    pub fn open_window(&mut self, view: Box<dyn View>, opts: WindowOptions) -> WindowId {
        let id = window::next_id();
        self.req.opens.push(PendingWindow { id, view, opts });
        self.windows.push(id);
        id
    }

    /// Closes app window `id` (its view is dropped; [`App::on_window_closed`] follows). A
    /// window opened in this same callback is simply never created (no `on_window_closed`).
    pub fn close_window(&mut self, id: WindowId) {
        self.windows.retain(|&w| w != id);
        if let Some(i) = self.req.opens.iter().position(|p| p.id == id) {
            self.req.opens.remove(i);
        } else {
            self.req.closes.push(id);
        }
    }

    /// Whether app window `id` is open (including requests made in this callback).
    pub fn window_open(&self, id: WindowId) -> bool {
        self.windows.contains(&id)
    }

    /// Ids of the open app windows.
    pub fn windows(&self) -> &[WindowId] {
        &self.windows
    }

    /// Brings app window `id` to the front (restoring it if minimised) and activates it.
    pub fn focus_window(&mut self, id: WindowId) {
        self.req.focus.push(id);
    }

    pub fn set_window_title(&mut self, id: WindowId, title: impl Into<String>) {
        self.req.titles.push((id, title.into()));
    }

    /// Dark title bar for window `id`: `None` follows the Windows app theme.
    pub fn set_window_dark(&mut self, id: WindowId, dark: Option<bool>) {
        self.req.dark.push((id, dark));
    }

    /// Runs `f` on the keyboard view right after this callback (e.g. a theme chosen in the
    /// settings window). Its response is handled like a keyboard view response (actions go to
    /// [`App::on_action`]). `f` must not assume a concrete view type without checking
    /// ([`View::as_any_mut`]).
    pub fn update_keyboard(&mut self, f: impl FnOnce(&mut dyn View) -> Response + 'static) {
        self.req.keyboard_updates.push(Box::new(f));
    }

    /// Runs `f` on the view of app window `id` right after this callback (e.g. the layout was
    /// changed from the tray while settings are open); its response is handled like that
    /// window's view response. Nothing happens if the window is closed.
    pub fn update_window(&mut self, id: WindowId, f: impl FnOnce(&mut dyn View) -> Response + 'static) {
        self.req.window_updates.push((id, Box::new(f)));
    }
}

/// Thread-safe handle to the host (`Send + Sync + Copy`). Messages are posted to the UI thread.
#[derive(Clone, Copy, Debug)]
pub struct HostProxy {
    hwnd: isize,
}

impl HostProxy {
    fn cmd(&self, cmd: usize) -> bool {
        unsafe { PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_CMD, WPARAM(cmd), LPARAM(0)).is_ok() }
    }

    /// Delivers `event` to [`App::on_event`] on the UI thread. False if the host is gone.
    pub fn post<T: Any + Send>(&self, event: T) -> bool {
        let boxed: Box<Box<dyn Any + Send>> = Box::new(Box::new(event));
        let raw = Box::into_raw(boxed);
        let ok = unsafe {
            PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_EVENT, WPARAM(0), LPARAM(raw as isize)).is_ok()
        };
        if !ok {
            drop(unsafe { Box::from_raw(raw) });
        }
        ok
    }

    pub fn show(&self) -> bool {
        self.cmd(CMD_SHOW)
    }

    pub fn hide(&self) -> bool {
        self.cmd(CMD_HIDE)
    }

    pub fn toggle(&self) -> bool {
        self.cmd(CMD_TOGGLE)
    }

    pub fn quit(&self) -> bool {
        self.cmd(CMD_QUIT)
    }

    /// The keyboard window handle (e.g. to exclude it from focus tracking).
    pub fn hwnd(&self) -> isize {
        self.hwnd
    }
}

pub(crate) struct Host {
    hwnd: HWND,
    tray_hwnd: HWND,
    view: Box<dyn View>,
    pub(crate) app: Box<dyn App>,
    renderer: Renderer,
    pub(crate) opts: HostOptions,
    visible: bool,
    appbar_on: bool,
    appbar: AppBar,
    dpi: u32,
    /// Client size in px and the DPI the view was last resized for.
    sized: (i32, i32, u32),
    down: Vec<u32>,
    tray: Option<Tray>,
    handle: Option<EdgeHandle>,
    paint_failures: u32,
    tray_menu: Vec<TrayItem>,
    /// Shell says a full-screen app is in front.
    fullscreen: bool,
    /// The visible keyboard was pushed out of the topmost band for a full-screen app.
    demoted: bool,
    /// The tray window is registered as a (zero-size) AppBar to receive `ABN_FULLSCREENAPP`
    /// even while the keyboard itself isn't an AppBar.
    notifier: bool,
    /// Open app windows (settings, about, onboarding).
    pub(crate) windows: Vec<AppWindow>,
}

/// Whose view a response came from: decides where repaint/timer go and which App method gets
/// the actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Keyboard,
    Window(WindowId),
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Runs `f` on the host state; `None` if there is no host or it is already borrowed (a message
/// dispatched re-entrantly while a callback is running).
pub(crate) fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn post_cmd(hwnd: HWND, cmd: usize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_APP_CMD, WPARAM(cmd), LPARAM(0));
    }
}

/// Runs an AppBar operation without holding the host borrow (`SHAppBarMessage` waits on
/// Explorer, during which sent messages are dispatched to us).
fn appbar_op<R>(f: impl FnOnce(&mut AppBar, HWND) -> R) -> Option<R> {
    let (mut ab, hwnd) = with(|h| (std::mem::take(&mut h.appbar), h.hwnd))?;
    let r = f(&mut ab, hwnd);
    with(|h| h.appbar = ab);
    Some(r)
}

/// Opts the process into Per-Monitor-V2 DPI awareness (no manifest needed). [`run`] calls it;
/// call it earlier if other windows are created first. Errors (already set) are ignored.
pub fn enable_per_monitor_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Turns off Windows touch/pen visual feedback (contact circles, press-and-hold ring and the
/// press-and-hold right click) for `hwnd`.
pub(crate) fn disable_touch_feedback(hwnd: HWND) {
    let off = FALSE;
    for kind in 1..=11 {
        unsafe {
            let _ = SetWindowFeedbackSetting(
                hwnd,
                FEEDBACK_TYPE(kind),
                0,
                size_of_val(&off) as u32,
                Some(&off as *const _ as *const _),
            );
        }
    }
}

/// [`run_with`] with default options.
pub fn run(view: Box<dyn View>, app: Box<dyn App>) -> Result<()> {
    run_with(view, app, HostOptions::default())
}

/// Creates the keyboard window (plus tray icon and edge handle) and runs the message loop on
/// the calling thread until the app quits (or 退出 in the tray menu).
pub fn run_with(view: Box<dyn View>, app: Box<dyn App>, opts: HostOptions) -> Result<()> {
    enable_per_monitor_dpi();
    unsafe {
        let _ = EnableMouseInPointer(true);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        let instance = GetModuleHandleW(None)?.into();

        let class = WNDCLASSW {
            lpfnWndProc: Some(keyboard_proc),
            hInstance: instance,
            lpszClassName: w!("DianmoKeyboard"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            w!("DianmoKeyboard"),
            w!("点墨"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )?;
        disable_touch_feedback(hwnd);

        // Hidden owner of the tray icon and its menu (TrackPopupMenu needs a window that may be
        // foreground; the keyboard window must never be).
        let class = WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: instance,
            lpszClassName: w!("DianmoTray"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let tray_hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("DianmoTray"),
            w!("点墨托盘"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?;
        SetWindowLongPtrW(tray_hwnd, GWLP_USERDATA, hwnd.0 as isize);
        KEYBOARD.store(hwnd.0 as isize, Ordering::Relaxed);
        crate::canvas::set_image_dir(opts.image_dir.clone());

        let dpi = monitor_dpi(hwnd);
        let renderer = Renderer::new(hwnd, opts.hardware_gpu, true)?;
        let tray = opts.tray.then(|| Tray::new(tray_hwnd, WM_APP_TRAY, &opts.tray_tip, dpi));
        let handle = if opts.edge_handle { EdgeHandle::new(hwnd, opts.ball_pos.unwrap_or_default()).ok() } else { None };
        let start_visible = opts.start_visible;
        let appbar_on = opts.appbar;
        let tray_menu = opts.tray_menu.clone();
        let notifier = register_notifier(tray_hwnd);
        HOST.with(|cell| {
            *cell.borrow_mut() = Some(Host {
                hwnd,
                tray_hwnd,
                view,
                app,
                renderer,
                opts,
                visible: false,
                appbar_on,
                appbar: AppBar::default(),
                dpi,
                sized: (0, 0, 0),
                down: Vec::new(),
                tray,
                handle,
                paint_failures: 0,
                tray_menu,
                fullscreen: false,
                demoted: false,
                notifier,
                windows: Vec::new(),
            })
        });

        if let Some(req) = with(|h| {
            let mut ctl = h.control();
            let r = h.app.on_start(&mut *h.view, &mut ctl);
            h.process(ctl, r)
        }) {
            apply(req);
        }

        if start_visible {
            apply(Requests { visible: Some(true), ..Default::default() });
        } else {
            layout();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    teardown();
    Ok(())
}

fn monitor_dpi(hwnd: HWND) -> u32 {
    let (mut x, mut y) = (96u32, 96u32);
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_err() {
            x = 96;
        }
    }
    x.max(48)
}

impl Host {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    pub(crate) fn control(&self) -> HostControl {
        HostControl {
            visible: self.visible,
            appbar: self.appbar_on,
            fullscreen: self.fullscreen,
            req: Requests::default(),
            proxy: HostProxy { hwnd: self.hwnd.0 as isize },
            windows: self.windows.iter().map(|w| w.id).collect(),
        }
    }

    /// Executes a keyboard view response: actions go to the app (their responses are merged in
    /// turn), then repaint and timer requests are honoured. Returns the app's host requests.
    fn process(&mut self, ctl: HostControl, first: Response) -> Requests {
        self.process_for(ctl, Target::Keyboard, first)
    }

    /// [`Self::process`] for a response from `target`'s view, then the deferred view updates
    /// ([`HostControl::update_keyboard`] / [`HostControl::update_window`]) and their responses.
    pub(crate) fn process_for(&mut self, mut ctl: HostControl, target: Target, first: Response) -> Requests {
        let mut work = VecDeque::from([(target, first)]);
        let mut budget = 64;
        while let Some((target, r)) = work.pop_front() {
            match target {
                Target::Keyboard => self.process_inner(&mut ctl, r),
                Target::Window(id) => self.process_window(&mut ctl, id, r),
            }
            budget -= 1;
            if budget == 0 {
                break;
            }
            for f in std::mem::take(&mut ctl.req.keyboard_updates) {
                work.push_back((Target::Keyboard, f(&mut *self.view)));
            }
            for (id, f) in std::mem::take(&mut ctl.req.window_updates) {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    work.push_back((Target::Window(id), f(&mut *w.view)));
                } else if let Some(p) = ctl.req.opens.iter_mut().find(|p| p.id == id) {
                    // Not created yet: it gets resized and painted on creation anyway.
                    let _ = f(&mut *p.view);
                }
            }
        }
        if let Some(menu) = ctl.req.tray_menu.take() {
            self.tray_menu = menu;
        }
        ctl.req
    }

    fn process_window(&mut self, ctl: &mut HostControl, id: WindowId, first: Response) {
        let mut repaint = first.repaint;
        let mut timer = first.timer_ms;
        let mut queue: VecDeque<UiAction> = first.actions.into();
        let mut budget = 256;
        while let Some(action) = queue.pop_front() {
            budget -= 1;
            let Some(w) = self.windows.iter_mut().find(|w| w.id == id) else { break };
            if budget == 0 {
                break;
            }
            let r = self.app.on_window_action(id, action, &mut *w.view, ctl);
            repaint |= r.repaint;
            if r.timer_ms.is_some() {
                timer = r.timer_ms;
            }
            queue.extend(r.actions);
        }
        let Some(w) = self.windows.iter().find(|w| w.id == id) else { return };
        if repaint {
            window::invalidate(w.hwnd);
        }
        if let Some(ms) = timer {
            window::set_timer(w.hwnd, ms);
        }
    }

    fn window_hwnd(&self, id: WindowId) -> Option<HWND> {
        self.windows.iter().find(|w| w.id == id).map(|w| w.hwnd)
    }

    fn process_inner(&mut self, ctl: &mut HostControl, first: Response) {
        let mut repaint = false;
        let mut timer = None;
        let mut queue = VecDeque::new();
        let mut merge = |r: Response, queue: &mut VecDeque<UiAction>| {
            repaint |= r.repaint;
            if r.timer_ms.is_some() {
                timer = r.timer_ms;
            }
            queue.extend(r.actions);
        };
        merge(first, &mut queue);
        let mut budget = 256;
        while let Some(action) = queue.pop_front() {
            budget -= 1;
            if budget == 0 {
                break;
            }
            let r = self.app.on_action(action, &mut *self.view, ctl);
            merge(r, &mut queue);
        }
        unsafe {
            if repaint {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
            if let Some(ms) = timer {
                SetTimer(Some(self.hwnd), TIMER_ID, ms.clamp(1, u32::MAX as u64) as u32, None);
            }
        }
    }

    fn respond(&mut self, r: Response) -> Requests {
        let ctl = self.control();
        self.process(ctl, r)
    }

    fn pointer(&mut self, msg: u32, id: u32, flags: usize, pt: POINT) -> Requests {
        let phase = match msg {
            WM_POINTERDOWN => {
                if !self.down.contains(&id) {
                    self.down.push(id);
                }
                PointerPhase::Down
            }
            WM_POINTERUPDATE => {
                if !self.down.contains(&id) || flags & POINTER_MESSAGE_FLAG_INCONTACT == 0 {
                    return Requests::default();
                }
                PointerPhase::Move
            }
            WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let Some(i) = self.down.iter().position(|&d| d == id) else { return Requests::default() };
                self.down.remove(i);
                if msg == WM_POINTERCAPTURECHANGED || flags & POINTER_MESSAGE_FLAG_CANCELED != 0 {
                    PointerPhase::Cancel
                } else {
                    PointerPhase::Up
                }
            }
            _ => return Requests::default(),
        };
        let s = self.scale();
        let ev = PointerEvent { id, phase, x: pt.x as f32 / s, y: pt.y as f32 / s, time_ms: now_ms() };
        let r = self.view.pointer(ev);
        self.respond(r)
    }

    /// Cancels every pointer still down (on hide).
    fn cancel_pointers(&mut self) -> Requests {
        let mut req = Requests::default();
        for id in std::mem::take(&mut self.down) {
            let ev = PointerEvent { id, phase: PointerPhase::Cancel, x: 0.0, y: 0.0, time_ms: now_ms() };
            let r = self.view.pointer(ev);
            merge_requests(&mut req, self.respond(r));
        }
        req
    }

    fn sync_size(&mut self, w: i32, h: i32) {
        if (w, h, self.dpi) == self.sized {
            return;
        }
        self.sized = (w, h, self.dpi);
        let s = self.scale();
        self.renderer.resize(w.max(0) as u32, h.max(0) as u32, self.dpi as f32);
        self.view.resize(w as f32 / s, h as f32 / s);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn paint(&mut self) {
        let view = &mut self.view;
        match self.renderer.render(|canvas| view.paint(canvas)) {
            Ok(true) => self.paint_failures = 0,
            // Device lost: resources dropped, draw again with new ones.
            Ok(false) | Err(_) => {
                self.paint_failures += 1;
                if self.paint_failures <= 3 {
                    post_cmd(self.hwnd, CMD_REPAINT);
                }
            }
        }
    }
}

fn merge_requests(into: &mut Requests, r: Requests) {
    if r.visible.is_some() {
        into.visible = r.visible;
    }
    if r.appbar.is_some() {
        into.appbar = r.appbar;
    }
    into.quit |= r.quit;
    if r.ball_state.is_some() {
        into.ball_state = r.ball_state;
    }
    into.opens.extend(r.opens);
    into.closes.extend(r.closes);
    into.titles.extend(r.titles);
    into.dark.extend(r.dark);
    into.focus.extend(r.focus);
}

/// Creates, closes and updates app windows (outside the borrow: these dispatch messages).
fn apply_windows(req: &mut Requests) {
    for p in std::mem::take(&mut req.opens) {
        window::open(p);
    }
    for id in std::mem::take(&mut req.closes) {
        if let Some(Some(hwnd)) = with(|h| h.window_hwnd(id)) {
            window::close(hwnd);
        }
    }
    for (id, title) in std::mem::take(&mut req.titles) {
        if let Some(Some(hwnd)) = with(|h| h.window_hwnd(id)) {
            window::set_title(hwnd, &title);
        }
    }
    for (id, dark) in std::mem::take(&mut req.dark) {
        if let Some(Some(hwnd)) = with(|h| h.window_hwnd(id)) {
            window::set_dark(hwnd, dark);
        }
    }
    for id in std::mem::take(&mut req.focus) {
        if let Some(Some(hwnd)) = with(|h| h.window_hwnd(id)) {
            window::focus(hwnd);
        }
    }
}

/// Applies host requests outside the state borrow. Visibility callbacks may produce more.
pub(crate) fn apply(mut req: Requests) {
    for _ in 0..8 {
        if req.is_empty() {
            return;
        }
        let mut next_req = std::mem::take(&mut req);
        apply_windows(&mut next_req);
        if next_req.quit {
            if let Some(hwnd) = with(|h| h.hwnd) {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            return;
        }
        if let Some(state) = next_req.ball_state {
            with(|h| {
                if let Some(ball) = &h.handle {
                    ball.set_state(state);
                }
            });
        }
        if let Some(on) = next_req.appbar {
            set_appbar(on);
        }
        if let Some(v) = next_req.visible {
            req = set_visible(v);
        }
    }
}

fn set_appbar(on: bool) {
    let Some((visible, was)) = with(|h| {
        let was = h.appbar_on;
        h.appbar_on = on;
        (h.visible, was)
    }) else {
        return;
    };
    if was == on || !visible {
        return;
    }
    if on {
        appbar_op(|ab, hwnd| ab.register(hwnd, WM_APP_APPBAR));
    } else {
        appbar_op(|ab, hwnd| ab.remove(hwnd));
    }
    layout();
}

fn set_visible(visible: bool) -> Requests {
    let Some((hwnd, was, appbar_on, demoted)) = with(|h| (h.hwnd, h.visible, h.appbar_on, h.demoted)) else {
        return Requests::default();
    };
    if was == visible {
        if visible && demoted {
            // Shown again while pushed behind a full-screen app (e.g. a field in it was tapped).
            with(|h| h.demoted = false);
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
        }
        return Requests::default();
    }
    let mut req = Requests::default();
    if visible {
        // An explicit show goes on top even over a full-screen app.
        with(|h| {
            h.visible = true;
            h.demoted = false;
        });
        if appbar_on {
            appbar_op(|ab, hwnd| ab.register(hwnd, WM_APP_APPBAR));
        }
        layout();
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_TRIM);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            // The device may have been released while hidden: paint (and create it) right away.
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    } else {
        if let Some(r) = with(|h| {
            h.visible = false;
            h.demoted = false;
            unsafe {
                let _ = KillTimer(Some(h.hwnd), TIMER_ID);
            }
            h.cancel_pointers()
        }) {
            merge_requests(&mut req, r);
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = SetTimer(Some(hwnd), TIMER_TRIM, TRIM_AFTER_MS, None);
        }
        appbar_op(|ab, hwnd| ab.remove(hwnd));
        layout();
    }
    if let Some(r) = with(|h| {
        let mut ctl = h.control();
        let resp = h.app.on_visibility_changed(visible, &mut *h.view, &mut ctl);
        h.process(ctl, resp)
    }) {
        merge_requests(&mut req, r);
    }
    req
}

/// Docks the window at the bottom of its monitor (AppBar-negotiated when registered), sizes it
/// for the view, and places the edge handle when hidden.
fn layout() {
    let Some((hwnd, visible, registered, max_frac, demoted, fullscreen)) = with(|h| {
        (h.hwnd, h.visible, h.appbar.is_registered(), h.opts.max_height_fraction, h.demoted, h.fullscreen)
    }) else {
        return;
    };
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return;
        }
        let dpi = monitor_dpi(hwnd);
        let scale = dpi as f32 / 96.0;
        let base = if registered { mi.rcMonitor } else { mi.rcWork };
        let width = base.right - base.left;
        let max_h = ((mi.rcMonitor.bottom - mi.rcMonitor.top) as f32 * max_frac.clamp(0.1, 1.0)) as i32;
        let Some(pref) = with(|h| {
            h.dpi = dpi;
            if let Some(t) = &mut h.tray {
                t.set_dpi(dpi);
            }
            h.view.preferred_height(width as f32 / scale)
        }) else {
            return;
        };
        let height = ((pref * scale).round() as i32).clamp(1, max_h.max(1));
        let rect = if registered {
            appbar_op(|ab, hwnd| ab.place_bottom(hwnd, mi.rcMonitor, height)).unwrap_or_default()
        } else {
            RECT { top: base.bottom - height, ..base }
        };
        let flags = if visible { SWP_NOACTIVATE | SWP_SHOWWINDOW } else { SWP_NOACTIVATE | SWP_NOZORDER };
        let _ = SetWindowPos(
            hwnd,
            Some(if demoted { HWND_BOTTOM } else { HWND_TOPMOST }),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            flags,
        );
        sync_client_size(hwnd);
        with(|h| {
            if let Some(handle) = &h.handle {
                if visible || fullscreen { handle.hide() } else { handle.show(mi.rcWork, scale) }
            }
        });
    }
}

fn sync_client_size(hwnd: HWND) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    if with(|h| h.sync_size(rc.right - rc.left, rc.bottom - rc.top)).is_none() {
        post_cmd(hwnd, CMD_SYNC_SIZE);
    }
}

fn teardown() {
    let Some(mut host) = HOST.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.take())) else { return };
    KEYBOARD.store(0, Ordering::Relaxed);
    window::destroy_all(std::mem::take(&mut host.windows));
    host.appbar.remove(host.hwnd);
    if host.notifier {
        crate::appbar::remove_notifier(host.tray_hwnd);
    }
    host.tray = None;
    unsafe {
        if let Some(handle) = host.handle.take() {
            let _ = DestroyWindow(handle.hwnd());
        }
        let _ = DestroyWindow(host.tray_hwnd);
    }
    drop(host);
}

fn command(hwnd: HWND, cmd: usize) {
    match cmd {
        CMD_SHOW | CMD_HIDE | CMD_TOGGLE => {
            let Some(visible) = with(|h| h.visible) else { return };
            let want = match cmd {
                CMD_SHOW => true,
                CMD_HIDE => false,
                _ => !visible,
            };
            apply(Requests { visible: Some(want), ..Default::default() });
        }
        CMD_APPBAR_TOGGLE => {
            if let Some(on) = with(|h| h.appbar_on) {
                apply(Requests { appbar: Some(!on), ..Default::default() });
            }
        }
        CMD_QUIT => apply(Requests { quit: true, ..Default::default() }),
        CMD_LAYOUT => layout(),
        CMD_SYNC_SIZE => sync_client_size(hwnd),
        CMD_REPAINT => unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        },
        CMD_REAP => window::reap(),
        _ => {}
    }
}

/// Registers the hidden tray window as a zero-size AppBar: it reserves no space but gets
/// `ABN_FULLSCREENAPP` from the shell.
fn register_notifier(tray_hwnd: HWND) -> bool {
    crate::appbar::register_notifier(tray_hwnd, WM_APP_FULLSCREEN)
}

/// The shell reported a full-screen app opening (`on`) or closing: drop the visible keyboard out
/// of the topmost band (it stays shown, behind the full-screen window) and hide the edge handle;
/// restore both afterwards.
fn set_fullscreen(on: bool) {
    let Some((hwnd, visible, changed)) = with(|h| {
        let changed = h.fullscreen != on;
        if changed {
            h.fullscreen = on;
            h.demoted = on && h.visible;
        }
        (h.hwnd, h.visible, changed)
    }) else {
        return;
    };
    if !changed {
        return;
    }
    unsafe {
        let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
        if visible {
            let _ = SetWindowPos(hwnd, Some(if on { HWND_BOTTOM } else { HWND_TOPMOST }), 0, 0, 0, 0, flags);
        }
    }
    if !visible {
        layout();
    }
}

extern "system" fn keyboard_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_POINTERACTIVATE => LRESULT(PA_NOACTIVATE as isize),
            WM_TABLET_QUERYSYSTEMGESTURESTATUS => {
                // TABLET_DISABLE_PRESSANDHOLD | PENTAPFEEDBACK | PENBARRELFEEDBACK | FLICKS |
                // SMOOTHSCROLLING | FLICKFALLBACKKEYS
                LRESULT(0x0000_0001 | 0x0000_0008 | 0x0000_0010 | 0x0001_0000 | 0x0008_0000 | 0x0010_0000)
            }
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let id = (wp.0 & 0xFFFF) as u32;
                let flags = (wp.0 >> 16) & 0xFFFF;
                let mut pt = POINT { x: (lp.0 & 0xFFFF) as u16 as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                let mut kind = POINTER_INPUT_TYPE(0);
                let is_mouse = msg != WM_POINTERCAPTURECHANGED && GetPointerType(id, &mut kind).is_ok() && kind == PT_MOUSE;
                if is_mouse && msg == WM_POINTERDOWN {
                    SetCapture(hwnd);
                }
                let req = with(|h| h.pointer(msg, id, flags, pt));
                if is_mouse && msg == WM_POINTERUP && GetCapture() == hwnd {
                    let _ = ReleaseCapture();
                }
                if let Some(req) = req {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_TRIM => {
                let _ = KillTimer(Some(hwnd), TIMER_TRIM);
                with(|h| {
                    if !h.visible {
                        h.renderer.release();
                    }
                });
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ID => {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                if let Some(req) = with(|h| {
                    let r = h.view.timer(now_ms());
                    h.respond(r)
                }) {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                if with(|h| h.paint()).is_none() {
                    post_cmd(hwnd, CMD_REPAINT);
                }
                LRESULT(0)
            }
            WM_SIZE => {
                let (w, h) = ((lp.0 & 0xFFFF) as i32, ((lp.0 >> 16) & 0xFFFF) as i32);
                if with(|host| host.sync_size(w, h)).is_none() {
                    post_cmd(hwnd, CMD_SYNC_SIZE);
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // We dock ourselves; ignore the suggested rectangle.
                with(|h| h.dpi = (wp.0 & 0xFFFF) as u32);
                post_cmd(hwnd, CMD_LAYOUT);
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                with(|h| h.appbar.invalidate());
                post_cmd(hwnd, CMD_LAYOUT);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SETTINGCHANGE => {
                // Our own AppBar changes the work area too; when registered the shell tells us
                // about relevant changes through ABN_POSCHANGED instead.
                if wp.0 == SPI_SETWORKAREA.0 as usize && with(|h| h.appbar.is_registered()) == Some(false) {
                    post_cmd(hwnd, CMD_LAYOUT);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_WINDOWPOSCHANGED => {
                appbar_op(|ab, hwnd| ab.window_pos_changed(hwnd));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_APP_APPBAR => {
                if wp.0 == ABN_POSCHANGED || wp.0 == ABN_STATECHANGE {
                    with(|h| h.appbar.invalidate());
                    post_cmd(hwnd, CMD_LAYOUT);
                }
                LRESULT(0)
            }
            WM_APP_CMD => {
                command(hwnd, wp.0);
                LRESULT(0)
            }
            WM_APP_TRAY_CMD => {
                let id = wp.0 as u32;
                if let Some(req) = with(|h| {
                    let mut ctl = h.control();
                    let r = h.app.on_tray_command(id, &mut *h.view, &mut ctl);
                    h.process(ctl, r)
                }) {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_APP_FULLSCREEN => {
                set_fullscreen(wp.0 != 0);
                LRESULT(0)
            }
            WM_APP_BALL => {
                if let Some(event) = crate::handle::decode(wp, lp)
                    && let Some(req) = with(|h| {
                        let mut ctl = h.control();
                        let r = h.app.on_ball(event, &mut *h.view, &mut ctl);
                        h.process(ctl, r)
                    })
                {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_APP_EVENT => {
                let event = *Box::from_raw(lp.0 as *mut Box<dyn Any + Send>);
                if let Some(req) = with(|h| {
                    let mut ctl = h.control();
                    let r = h.app.on_event(event, &mut *h.view, &mut ctl);
                    h.process(ctl, r)
                }) {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_ENDSESSION if wp.0 != 0 => {
                teardown();
                LRESULT(0)
            }
            WM_DESTROY => {
                teardown();
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                // Explorer restarted: tray icon and AppBar registration are gone.
                with(|h| {
                    if let Some(t) = &h.tray {
                        t.add();
                    }
                });
                appbar_op(|ab, hwnd| ab.reregister(hwnd, WM_APP_APPBAR));
                if let Some(tray_hwnd) = with(|h| h.tray_hwnd) {
                    let ok = register_notifier(tray_hwnd);
                    with(|h| h.notifier = ok);
                }
                post_cmd(hwnd, CMD_LAYOUT);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let keyboard = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
        if msg == WM_APP_TRAY {
            match (lp.0 & 0xFFFF) as u32 {
                NIN_SELECT | NIN_KEYSELECT => post_cmd(keyboard, CMD_TOGGLE),
                WM_CONTEXTMENU => {
                    let (visible, appbar, items) =
                        with(|h| (h.visible, h.appbar_on, h.tray_menu.clone())).unwrap_or_default();
                    match tray::menu(hwnd, visible, appbar, &items) {
                        Choice::Builtin(tray::CMD_TOGGLE) => post_cmd(keyboard, CMD_TOGGLE),
                        Choice::Builtin(tray::CMD_APPBAR) => post_cmd(keyboard, CMD_APPBAR_TOGGLE),
                        Choice::Builtin(tray::CMD_QUIT) => post_cmd(keyboard, CMD_QUIT),
                        Choice::App(id) => {
                            let _ = PostMessageW(Some(keyboard), WM_APP_TRAY_CMD, WPARAM(id as usize), LPARAM(0));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        if msg == WM_APP_FULLSCREEN {
            // Notifier AppBar callback: wParam = ABN_*, lParam = TRUE when a full-screen app opens.
            if wp.0 == ABN_FULLSCREENAPP {
                let _ = PostMessageW(Some(keyboard), WM_APP_FULLSCREEN, WPARAM((lp.0 != 0) as usize), LPARAM(0));
            }
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
