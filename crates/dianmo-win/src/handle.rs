//! The floating ball (DESIGN.md §2 悬浮球): shown while the keyboard is hidden. Tap → the app's
//! [`crate::App::on_ball`] (by default shows the keyboard); also the voice ball of voice mode.
//!
//! - Look: a ~48 DIP blue → violet → magenta gradient circle with the white calligraphic dot of
//!   the app icon and a soft shadow, rendered on the CPU into a premultiplied BGRA DIB and shown
//!   with `UpdateLayeredWindow` (per-pixel alpha: anti-aliased edge, transparent corners that
//!   don't take input).
//! - Dragging moves it; on release it snaps to the nearest left/right edge of the work area and
//!   reports the position ([`BallEvent::Moved`]). After 3 s without interaction it shrinks to a
//!   translucent 32 DIP ball; a touch brings it back. It never touches the screen edge, out or
//!   tucked, so grabbing it doesn't start a system edge swipe (geometry: `ball_geom.rs`).
//! - [`BallState::Listening`] draws a breathing halo; that animation (a ~30 fps timer) is the only
//!   thing that runs, and only in that state.
//! - Never activates (like the keyboard window).

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows::Win32::UI::Input::Pointer::GetPointerType;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, KillTimer, MA_NOACTIVATE, PA_NOACTIVATE, POINTER_INPUT_TYPE,
    PT_MOUSE, PostMessageW, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetTimer, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_MOUSEACTIVATE, WM_POINTERACTIVATE,
    WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

pub use crate::ball_geom::BallEdge;
use crate::ball_geom::{self, Layout};
use crate::clock::now_ms;

/// Where the ball sits: an edge and the vertical position of its centre as a fraction of the
/// work area's height (0 = top, 1 = bottom). Save it from [`BallEvent::Moved`] and pass it back in
/// [`crate::HostOptions::ball_pos`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallPos {
    pub edge: BallEdge,
    pub y_frac: f32,
}

impl Default for BallPos {
    fn default() -> Self {
        Self { edge: BallEdge::Left, y_frac: 0.62 }
    }
}

/// What the user did with the ball ([`crate::App::on_ball`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BallEvent {
    Tap,
    /// Held ~0.55 s without moving (voice mode: expand to the keyboard).
    LongPress,
    /// Dragged and dropped; it snapped to this edge and height.
    Moved(BallPos),
}

/// How the ball looks ([`crate::HostControl::set_ball_state`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BallState {
    #[default]
    Idle,
    /// Voice input running: breathing halo.
    Listening,
}

/// Message posted to the keyboard window: wParam = event code, lParam = packed position.
pub(crate) const BALL_TAP: usize = 1;
pub(crate) const BALL_LONG_PRESS: usize = 2;
pub(crate) const BALL_MOVED: usize = 3;

/// Decodes the keyboard-window message posted by the ball.
pub(crate) fn decode(wp: WPARAM, lp: LPARAM) -> Option<BallEvent> {
    match wp.0 {
        BALL_TAP => Some(BallEvent::Tap),
        BALL_LONG_PRESS => Some(BallEvent::LongPress),
        BALL_MOVED => {
            let edge = if lp.0 & 1 != 0 { BallEdge::Right } else { BallEdge::Left };
            let y_frac = ((lp.0 >> 1) & 0xFFFF) as f32 / 65535.0;
            Some(BallEvent::Moved(BallPos { edge, y_frac }))
        }
        _ => None,
    }
}

const DRAG_SLOP_DIP: f32 = 8.0;
const LONG_PRESS_MS: u32 = 550;
const IDLE_MS: u32 = 3000;
const FRAME_MS: u32 = 33;
const TUCKED_ALPHA: u8 = 150;

const TIMER_LONG: usize = 1;
const TIMER_IDLE: usize = 2;
const TIMER_FRAME: usize = 3;

pub(crate) struct EdgeHandle {
    hwnd: HWND,
}

