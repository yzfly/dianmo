//! App windows (PRODUCT.md P3–P5: settings, about, onboarding): ordinary top-level windows that
//! activate, have a taskbar button and the exe icon, a system title bar (dark when the theme is
//! dark), and show a [`View`] painted with the same Direct2D canvas as the keyboard.
//!
//! They run on the keyboard's thread and message loop, with D3D/D2D devices of their own (freed
//! on close; see `canvas::Gpu::get`). Opened and closed through [`crate::HostControl::open_window`] /
//! [`crate::HostControl::close_window`]; the view's actions go to
//! [`crate::App::on_window_action`]. Closing a window drops its view and renderer.
//!
//! Input: `WM_POINTER` (touch, pen and — through `EnableMouseInPointer` — the mouse) →
//! [`View::pointer`]; hovering mouse/pen → [`View::hover`]; wheel → [`View::wheel`] in DIPs;
//! keys → [`View::key`]. Painting is on demand and timers are single-shot, like the keyboard, so
//! an idle window costs no CPU.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use dianmo_ui::{PointerEvent, PointerPhase, Response, View};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromPoint, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::{FEEDBACK_TYPE, SetWindowFeedbackSetting};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows::Win32::UI::Input::Pointer::GetPointerType;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetCursorPos, GetForegroundWindow, HICON, ICON_BIG, ICON_SMALL, IDC_ARROW, IMAGE_ICON, IsIconic, IsWindow,
    KillTimer, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW, MINMAXINFO, POINTER_INPUT_TYPE, PT_MOUSE, PT_PEN,
    PostMessageW, RegisterClassW, SM_CXICON, SM_CXSMICON, SPI_GETWHEELSCROLLLINES, SW_RESTORE, SW_SHOWNORMAL,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP,
    WM_MOUSEWHEEL, WM_NCACTIVATE, WM_PAINT, WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP,
    WM_POINTERUPDATE, WM_POINTERWHEEL, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_TIMER, WNDCLASSW, WS_CAPTION, WS_EX_NOREDIRECTIONBITMAP, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_OVERLAPPEDWINDOW, WS_SYSMENU,
};
use windows::core::{HSTRING, PCWSTR, Result, s, w};

use crate::canvas::Renderer;
use crate::clock::now_ms;
use crate::host::{Host, Requests, Target, apply, with};

/// Identifies an app window opened with [`crate::HostControl::open_window`]. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub(crate) u32);

/// How an app window looks. Sizes are DIPs (client area, without the title bar).
#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    pub width: f32,
    pub height: f32,
    /// Smallest client size the user can resize to (ignored when not `resizable`).
    pub min_width: f32,
    pub min_height: f32,
    /// Resizable border and maximize button.
    pub resizable: bool,
    /// Title-bar and taskbar icon from the exe's icon resource 1 (none if the exe has none).
    pub icon: bool,
    /// Dark title bar: `None` follows the Windows app theme (and its changes), `Some` forces it.
    /// Change later with [`crate::HostControl::set_window_dark`].
    pub dark: Option<bool>,
    /// Windows 11 22H2+: Mica backdrop for the title bar (ignored on Windows 10).
    pub mica: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "点墨".to_owned(),
            width: 880.0,
            height: 620.0,
            min_width: 480.0,
            min_height: 360.0,
            resizable: true,
            icon: true,
            dark: None,
            mica: false,
        }
    }
}

