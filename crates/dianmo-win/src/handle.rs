//! The edge handle: a small tab on the left screen edge, shown while the keyboard is hidden.
//! Tapping it brings the keyboard back (DESIGN.md §2 自动弹出/收起：屏幕边缘有一个小把手).
//! Never activates, painted with GDI (it's tiny and static).

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DT_CENTER, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
    FillRect, HGDIOBJ, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
    HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, PA_NOACTIVATE, PostMessageW, RegisterClassW, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_MOUSEACTIVATE,
    WM_PAINT, WM_POINTERACTIVATE, WM_POINTERDOWN, WM_POINTERUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use crate::host::{CMD_SHOW, WM_APP_CMD};

pub(crate) struct EdgeHandle {
    hwnd: HWND,
}

impl EdgeHandle {
    pub(crate) fn new(keyboard: HWND) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: w!("DianmoHandle"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                w!("DianmoHandle"),
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
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, keyboard.0 as isize);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 210, LWA_ALPHA);
            crate::host::disable_touch_feedback(hwnd);
            Ok(Self { hwnd })
        }
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Shows the tab on the left edge of `work` (physical px) at `scale` (DPI / 96).
    pub(crate) fn show(&self, work: RECT, scale: f32) {
        let w = (22.0 * scale).round() as i32;
        let h = (64.0 * scale).round() as i32;
        let top = work.top + (work.bottom - work.top) * 62 / 100;
        unsafe {
            let r = w;
            let rgn = CreateRoundRectRgn(-r, 0, w + 1, h + 1, r, r);
            SetWindowRgn(self.hwnd, Some(rgn), false);
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), work.left, top, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    pub(crate) fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_POINTERACTIVATE => LRESULT(PA_NOACTIVATE as isize),
            WM_POINTERDOWN => LRESULT(0),
            WM_POINTERUP => {
                let keyboard = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                if keyboard != 0 {
                    let _ = PostMessageW(Some(HWND(keyboard as *mut _)), WM_APP_CMD, WPARAM(CMD_SHOW), LPARAM(0));
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let bg = CreateSolidBrush(COLORREF(0x0030_3030));
                FillRect(dc, &rc, bg);
                let _ = DeleteObject(HGDIOBJ(bg.0));
                let font = CreateFontW(
                    -((rc.right - rc.left) * 7 / 10),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    FONT_CHARSET(1),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe MDL2 Assets"),
                );
                let old = SelectObject(dc, HGDIOBJ(font.0));
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00FF_FFFF));
                let mut glyph = [0xE765u16]; // KeyboardClassic
                DrawTextW(dc, &mut glyph, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(font.0));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
