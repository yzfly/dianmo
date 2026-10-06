//! The progress window: 「点墨 · 正在安装…」 and a bar. No buttons, no questions; errors end in one
//! message box. The work runs on a worker thread.

use std::cell::Cell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD,
    FillRect, HDC, HFONT, InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetMessageW, IDC_ARROW, LoadCursorW, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, SPI_GETWORKAREA, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_CAPTION,
    WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::win::{self, Opts};

const WM_PROGRESS: u32 = WM_APP + 1;
const WM_DONE: u32 = WM_APP + 2;

/// Client size in DIP.
const W: i32 = 380;
const H: i32 = 132;

thread_local! {
    static PROGRESS: Cell<u32> = const { Cell::new(0) };
}

pub fn run(opts: Opts) -> i32 {
    let hwnd = match create_window() {
        Ok(h) => h,
        Err(e) => {
            win::log(&format!("window: {e}; installing without one"));
            return match win::install(&opts, &|_| {}) {
                Ok(_) => 0,
                Err(e) => {
                    message(None, &format!("点墨没有装好：{e}"), true);
                    1
                }
            };
        }
    };
    let target = hwnd.0 as isize;
    let worker = std::thread::spawn(move || {
        let post = |msg: u32, wp: usize, lp: isize| unsafe {
            let _ = PostMessageW(Some(HWND(target as *mut _)), msg, WPARAM(wp), LPARAM(lp));
        };
        let result = win::install(&opts, &|p| post(WM_PROGRESS, p as usize, 0));
        // Tests: keep the finished window up for a screenshot.
        if let Some(ms) = std::env::var("DIANMO_SETUP_HOLD_MS").ok().and_then(|v| v.parse().ok()) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        post(WM_DONE, 0, 0);
        result
    });
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_DONE {
                break;
            }
        }
    }
    let result = worker.join().unwrap_or_else(|_| Err("安装程序内部错误".into()));
    let code = match &result {
        Ok(None) => {
            win::log("done");
            0
        }
        Ok(Some(note)) => {
            win::log(&format!("done ({note})"));
            message(Some(hwnd), &format!("点墨已经装好。\n\n{note}"), false);
            0
        }
        Err(e) => {
            win::log(&format!("failed: {e}"));
            message(Some(hwnd), &format!("点墨没有装好：{e}\n\n安装日志：%TEMP%\\DianmoSetup.log"), true);
            1
        }
    };
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    code
}

fn message(owner: Option<HWND>, text: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe {
        MessageBoxW(owner, &HSTRING::from(text), w!("点墨 安装"), MB_OK | icon);
    }
}

fn style() -> windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE {
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU
}

/// Window rect for a `W`×`H` DIP client area at `dpi`, centred in the work area.
fn window_rect(dpi: u32) -> RECT {
    let s = |v: i32| v * dpi as i32 / 96;
    let mut r = RECT { left: 0, top: 0, right: s(W), bottom: s(H) };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style(), false, WINDOW_EX_STYLE(0), dpi);
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) * 2 / 5;
        RECT { left: x, top: y, right: x + w, bottom: y + h }
    }
}

fn create_window() -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("DianmoSetup"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let r = window_rect(GetDpiForSystem());
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("DianmoSetup"),
            w!("点墨 安装"),
            style(),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        Ok(hwnd)
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PROGRESS => {
                PROGRESS.with(|p| p.set(wp.0 as u32));
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            // Installing can't be interrupted halfway.
            WM_CLOSE => LRESULT(0),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn font(dpi: u32, px: i32, weight: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -(px * dpi as i32 / 96),
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Microsoft YaHei UI"),
        )
    }
}

fn fill(dc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(color);
        FillRect(dc, &r, b);
        let _ = DeleteObject(b.into());
    }
}

fn text(dc: HDC, s: &str, mut r: RECT, f: HFONT, color: COLORREF) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let old = SelectObject(dc, f.into());
        SetTextColor(dc, color);
        DrawTextW(dc, &mut wide, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        SelectObject(dc, old);
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let (cw, ch) = (client.right, client.bottom);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let s = |v: i32| v * dpi as i32 / 96;

        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, cw, ch);
        let old_bmp = SelectObject(mem, bmp.into());
        SetBkMode(mem, TRANSPARENT);

        fill(mem, client, rgb(0xFF, 0xFF, 0xFF));
        let (title_f, sub_f) = (font(dpi, 18, FW_SEMIBOLD.0), font(dpi, 12, FW_NORMAL.0));
        let pad = s(24);
        let progress = PROGRESS.with(Cell::get).min(1000) as i32;
        let title = if progress >= 1000 { "点墨 · 安装完成" } else { "点墨 · 正在安装…" };
        text(
            mem,
            title,
            RECT { left: pad, top: s(20), right: cw - pad, bottom: s(50) },
            title_f,
            rgb(0x1F, 0x23, 0x29),
        );
        let sub = format!("版本 {}　·　{}%", win::VERSION, progress / 10);
        text(mem, &sub, RECT { left: pad, top: s(52), right: cw - pad, bottom: s(72) }, sub_f, rgb(0x86, 0x90, 0x9C));
        let bar = RECT { left: pad, top: s(92), right: cw - pad, bottom: s(98) };
        fill(mem, bar, rgb(0xE5, 0xE6, 0xEB));
        let done = RECT { right: bar.left + (bar.right - bar.left) * progress / 1000, ..bar };
        if done.right > done.left {
            fill(mem, done, rgb(0x3B, 0x6C, 0xF6));
        }
        let _ = DeleteObject(title_f.into());
        let _ = DeleteObject(sub_f.into());

        let _ = BitBlt(hdc, 0, 0, cw, ch, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}