/// Whether Windows apps use the dark theme (Settings → Personalisation → Colours → app mode).
pub fn system_dark_mode() -> bool {
    let mut value = 1u32;
    let mut size = size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && value == 0
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub(crate) fn next_id() -> WindowId {
    WindowId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// A window requested in a callback, created by `apply` after it returns.
pub(crate) struct PendingWindow {
    pub(crate) id: WindowId,
    pub(crate) view: Box<dyn View>,
    pub(crate) opts: WindowOptions,
}

pub(crate) struct AppWindow {
    pub(crate) id: WindowId,
    pub(crate) hwnd: HWND,
    pub(crate) view: Box<dyn View>,
    renderer: Renderer,
    opts: WindowOptions,
    dpi: u32,
    /// Client size in px and the DPI the view was last resized for.
    sized: (i32, i32, u32),
    down: Vec<u32>,
    /// A mouse/pen hover was reported (so a leave is sent once).
    hovering: bool,
    icons: [HICON; 2],
    paint_failures: u32,
    /// The title-bar theme last applied (dark?), so repeated theme notifications don't repaint.
    title_dark: bool,
}

impl Drop for AppWindow {
    fn drop(&mut self) {
        for icon in self.icons {
            if !icon.is_invalid() {
                unsafe {
                    let _ = DestroyIcon(icon);
                }
            }
        }
    }
}

const CLASS: PCWSTR = w!("DianmoAppWindow");
const TIMER_ID: usize = 1;
const WM_APP_WIN: u32 = WM_APP + 20;
const CMD_REPAINT: usize = 1;
const CMD_SYNC_SIZE: usize = 2;
const WM_TABLET_QUERYSYSTEMGESTURESTATUS: u32 = 0x02CC;
const POINTER_MESSAGE_FLAG_INCONTACT: usize = 0x4;
const POINTER_MESSAGE_FLAG_CANCELED: usize = 0x8000;
/// DIPs scrolled per wheel "line" (Windows default: 3 lines per notch → 60 DIPs).
const WHEEL_LINE_DIP: f32 = 20.0;

fn style(opts: &WindowOptions) -> WINDOW_STYLE {
    if opts.resizable { WS_OVERLAPPEDWINDOW } else { WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX }
}

const EX_STYLE: WINDOW_EX_STYLE = WS_EX_NOREDIRECTIONBITMAP;

fn register_class() {
    static DONE: AtomicU32 = AtomicU32::new(0);
    if DONE.swap(1, Ordering::Relaxed) == 1 {
        return;
    }
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
    }
}

/// Outer window size in px for a client size in DIPs at `dpi`.
fn outer_size(opts: &WindowOptions, w: f32, h: f32, dpi: u32) -> (i32, i32) {
    let s = dpi as f32 / 96.0;
    let mut rc = RECT { left: 0, top: 0, right: (w * s).round() as i32, bottom: (h * s).round() as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut rc, style(opts), false, EX_STYLE, dpi);
    }
    (rc.right - rc.left, rc.bottom - rc.top)
}

/// Creates and shows a window (outside the host borrow). On failure the app hears
/// `on_window_closed` right away.
pub(crate) fn open(p: PendingWindow) {
    let PendingWindow { id, view, opts } = p;
    let Some(hardware) = with(|h| h.opts.hardware_gpu) else { return };
    let dark = opts.dark.unwrap_or_else(system_dark_mode);
    let hwnd = match create_hwnd(&opts, dark) {
        Ok(hwnd) => hwnd,
        Err(_) => {
            drop(view);
            closed(id);
            return;
        }
    };
    let renderer = match Renderer::new(hwnd, hardware, false) {
        Ok(r) => r,
        Err(_) => unsafe {
            let _ = DestroyWindow(hwnd);
            drop(view);
            closed(id);
            return;
        },
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(48);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, id.0 as isize);
    }
    let mut win = AppWindow {
        id,
        hwnd,
        view,
        renderer,
        opts,
        dpi,
        sized: (0, 0, 0),
        down: Vec::new(),
        hovering: false,
        icons: [HICON::default(); 2],
        paint_failures: 0,
        title_dark: dark,
    };
    if win.opts.icon {
        win.load_icons();
    }
    if with(|h| h.windows.push(win)).is_none() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    }
    // First frame before showing: no flash of an empty window.
    sync_client_size(hwnd);
    with_window(hwnd, |h, i| h.windows[i].paint());
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn create_hwnd(opts: &WindowOptions, dark: bool) -> Result<HWND> {
    register_class();
    unsafe {
        // On the monitor of the last pointer position (where the user just tapped), centred in
        // its work area (which excludes the docked keyboard's AppBar).
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        let work = mi.rcWork;
        let (mut w, mut h) = outer_size(opts, opts.width, opts.height, dx);
        w = w.min(work.right - work.left);
        h = h.min(work.bottom - work.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        let module = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            EX_STYLE,
            CLASS,
            &HSTRING::from(opts.title.as_str()),
            style(opts),
            x,
            y,
            w,
            h,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        // Title-bar theme before the window is first shown: Windows 10 picks it up when the
        // frame is first drawn, later changes need `repaint_caption`.
        set_dark_title(hwnd, dark);
        if opts.mica {
            let backdrop = DWMSBT_MAINWINDOW;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const _,
                size_of_val(&backdrop) as u32,
            );
        }
        // No press-and-hold right click / ring (views use long press themselves); keep the
        // normal touch contact feedback.
        let off = windows::Win32::Foundation::FALSE;
        for kind in [5, 6, 9, 10, 11] {
            let _ = SetWindowFeedbackSetting(
                hwnd,
                FEEDBACK_TYPE(kind),
                0,
                size_of_val(&off) as u32,
                Some(&off as *const _ as *const _),
            );
        }
        Ok(hwnd)
    }
}