struct Drag {
    id: u32,
    start: POINT,
    last: POINT,
    /// Window origin when the press began.
    origin: POINT,
    /// Ball centre relative to that origin (the tucked ball is smaller).
    center_off: f32,
    moved: bool,
    long_fired: bool,
}

struct Ball {
    hwnd: HWND,
    keyboard: HWND,
    pos: BallPos,
    state: BallState,
    /// Work area (px) and scale of the last `show`.
    work: RECT,
    scale: f32,
    shown: bool,
    tucked: bool,
    drag: Option<Drag>,
    /// Side (px) of the DIB: the out ball's window, the largest. The tucked ball uses its top-left
    /// corner.
    size: i32,
    /// The rendered out and tucked balls for their window sizes (premultiplied RGBA, f32).
    base: Vec<[f32; 4]>,
    tucked_base: Vec<[f32; 4]>,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    anim_start: u64,
}

thread_local! {
    static BALL: RefCell<Option<Ball>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Ball) -> R) -> Option<R> {
    BALL.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

impl EdgeHandle {
    pub(crate) fn new(keyboard: HWND, pos: BallPos) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance, lpszClassName: w!("DianmoBall"), ..Default::default() };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                w!("DianmoBall"),
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
            crate::host::disable_touch_feedback(hwnd);
            let y_frac = if pos.y_frac.is_finite() { pos.y_frac.clamp(0.0, 1.0) } else { 0.62 };
            BALL.with(|cell| {
                *cell.borrow_mut() = Some(Ball {
                    hwnd,
                    keyboard,
                    pos: BallPos { y_frac, ..pos },
                    state: BallState::Idle,
                    work: RECT::default(),
                    scale: 0.0,
                    shown: false,
                    tucked: false,
                    drag: None,
                    size: 0,
                    base: Vec::new(),
                    tucked_base: Vec::new(),
                    dc: HDC::default(),
                    bitmap: HBITMAP::default(),
                    bits: std::ptr::null_mut(),
                    anim_start: 0,
                })
            });
            Ok(Self { hwnd })
        }
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Where the ball sits (also after the user dragged it).
    pub(crate) fn pos(&self) -> Option<BallPos> {
        with(|b| b.pos)
    }

    /// Moves the ball to `pos` (shown at once if it is showing).
    pub(crate) fn set_pos(&self, pos: BallPos) {
        with(|b| {
            let y_frac = if pos.y_frac.is_finite() { pos.y_frac.clamp(0.0, 1.0) } else { b.pos.y_frac };
            b.pos = BallPos { y_frac, ..pos };
            if b.shown {
                b.present();
            }
        });
    }

    /// Brings the ball fully out of the screen edge and keeps it there for `ms` (instead of the
    /// usual 3 s) before it tucks in again. False if the ball isn't showing.
    pub(crate) fn reveal(&self, ms: u32) -> bool {
        with(|b| {
            if !b.shown {
                return false;
            }
            if b.tucked {
                b.tucked = false;
                b.present();
            }
            if b.state == BallState::Idle && b.drag.is_none() {
                unsafe {
                    SetTimer(Some(b.hwnd), TIMER_IDLE, ms.max(1), None);
                }
            }
            true
        })
        .unwrap_or(false)
    }

    /// Shows the ball on its edge of `work` (physical px) at `scale` (DPI / 96).
    pub(crate) fn show(&self, work: RECT, scale: f32) {
        with(|b| {
            let first = !b.shown;
            b.work = work;
            if (b.scale - scale).abs() > 0.001 || b.base.is_empty() {
                b.scale = scale;
                b.render_base();
            }
            b.shown = true;
            if first {
                b.tucked = false;
                b.restart_idle();
            }
            b.present();
            b.update_animation();
        });
    }

    pub(crate) fn hide(&self) {
        with(|b| {
            b.shown = false;
            b.drag = None;
            unsafe {
                let _ = KillTimer(Some(b.hwnd), TIMER_LONG);
                let _ = KillTimer(Some(b.hwnd), TIMER_IDLE);
                let _ = KillTimer(Some(b.hwnd), TIMER_FRAME);
                let _ = ShowWindow(b.hwnd, SW_HIDE);
            }
        });
    }

    pub(crate) fn set_state(&self, state: BallState) {
        with(|b| {
            if b.state == state {
                return;
            }
            b.state = state;
            b.anim_start = now_ms();
            if state == BallState::Listening {
                // Listening is something to see: come out of the edge.
                b.tucked = false;
                unsafe {
                    let _ = KillTimer(Some(b.hwnd), TIMER_IDLE);
                }
            } else {
                b.restart_idle();
            }
            if b.shown {
                b.present();
            }
            b.update_animation();
        });
    }
}

