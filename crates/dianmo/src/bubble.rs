//! A small hint bubble next to the floating ball (voice mode): 「点一下说话，长按展开键盘」 and
//! why voice input could not start. The keyboard's toast only works while the keyboard is shown;
//! in voice mode it usually isn't.
//!
//! A plain GDI popup on the UI thread: topmost, never activated, no taskbar button, slightly
//! translucent, rounded. It hides itself after a few seconds (one-shot timer) or when touched.
//! Nothing runs while it is hidden.
//!
//! Also [`keep_ball_out`]: keeps the ball fully visible (not tucked into the screen edge) for a
//! while, so a new voice-mode user can find it.

use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DT_CALCRECT, DT_CENTER, DT_NOPREFIX,
    DT_VCENTER, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, HFONT, HGDIOBJ, PAINTSTRUCT,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnumThreadWindows, GetClassNameW, GetWindowRect, HWND_TOPMOST, IsWindowVisible,
    KillTimer, LWA_ALPHA, MA_NOACTIVATE, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes,
    SetTimer, SetWindowPos, ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, w};

const HIDE_TIMER: usize = 1;
/// dianmo-win's ball (handle.rs): its window class and the id of its 3 s "tuck into the edge"
/// timer. Re-arming that timer with a longer delay keeps the ball out (same thread, same window).
const BALL_CLASS: &str = "DianmoBall";
const BALL_TUCK_TIMER: usize = 2;

const FONT_DIP: i32 = 15;
const PAD_X_DIP: i32 = 14;
const PAD_Y_DIP: i32 = 9;
const MAX_W_DIP: i32 = 300;
const GAP_DIP: i32 = 2;

struct State {
    hwnd: HWND,
    text: Vec<u16>,
    font: Option<HFONT>,
}

thread_local! {
    static BUBBLE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Our own ball window (found on this, the UI, thread: the user's other 点墨 has its own).
fn ball_hwnd() -> Option<HWND> {
    unsafe extern "system" fn cb(h: HWND, out: LPARAM) -> BOOL {
        let mut c = [0u16; 32];
        let n = unsafe { GetClassNameW(h, &mut c) } as usize;
        if String::from_utf16_lossy(&c[..n]) == BALL_CLASS {
            unsafe { *(out.0 as *mut Option<HWND>) = Some(h) };
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(cb), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Keeps the (shown) ball out of the screen edge for `ms` instead of 3 s. False if there is no
/// visible ball.
pub fn keep_ball_out(ms: u32) -> bool {
    let Some(ball) = ball_hwnd() else { return false };
    unsafe {
        if !IsWindowVisible(ball).as_bool() {
            return false;
        }
        SetTimer(Some(ball), BALL_TUCK_TIMER, ms, None) != 0
    }
}

/// Shows `text` beside the ball for `ms` (replacing a bubble already shown). False if the ball
/// isn't visible (keyboard shown, ball disabled) or the window could not be created.
pub fn show(text: &str, ms: u32) -> bool {
    let Some(ball) = ball_hwnd() else { return false };
    unsafe {
        if !IsWindowVisible(ball).as_bool() {
            return false;
        }
    }
    let Some(hwnd) = ensure_window() else { return false };
    let mut br = RECT::default();
    unsafe {
        if GetWindowRect(ball, &mut br).is_err() {
            return false;
        }
    }
    let dpi = unsafe { GetDpiForWindow(ball) }.max(96) as i32;
    let px = |dip: i32| dip * dpi / 96;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let font = unsafe {
        CreateFontW(-px(FONT_DIP), 0, 0, 0, 500, 0, 0, 0, Default::default(), Default::default(), Default::default(), Default::default(), 0, w!("Microsoft YaHei UI"))
    };
    // Measure.
    let mut tr = RECT { left: 0, top: 0, right: px(MAX_W_DIP), bottom: 0 };
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let mut buf = wide.clone();
        DrawTextW(dc, &mut buf, &mut tr, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
    }
    let w = tr.right - tr.left + 2 * px(PAD_X_DIP);
    let h = tr.bottom - tr.top + 2 * px(PAD_Y_DIP);
    // The ball's window has a transparent margin around the disc: overlap it a little. Which side:
    // away from the screen edge the ball sits on.
    let screen_w = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN) };
    let ball_mid = (br.left + br.right) / 2;
    let margin = px(10);
    let x = if ball_mid < screen_w / 2 { br.right - margin + px(GAP_DIP) } else { br.left + margin - px(GAP_DIP) - w };
    let y = (br.top + br.bottom) / 2 - h / 2;
    BUBBLE.with(|b| {
        if let Some(st) = b.borrow_mut().as_mut() {
            st.text = wide;
            if let Some(f) = st.font.replace(font) {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(f.0));
                }
            }
        }
    });
    unsafe {
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, px(16), px(16));
        SetWindowRgn(hwnd, Some(rgn), false); // the window owns the region now
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
        SetTimer(Some(hwnd), HIDE_TIMER, ms, None);
    }
    true
}

/// Hides the bubble (no-op if it isn't shown).
pub fn hide() {
    let hwnd = BUBBLE.with(|b| b.borrow().as_ref().map(|s| s.hwnd));
    if let Some(h) = hwnd {
        unsafe {
            let _ = KillTimer(Some(h), HIDE_TIMER);
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

fn ensure_window() -> Option<HWND> {
    if let Some(h) = BUBBLE.with(|b| b.borrow().as_ref().map(|s| s.hwnd)) {
        return Some(h);
    }
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).ok()?.into();
        let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance, lpszClassName: w!("DianmoBubble"), ..Default::default() };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            w!("DianmoBubble"),
            w!("点墨提示"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .ok()?
    };
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 238, LWA_ALPHA);
    }
    BUBBLE.with(|b| *b.borrow_mut() = Some(State { hwnd, text: Vec::new(), font: None }));
    Some(hwnd)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // A touch is turned into WM_LBUTTONDOWN by DefWindowProc: tapping the bubble dismisses it.
            WM_LBUTTONDOWN | WM_TIMER => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
                let bg = CreateSolidBrush(COLORREF(0x00302A2A)); // BGR: dark slate
                FillRect(dc, &rc, bg);
                let _ = DeleteObject(HGDIOBJ(bg.0));
                BUBBLE.with(|b| {
                    if let Some(st) = b.borrow().as_ref() {
                        let old = st.font.map(|f| SelectObject(dc, HGDIOBJ(f.0)));
                        SetBkMode(dc, TRANSPARENT);
                        SetTextColor(dc, COLORREF(0x00FFFFFF));
                        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                        let (px, py) = (PAD_X_DIP * dpi / 96, PAD_Y_DIP * dpi / 96);
                        let mut tr = RECT { left: rc.left + px, top: rc.top + py, right: rc.right - px, bottom: rc.bottom - py };
                        let mut text = st.text.clone();
                        let single = !text.contains(&(b'\n' as u16));
                        let mut flags = DT_CENTER | DT_WORDBREAK | DT_NOPREFIX;
                        if single {
                            flags |= DT_VCENTER;
                        }
                        DrawTextW(dc, &mut text, &mut tr, flags);
                        if let Some(o) = old {
                            SelectObject(dc, o);
                        }
                    }
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