/// Windows build number (19044 = Windows 10 21H2, 22000+ = Windows 11), from `RtlGetVersion`
/// (`GetVersionEx` lies without a compatibility manifest). 0 if unknown.
fn windows_build() -> u32 {
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| unsafe {
        type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;
        let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else { return 0 };
        let Some(f) = GetProcAddress(ntdll, s!("RtlGetVersion")) else { return 0 };
        let f = std::mem::transmute::<unsafe extern "system" fn() -> isize, RtlGetVersion>(f);
        let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
        if f(&mut info) == 0 { info.dwBuildNumber } else { 0 }
    })
}

/// `DWMWA_USE_IMMERSIVE_DARK_MODE` is 20 from Windows 10 20H1 (build 19041; 18985 in Insider
/// builds); 1809–1909 used the undocumented 19. Before 1809 there is no dark title bar.
const DARK_MODE_ATTR_BEFORE_20H1: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(19);
const BUILD_20H1: u32 = 19041;
const BUILD_1809: u32 = 17763;
const BUILD_1903: u32 = 18362;
const BUILD_WIN11: u32 = 22000;

fn set_dark_title(hwnd: HWND, dark: bool) {
    let build = windows_build();
    if build != 0 && build < BUILD_1809 {
        return;
    }
    let value = windows::core::BOOL::from(dark);
    let set = |attr: DWMWINDOWATTRIBUTE| unsafe {
        DwmSetWindowAttribute(hwnd, attr, &value as *const _ as *const _, size_of_val(&value) as u32).is_ok()
    };
    let (first, second) = if build == 0 || build >= BUILD_20H1 {
        (DWMWA_USE_IMMERSIVE_DARK_MODE, DARK_MODE_ATTR_BEFORE_20H1)
    } else {
        (DARK_MODE_ATTR_BEFORE_20H1, DWMWA_USE_IMMERSIVE_DARK_MODE)
    };
    if !set(first) {
        let _ = set(second);
    }
    // Windows 10 1903+: the same switch through user32 (what Explorer and winit use); makes sure
    // the frame's own idea of the theme matches the DWM attribute.
    if (BUILD_1903..BUILD_WIN11).contains(&build) {
        set_dark_composition(hwnd, dark);
    }
}

/// `SetWindowCompositionAttribute(WCA_USEDARKMODECOLORS)` (undocumented, user32).
fn set_dark_composition(hwnd: HWND, dark: bool) {
    #[repr(C)]
    struct Data {
        attrib: u32,
        data: *mut std::ffi::c_void,
        size: usize,
    }
    type SetWindowCompositionAttribute = unsafe extern "system" fn(HWND, *mut Data) -> windows::core::BOOL;
    const WCA_USEDARKMODECOLORS: u32 = 26;
    static FUNC: OnceLock<Option<SetWindowCompositionAttribute>> = OnceLock::new();
    let f = FUNC.get_or_init(|| unsafe {
        let user32 = GetModuleHandleW(w!("user32.dll")).ok()?;
        let f = GetProcAddress(user32, s!("SetWindowCompositionAttribute"))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, SetWindowCompositionAttribute>(f))
    });
    if let Some(f) = f {
        let mut value = windows::core::BOOL::from(dark);
        let mut data =
            Data { attrib: WCA_USEDARKMODECOLORS, data: &mut value as *mut _ as *mut _, size: size_of_val(&value) };
        unsafe {
            let _ = f(hwnd, &mut data);
        }
    }
}

