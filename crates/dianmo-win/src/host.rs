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
use std::sync::atomic::{AtomicU32, Ordering};

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
    GetWindowLongPtrW, HWND_TOPMOST, KillTimer, MA_NOACTIVATE, MSG, PA_NOACTIVATE, POINTER_INPUT_TYPE, PT_MOUSE,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SPI_SETWORKAREA, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_ENDSESSION, WM_MOUSEACTIVATE, WM_PAINT, WM_POINTERACTIVATE, WM_POINTERCAPTURECHANGED, WM_POINTERDOWN,
    WM_POINTERUP, WM_POINTERUPDATE, WM_SETTINGCHANGE, WM_SIZE, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use crate::appbar::{ABN_POSCHANGED, ABN_STATECHANGE, AppBar};
use crate::canvas::Renderer;
use crate::clock::now_ms;
use crate::handle::EdgeHandle;
use crate::tray::{self, Tray};

pub(crate) const WM_APP_CMD: u32 = WM_APP + 1;
const WM_APP_EVENT: u32 = WM_APP + 2;
const WM_APP_APPBAR: u32 = WM_APP + 3;
const WM_APP_TRAY: u32 = WM_APP + 4;

pub(crate) const CMD_SHOW: usize = 1;
const CMD_HIDE: usize = 2;
const CMD_TOGGLE: usize = 3;
const CMD_QUIT: usize = 4;
const CMD_LAYOUT: usize = 5;
const CMD_SYNC_SIZE: usize = 6;
const CMD_APPBAR_TOGGLE: usize = 7;
const CMD_REPAINT: usize = 8;

const TIMER_ID: usize = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_TABLET_QUERYSYSTEMGESTURESTATUS: u32 = 0x02CC;
const POINTER_MESSAGE_FLAG_INCONTACT: usize = 0x4;
const POINTER_MESSAGE_FLAG_CANCELED: usize = 0x8000;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

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
    /// Small tab on the left screen edge while hidden; tapping it shows the keyboard.
    pub edge_handle: bool,
    pub tray_tip: String,
    /// Upper bound of the keyboard height as a fraction of the monitor height.
    pub max_height_fraction: f32,
    /// Render on the GPU driver instead of WARP. Off by default: on the Surface the driver costs
    /// ~47 MB private memory for no measurable CPU gain. Env `DIANMO_D3D=hardware|warp` overrides.
    pub hardware_gpu: bool,
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
        }
    }
}

/// The application behind the keyboard: turns [`UiAction`]s into input (e.g. through
/// `dianmo_core::InputController` with a [`crate::SendInputSink`]) and tells the host what to do.
pub trait App {
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
}

#[derive(Debug, Default)]
struct Requests {
    visible: Option<bool>,
    appbar: Option<bool>,
    quit: bool,
}

impl Requests {
    fn is_empty(&self) -> bool {
        self.visible.is_none() && self.appbar.is_none() && !self.quit
    }
}

/// What the app may ask of the host during a callback. Requests take effect right after the
/// callback returns.
pub struct HostControl {
    visible: bool,
    appbar: bool,
    req: Requests,
    proxy: HostProxy,
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

struct Host {
    hwnd: HWND,
    tray_hwnd: HWND,
    view: Box<dyn View>,
    app: Box<dyn App>,
    renderer: Renderer,
    opts: HostOptions,
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
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Runs `f` on the host state; `None` if there is no host or it is already borrowed (a message
/// dispatched re-entrantly while a callback is running).
fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
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

        let dpi = monitor_dpi(hwnd);
        let renderer = Renderer::new(hwnd, opts.hardware_gpu)?;
        let tray = opts.tray.then(|| Tray::new(tray_hwnd, WM_APP_TRAY, &opts.tray_tip, dpi));
        let handle = if opts.edge_handle { EdgeHandle::new(hwnd).ok() } else { None };
        let start_visible = opts.start_visible;
        let appbar_on = opts.appbar;
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
            })
        });

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

    fn control(&self) -> HostControl {
        HostControl {
            visible: self.visible,
            appbar: self.appbar_on,
            req: Requests::default(),
            proxy: HostProxy { hwnd: self.hwnd.0 as isize },
        }
    }

    /// Executes a response: actions go to the app (their responses are merged in turn), then
    /// repaint and timer requests are honoured. Returns the app's host requests.
    fn process(&mut self, mut ctl: HostControl, first: Response) -> Requests {
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
            let r = self.app.on_action(action, &mut *self.view, &mut ctl);
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
        ctl.req
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
}

/// Applies host requests outside the state borrow. Visibility callbacks may produce more.
fn apply(mut req: Requests) {
    for _ in 0..8 {
        if req.is_empty() {
            return;
        }
        let next_req = std::mem::take(&mut req);
        if next_req.quit {
            if let Some(hwnd) = with(|h| h.hwnd) {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            return;
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
    let Some((hwnd, was, appbar_on)) = with(|h| (h.hwnd, h.visible, h.appbar_on)) else {
        return Requests::default();
    };
    if was == visible {
        return Requests::default();
    }
    let mut req = Requests::default();
    if visible {
        with(|h| h.visible = true);
        if appbar_on {
            appbar_op(|ab, hwnd| ab.register(hwnd, WM_APP_APPBAR));
        }
        layout();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    } else {
        if let Some(r) = with(|h| {
            h.visible = false;
            unsafe {
                let _ = KillTimer(Some(h.hwnd), TIMER_ID);
            }
            h.cancel_pointers()
        }) {
            merge_requests(&mut req, r);
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
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
    let Some((hwnd, visible, registered, max_frac)) =
        with(|h| (h.hwnd, h.visible, h.appbar.is_registered(), h.opts.max_height_fraction))
    else {
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
            Some(HWND_TOPMOST),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            flags,
        );
        sync_client_size(hwnd);
        with(|h| {
            if let Some(handle) = &h.handle {
                if visible { handle.hide() } else { handle.show(mi.rcWork, scale) }
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
    host.appbar.remove(host.hwnd);
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
        _ => {}
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
                post_cmd(hwnd, CMD_LAYOUT);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_APP_TRAY {
            let keyboard = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
            match (lp.0 & 0xFFFF) as u32 {
                NIN_SELECT | NIN_KEYSELECT => post_cmd(keyboard, CMD_TOGGLE),
                WM_CONTEXTMENU => {
                    let (visible, appbar) = with(|h| (h.visible, h.appbar_on)).unwrap_or((false, false));
                    match tray::menu(hwnd, visible, appbar) {
                        tray::CMD_TOGGLE => post_cmd(keyboard, CMD_TOGGLE),
                        tray::CMD_APPBAR => post_cmd(keyboard, CMD_APPBAR_TOGGLE),
                        tray::CMD_QUIT => post_cmd(keyboard, CMD_QUIT),
                        _ => {}
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