impl Drop for EdgeHandle {
    fn drop(&mut self) {
        BALL.with(|cell| {
            if let Ok(mut g) = cell.try_borrow_mut()
                && let Some(b) = g.take()
            {
                b.free_surface();
            }
        });
    }
}

impl Ball {
    fn px(&self, dip: f32) -> f32 {
        dip * self.scale
    }

    fn work(&self) -> ball_geom::Rect {
        let w = &self.work;
        ball_geom::Rect { left: w.left, top: w.top, right: w.right, bottom: w.bottom }
    }

    /// The window for the current position and state (out or tucked).
    fn layout(&self) -> Layout {
        ball_geom::layout(self.work(), self.scale, self.pos.edge, self.pos.y_frac, self.tucked)
    }

    fn restart_idle(&self) {
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_IDLE, IDLE_MS, None);
        }
    }

    fn update_animation(&self) {
        unsafe {
            if self.shown && self.state == BallState::Listening {
                SetTimer(Some(self.hwnd), TIMER_FRAME, FRAME_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_FRAME);
            }
        }
    }

    fn free_surface(&self) {
        unsafe {
            if !self.dc.is_invalid() {
                let _ = DeleteDC(self.dc);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
        }
    }

    /// Renders the static balls (shadow, gradient disc, dot), out and tucked, for the current scale.
    fn render_base(&mut self) {
        let (size, r, _) = ball_geom::ball_size(false, self.scale);
        if size != self.size {
            self.free_surface();
            self.size = size;
            unsafe {
                self.dc = CreateCompatibleDC(None);
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: size,
                        biHeight: -size,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                self.bitmap = CreateDIBSection(Some(self.dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
                self.bits = bits as *mut u32;
                SelectObject(self.dc, HGDIOBJ(self.bitmap.0));
            }
        }
        self.base = render_ball(size, r, self.scale);
        let (tucked_size, tucked_r, _) = ball_geom::ball_size(true, self.scale);
        self.tucked_base = render_ball(tucked_size, tucked_r, self.scale);
    }

    /// Composes the current frame into the DIB and updates the layered window.
    fn present(&mut self) {
        let l = self.layout();
        let base = if self.tucked { &self.tucked_base } else { &self.base };
        let (stride, w) = (self.size.max(0) as usize, l.size.max(0) as usize);
        if self.bits.is_null() || w == 0 || w > stride || base.len() != w * w {
            return;
        }
        let px = unsafe { std::slice::from_raw_parts_mut(self.bits, stride * stride) };
        let halo = (self.state == BallState::Listening).then(|| {
            let t = now_ms().saturating_sub(self.anim_start) as f32 / 1000.0;
            // Breathing: 1.6 s period.
            0.5 - 0.5 * (t * std::f32::consts::TAU / 1.6).cos()
        });
        let c = l.size as f32 / 2.0;
        let r = l.radius;
        // The window shows the top-left `w`×`w` of the DIB.
        for (i, &b) in base.iter().enumerate() {
            let p = &mut px[(i / w) * stride + i % w];
            let mut out = b;
            if let Some(k) = halo {
                let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
                let ring = r + self.px(2.0 + 5.0 * k);
                let width = self.px(3.0 + 2.0 * k);
                let a = (0.75 - 0.35 * k) * (-((d - ring) / width).powi(2)).exp() * (d > r - 1.0) as u8 as f32;
                // Halo under the ball: out = base + halo * (1 - base.a).
                let h = [0.93 * a, 0.29 * a, 0.60 * a, a]; // #EC4899-ish, premultiplied
                let ia = 1.0 - out[3];
                for j in 0..4 {
                    out[j] += h[j] * ia;
                }
            }
            let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            *p = (to8(out[3]) << 24) | (to8(out[0]) << 16) | (to8(out[1]) << 8) | to8(out[2]);
        }
        let origin = POINT { x: l.x, y: l.y };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: if self.tucked && self.state == BallState::Idle { TUCKED_ALPHA } else { 255 },
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: l.size, cy: l.size };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&origin),
                Some(&size),
                Some(self.dc),
                Some(&POINT::default()),
                windows::Win32::Foundation::COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), origin.x, origin.y, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW);
        }
    }

    fn post(&self, code: usize, lp: isize) {
        unsafe {
            let _ = PostMessageW(Some(self.keyboard), crate::host::WM_APP_BALL, WPARAM(code), LPARAM(lp));
        }
    }

    /// Changes only the window's constant alpha (no move: moving the window under a contact
    /// that just went down cancels the contact's capture).
    fn set_alpha(&self, alpha: u8) {
        let blend =
            BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                None,
                None,
                None,
                None,
                windows::Win32::Foundation::COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    fn pointer(&mut self, msg: u32, id: u32, pt: POINT) {
        match msg {
            WM_POINTERDOWN => {
                if self.drag.is_some() {
                    return;
                }
                if self.tucked {
                    // Solid while touched; it comes out of the edge when released.
                    self.set_alpha(255);
                }
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_IDLE);
                    SetTimer(Some(self.hwnd), TIMER_LONG, LONG_PRESS_MS, None);
                }
                let l = self.layout();
                self.drag = Some(Drag {
                    id,
                    start: pt,
                    last: pt,
                    origin: POINT { x: l.x, y: l.y },
                    center_off: l.center_off,
                    moved: false,
                    long_fired: false,
                });
            }
            WM_POINTERUPDATE => {
                let slop = self.px(DRAG_SLOP_DIP) as i32;
                let Some(d) = self.drag.as_mut().filter(|d| d.id == id) else { return };
                d.last = pt;
                let (dx, dy) = (pt.x - d.start.x, pt.y - d.start.y);
                if !d.moved && dx.abs().max(dy.abs()) < slop {
                    return;
                }
                if !d.moved {
                    d.moved = true;
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_LONG);
                    }
                }
                let (x, y) = (d.origin.x + dx, d.origin.y + dy);
                unsafe {
                    let _ = SetWindowPos(self.hwnd, None, x, y, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER);
                }
            }
            WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let Some(mut d) = self.drag.take().filter(|d| d.id == id) else { return };
                if msg == WM_POINTERUP {
                    d.last = pt; // (CAPTURECHANGED carries no position)
                }
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LONG);
                }
                self.tucked = false;
                if d.moved {
                    // Snap to the nearer edge at the drop height (also when the capture was lost).
                    let cx = (d.origin.x + d.last.x - d.start.x) as f32 + d.center_off;
                    let cy = (d.origin.y + d.last.y - d.start.y) as f32 + d.center_off;
                    let (edge, y_frac) = ball_geom::snap(self.work(), cx, cy);
                    self.pos = BallPos { edge, y_frac };
                    self.present();
                    let lp = (edge == BallEdge::Right) as isize | (((y_frac * 65535.0).round() as isize) << 1);
                    self.post(BALL_MOVED, lp);
                } else {
                    self.present();
                    if msg == WM_POINTERUP && !d.long_fired {
                        self.post(BALL_TAP, 0);
                    }
                }
                if self.state == BallState::Idle {
                    self.restart_idle();
                }
            }
            _ => {}
        }
    }

    fn timer(&mut self, id: usize) {
        match id {
            TIMER_LONG => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LONG);
                }
                if let Some(d) = self.drag.as_mut()
                    && !d.moved
                    && !d.long_fired
                {
                    d.long_fired = true;
                    self.post(BALL_LONG_PRESS, 0);
                }
            }
            TIMER_IDLE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_IDLE);
                }
                if self.shown && self.drag.is_none() && self.state == BallState::Idle && !self.tucked {
                    self.tucked = true;
                    self.present();
                }
            }
            TIMER_FRAME => {
                if self.shown && self.state == BallState::Listening {
                    self.present();
                } else {
                    self.update_animation();
                }
            }
            _ => {}
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_POINTERACTIVATE => LRESULT(PA_NOACTIVATE as isize),
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERCAPTURECHANGED => {
                let id = (wp.0 & 0xFFFF) as u32;
                let pt = POINT { x: (lp.0 & 0xFFFF) as u16 as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32 };
                let mut kind = POINTER_INPUT_TYPE(0);
                let is_mouse = msg != WM_POINTERCAPTURECHANGED && GetPointerType(id, &mut kind).is_ok() && kind == PT_MOUSE;
                if is_mouse && msg == WM_POINTERDOWN {
                    SetCapture(hwnd);
                }
                with(|b| b.pointer(msg, id, pt));
                if is_mouse && msg == WM_POINTERUP && GetCapture() == hwnd {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_TIMER => {
                with(|b| b.timer(wp.0));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering (CPU, once per scale; premultiplied RGBA in 0..1)

/// The brush dot of the app icon (`crates/dianmo/res/icon/dianmo.svg`), in its 1024 space.
const DOT: [[f32; 2]; 13] = [
    [296.0, 196.0],
    [368.0, 244.0],
    [408.0, 322.0],
    [398.0, 410.0],
    [390.0, 484.0],
    [316.0, 556.0],
    [330.0, 668.0],
    [346.0, 800.0],
    [520.0, 852.0],
    [654.0, 784.0],
    [792.0, 712.0],
    [806.0, 532.0],
    [694.0, 412.0],
];
const DOT_CLOSE: [[f32; 2]; 3] = [[600.0, 310.0], [440.0, 222.0], [296.0, 196.0]];

/// The dot outline as a polygon (cubic Béziers flattened) in the 1024 space.
fn dot_polygon() -> Vec<[f32; 2]> {
    let mut pts = vec![DOT[0]];
    let mut segs: Vec<[[f32; 2]; 4]> = Vec::new();
    let mut i = 0;
    while i + 3 < DOT.len() {
        segs.push([DOT[i], DOT[i + 1], DOT[i + 2], DOT[i + 3]]);
        i += 3;
    }
    segs.push([DOT[12], DOT_CLOSE[0], DOT_CLOSE[1], DOT_CLOSE[2]]);
    for [p0, p1, p2, p3] in segs {
        for k in 1..=24 {
            let t = k as f32 / 24.0;
            let u = 1.0 - t;
            let b = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            pts.push([
                b[0] * p0[0] + b[1] * p1[0] + b[2] * p2[0] + b[3] * p3[0],
                b[0] * p0[1] + b[1] * p1[1] + b[2] * p2[1] + b[3] * p3[1],
            ]);
        }
    }
    pts
}

fn inside(poly: &[[f32; 2]], x: f32, y: f32) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn hex(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// `src` (straight colour, alpha `a`) over premultiplied `dst`.
fn over(dst: &mut [f32; 4], src: [f32; 3], a: f32) {
    let ia = 1.0 - a;
    dst[0] = src[0] * a + dst[0] * ia;
    dst[1] = src[1] * a + dst[1] * ia;
    dst[2] = src[2] * a + dst[2] * ia;
    dst[3] = a + dst[3] * ia;
}

fn render_ball(size: i32, r: f32, scale: f32) -> Vec<[f32; 4]> {
    let n = size.max(0) as usize;
    let mut out = vec![[0.0f32; 4]; n * n];
    let c = size as f32 / 2.0;
    let (blue, violet, magenta, cyan) = (hex(0x3B82F6), hex(0x7B4DF5), hex(0xEC4899), hex(0x2DD4F0));
    // Dot: the icon's 1024 tile mapped onto the ball's bounding square, shrunk a bit and centred.
    let poly = dot_polygon();
    let (k, ox, oy) = (0.80, -39.0, -12.0); // scale about the centre; offset centres the dot's bbox
    let to_dot = |x: f32, y: f32| {
        let u = (x - (c - r)) / (2.0 * r) * 1024.0;
        let v = (y - (c - r)) / (2.0 * r) * 1024.0;
        ((u - 512.0) / k + 512.0 - ox, (v - 512.0) / k + 512.0 - oy)
    };
    let dot_cov = |x: f32, y: f32, dx: f32, dy: f32| {
        let mut hits = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let (u, v) = to_dot(x + (sx as f32 + 0.5) / 4.0 - dx, y + (sy as f32 + 0.5) / 4.0 - dy);
                hits += inside(&poly, u, v) as u32;
            }
        }
        hits as f32 / 16.0
    };
    let shadow_dy = 2.0 * scale;
    let blur = 5.0 * scale;
    for py in 0..n {
        for px in 0..n {
            let (x, y) = (px as f32, py as f32);
            let (fx, fy) = (x + 0.5, y + 0.5);
            let p = &mut out[py * n + px];
            // Soft drop shadow.
            let ds = ((fx - c).powi(2) + (fy - c - shadow_dy).powi(2)).sqrt();
            let s = (1.0 - ((ds - (r - 2.0 * scale)) / blur).clamp(0.0, 1.0)).powi(2) * 0.32;
            if s > 0.0 {
                over(p, [0.06, 0.03, 0.15], s);
            }
            // Disc: analytic anti-aliased edge.
            let d = ((fx - c).powi(2) + (fy - c).powi(2)).sqrt();
            let cov = (r - d + 0.5).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let (u, v) = ((fx - (c - r)) / (2.0 * r), (fy - (c - r)) / (2.0 * r));
            let t = ((u + v) / 2.0).clamp(0.0, 1.0);
            let mut col = if t < 0.5 { mix(blue, violet, t * 2.0) } else { mix(violet, magenta, t * 2.0 - 1.0) };
            // Cyan glow from the top-left, white sheen on the upper half.
            let g = (1.0 - (((u - 0.1).powi(2) + (v - 0.06).powi(2)).sqrt() / 0.6)).clamp(0.0, 1.0) * 0.55;
            col = mix(col, cyan, g);
            let sheen = (1.0 - v / 0.55).clamp(0.0, 1.0) * 0.16;
            col = mix(col, [1.0; 3], sheen);
            over(p, col, cov);
            // The dot's soft violet shadow, then the dot (only near the dot's bounding box).
            let (du, dv) = to_dot(fx, fy);
            if !(206.0..=896.0).contains(&du) || !(106.0..=942.0).contains(&dv) {
                continue;
            }
            let sh = dot_cov(x, y, 0.012 * 2.0 * r, 0.03 * 2.0 * r);
            if sh > 0.0 {
                over(p, hex(0x2A0F6E), sh * 0.22 * cov);
            }
            let dc = dot_cov(x, y, 0.0, 0.0);
            if dc > 0.0 {
                let ink = mix([1.0; 3], hex(0xECE9FF), v);
                over(p, ink, dc * cov);
            }
        }
    }
    out
}