/// The window is gone (destroyed, or never created): tell the app.
fn closed(id: WindowId) {
    with(|h| h.app.on_window_closed(id));
}

pub(crate) fn close(hwnd: HWND) {
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

pub(crate) fn focus(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

pub(crate) fn set_title(hwnd: HWND, title: &str) {
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(title));
    }
}

pub(crate) fn set_dark(hwnd: HWND, dark: Option<bool>) {
    let Some(()) = with_window(hwnd, |h, i| h.windows[i].opts.dark = dark) else { return };
    update_dark_title(hwnd, dark.unwrap_or_else(system_dark_mode));
}

/// Applies a title-bar theme change to an open window and redraws the caption (nothing if the
/// window already has that theme).
fn update_dark_title(hwnd: HWND, dark: bool) {
    if with_window(hwnd, |h, i| std::mem::replace(&mut h.windows[i].title_dark, dark)) == Some(dark) {
        return;
    }
    set_dark_title(hwnd, dark);
    repaint_caption(hwnd);
}

/// Windows 10 applies a changed title-bar theme only when the frame is next drawn (seen on the
/// Surface: the attribute read back as set, the bar stayed light until the window was
/// deactivated). Windows 11 redraws by itself.
/// - `SWP_FRAMECHANGED` makes the frame recalculate and redraw (no move, size or activation).
/// - Toggling the caption's activation state (`WM_NCACTIVATE` to `DefWindowProc`, the same
///   path as a real deactivate / reactivate, minus the focus change) makes DWM redraw the
///   caption with the new colours. Focus and the foreground window do not change.
fn repaint_caption(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
        let build = windows_build();
        if build != 0 && build >= BUILD_WIN11 {
            return;
        }
        let active = GetForegroundWindow() == hwnd;
        let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(!active as usize), LPARAM(0));
        let _ = DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(active as usize), LPARAM(0));
    }
}

/// Destroys every app window (host teardown; the host state is already gone, so no callbacks).
pub(crate) fn destroy_all(windows: Vec<AppWindow>) {
    for w in windows {
        let hwnd = w.hwnd;
        drop(w);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Runs `f` with the host and the index of `hwnd`'s window; `None` if the host is busy or the
/// window unknown.
fn with_window<R>(hwnd: HWND, f: impl FnOnce(&mut Host, usize) -> R) -> Option<R> {
    with(|h| {
        let i = h.windows.iter().position(|w| w.hwnd == hwnd)?;
        Some(f(h, i))
    })
    .flatten()
}

fn post(hwnd: HWND, cmd: usize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_APP_WIN, WPARAM(cmd), LPARAM(0));
    }
}

fn sync_client_size(hwnd: HWND) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    if with_window(hwnd, |h, i| h.windows[i].sync_size(rc.right - rc.left, rc.bottom - rc.top)).is_none()
        && with(|_| ()).is_none()
    {
        post(hwnd, CMD_SYNC_SIZE);
    }
}

impl AppWindow {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn sync_size(&mut self, w: i32, h: i32) {
        // Minimised: keep the last size (the view and swap chain stay as they are).
        if w <= 0 || h <= 0 || (w, h, self.dpi) == self.sized {
            return;
        }
        self.sized = (w, h, self.dpi);
        let s = self.scale();
        self.renderer.resize(w as u32, h as u32, self.dpi as f32);
        self.view.resize(w as f32 / s, h as f32 / s);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn paint(&mut self) {
        let view = &mut self.view;
        match self.renderer.render(|canvas| view.paint(canvas)) {
            Ok(true) => self.paint_failures = 0,
            Ok(false) | Err(_) => {
                self.paint_failures += 1;
                if self.paint_failures <= 3 {
                    post(self.hwnd, CMD_REPAINT);
                }
            }
        }
    }

    fn load_icons(&mut self) {
        unsafe {
            let Ok(module) = GetModuleHandleW(None) else { return };
            for (slot, (which, metric)) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)].into_iter().enumerate() {
                let size = GetSystemMetricsForDpi(metric, self.dpi);
                let icon = LoadImageW(
                    Some(module.into()),
                    PCWSTR(std::ptr::without_provenance(1)),
                    IMAGE_ICON,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                )
                .map(|h| HICON(h.0))
                .unwrap_or_default();
                if icon.is_invalid() {
                    continue;
                }
                SendMessageW(self.hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
                let old = std::mem::replace(&mut self.icons[slot], icon);
                if !old.is_invalid() {
                    let _ = DestroyIcon(old);
                }
            }
        }
    }
}

impl Host {
    fn window_respond(&mut self, i: usize, r: Response) -> Requests {
        let id = self.windows[i].id;
        let ctl = self.control();
        self.process_for(ctl, Target::Window(id), r)
    }

    fn window_pointer(&mut self, i: usize, msg: u32, id: u32, flags: usize, pt: POINT, hover_ok: bool) -> Requests {
        let w = &mut self.windows[i];
        let s = w.scale();
        let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
        let phase = match msg {
            WM_POINTERDOWN => {
                if !w.down.contains(&id) {
                    w.down.push(id);
                }
                PointerPhase::Down
            }
            WM_POINTERUPDATE => {
                if w.down.contains(&id) && flags & POINTER_MESSAGE_FLAG_INCONTACT != 0 {
                    PointerPhase::Move
                } else if hover_ok && flags & POINTER_MESSAGE_FLAG_INCONTACT == 0 {
                    w.hovering = true;
                    let r = w.view.hover(x, y);
                    return self.window_respond(i, r);
                } else {
                    return Requests::default();
                }
            }
            WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let Some(k) = w.down.iter().position(|&d| d == id) else { return Requests::default() };
                w.down.remove(k);
                if msg == WM_POINTERCAPTURECHANGED || flags & POINTER_MESSAGE_FLAG_CANCELED != 0 {
                    PointerPhase::Cancel
                } else {
                    PointerPhase::Up
                }
            }
            _ => return Requests::default(),
        };
        let r = w.view.pointer(PointerEvent { id, phase, x, y, time_ms: now_ms() });
        self.window_respond(i, r)
    }
}

fn wheel_lines() -> f32 {
    let mut lines = 3u32;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            Some(&mut lines as *mut u32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    lines as f32
}

/// `delta`: raw wheel delta (120 per notch, positive = away from the user); `pt` in screen px.
fn wheel(hwnd: HWND, delta: i16, mut pt: POINT) {
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let lines = wheel_lines();
    let req = with_window(hwnd, |h, i| {
        let w = &mut h.windows[i];
        let s = w.scale();
        let notches = -(delta as f32) / 120.0;
        let dip = if lines >= u32::MAX as f32 {
            // "One screen at a time".
            notches * (w.sized.1 as f32 / s) * 0.9
        } else {
            notches * lines * WHEEL_LINE_DIP
        };
        let r = w.view.wheel(pt.x as f32 / s, pt.y as f32 / s, dip);
        h.window_respond(i, r)
    });
    if let Some(req) = req {
        apply(req);
    }
}

fn screen_point(lp: LPARAM) -> POINT {
    POINT { x: (lp.0 & 0xFFFF) as u16 as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32 }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TABLET_QUERYSYSTEMGESTURESTATUS => {
                // TABLET_DISABLE_PRESSANDHOLD | PENTAPFEEDBACK | PENBARRELFEEDBACK | FLICKS
                LRESULT(0x0000_0001 | 0x0000_0008 | 0x0000_0010 | 0x0001_0000)
            }
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let id = (wp.0 & 0xFFFF) as u32;
                let flags = (wp.0 >> 16) & 0xFFFF;
                let mut pt = screen_point(lp);
                let _ = ScreenToClient(hwnd, &mut pt);
                let mut kind = POINTER_INPUT_TYPE(0);
                let typed = msg != WM_POINTERCAPTURECHANGED && GetPointerType(id, &mut kind).is_ok();
                let is_mouse = typed && kind == PT_MOUSE;
                let hover_ok = typed && (kind == PT_MOUSE || kind == PT_PEN);
                if is_mouse && msg == WM_POINTERDOWN {
                    SetCapture(hwnd);
                }
                let req = with_window(hwnd, |h, i| h.window_pointer(i, msg, id, flags, pt, hover_ok));
                if is_mouse && msg == WM_POINTERUP && GetCapture() == hwnd {
                    let _ = ReleaseCapture();
                }
                if let Some(req) = req {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_POINTERLEAVE => {
                let req = with_window(hwnd, |h, i| {
                    let w = &mut h.windows[i];
                    if !w.hovering {
                        return Requests::default();
                    }
                    w.hovering = false;
                    let r = w.view.hover(-1.0, -1.0);
                    h.window_respond(i, r)
                });
                if let Some(req) = req {
                    apply(req);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_POINTERWHEEL | WM_MOUSEWHEEL => {
                wheel(hwnd, ((wp.0 >> 16) & 0xFFFF) as u16 as i16, screen_point(lp));
                LRESULT(0)
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                let vk = wp.0 as u32;
                if let Some(req) = with_window(hwnd, |h, i| {
                    let r = h.windows[i].view.key(vk, down);
                    h.window_respond(i, r)
                }) {
                    apply(req);
                }
                // Alt+F4, Alt+Space and friends keep working.
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_TIMER if wp.0 == TIMER_ID => {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                if let Some(req) = with_window(hwnd, |h, i| {
                    let r = h.windows[i].view.timer(now_ms());
                    h.window_respond(i, r)
                }) {
                    apply(req);
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                if with_window(hwnd, |h, i| h.windows[i].paint()).is_none() && with(|_| ()).is_none() {
                    post(hwnd, CMD_REPAINT);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let (w, h) = ((lp.0 & 0xFFFF) as i32, ((lp.0 >> 16) & 0xFFFF) as i32);
                if with_window(hwnd, |host, i| host.windows[i].sync_size(w, h)).is_none() && with(|_| ()).is_none() {
                    post(hwnd, CMD_SYNC_SIZE);
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                with_window(hwnd, |h, i| {
                    let w = &mut h.windows[i];
                    w.dpi = dpi.max(48);
                    if w.opts.icon {
                        w.load_icons();
                    }
                });
                let rc = *(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                sync_client_size(hwnd);
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let dpi = GetDpiForWindow(hwnd).max(48);
                if let Some(Some((w, h))) = with_window(hwnd, |host, i| {
                    let o = &host.windows[i].opts;
                    o.resizable.then(|| outer_size(o, o.min_width.max(1.0), o.min_height.max(1.0), dpi))
                }) {
                    let info = &mut *(lp.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize = POINT { x: w, y: h };
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SETTINGCHANGE => {
                // "ImmersiveColorSet": the app theme may have changed.
                if lp.0 != 0 {
                    let name = PCWSTR(lp.0 as *const u16);
                    if name.to_string().is_ok_and(|s| s == "ImmersiveColorSet")
                        && with_window(hwnd, |h, i| h.windows[i].opts.dark) == Some(None)
                    {
                        update_dark_title(hwnd, system_dark_mode());
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_APP_WIN => {
                match wp.0 {
                    CMD_REPAINT => {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    CMD_SYNC_SIZE => sync_client_size(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                // Remove the window and drop its view and renderer (settings don't stay in
                // memory), then tell the app. If the host is busy (destroyed from inside a
                // callback, which `apply` avoids), the keyboard window reaps it later.
                let removed = with(|h| {
                    let i = h.windows.iter().position(|w| w.hwnd == hwnd)?;
                    Some(h.windows.remove(i))
                });
                match removed {
                    Some(Some(w)) => {
                        let id = w.id;
                        drop(w);
                        closed(id);
                    }
                    Some(None) => {}
                    None => crate::host::post_reap(),
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Drops windows whose `HWND` is gone (destroyed while the host was busy) and tells the app.
pub(crate) fn reap() {
    let dead: Vec<AppWindow> = with(|h| {
        let mut dead = Vec::new();
        let mut i = 0;
        while i < h.windows.len() {
            if unsafe { IsWindow(Some(h.windows[i].hwnd)).as_bool() } {
                i += 1;
            } else {
                dead.push(h.windows.remove(i));
            }
        }
        dead
    })
    .unwrap_or_default();
    for w in dead {
        let id = w.id;
        drop(w);
        closed(id);
    }
}

/// The timer of `hwnd`'s view (single shot; a later request replaces it).
pub(crate) fn set_timer(hwnd: HWND, ms: u64) {
    unsafe {
        SetTimer(Some(hwnd), TIMER_ID, ms.clamp(1, u32::MAX as u64) as u32, None);
    }
}

pub(crate) fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
