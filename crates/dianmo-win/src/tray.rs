//! Notification-area icon: tap toggles the keyboard, the menu (press-and-hold / right click) has
//! the app's own items ([`TrayItem`]) on top, then show/hide, the AppBar switch and 退出.
//!
//! The icon is the exe's icon resource 1 at the small-icon size for the current DPI (reloaded
//! when the DPI changes); without such a resource (e.g. the demo) a 「墨」 is drawn with GDI.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FW_BOLD, HGDIOBJ, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, HMENU, ICONINFO, MF_CHECKED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, PostMessageW, SM_CXSMICON, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL, CreateIconIndirect, FindWindowW, GetWindowRect,
    IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::core::{HSTRING, PCWSTR, w};

/// The window that owns our notification icon (uID 1), for [`icon_rect`]; 0 = none.
static OWNER: AtomicIsize = AtomicIsize::new(0);

/// Screen rectangle of our notification icon: on the taskbar, or in the hidden-icons flyout while
/// that is open; `None` when the icon is hidden in the closed flyout (or there is no icon).
/// Asks the shell (cross-process).
pub(crate) fn icon_rect() -> Option<RECT> {
    let owner = OWNER.load(Ordering::Relaxed);
    if owner == 0 {
        return None;
    }
    let id = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: HWND(owner as *mut _),
        uID: 1,
        ..Default::default()
    };
    let rc = unsafe { Shell_NotifyIconGetRect(&id) }.ok()?;
    (rc.right > rc.left && rc.bottom > rc.top).then_some(rc)
}

/// Whether `rc` (from [`icon_rect`]) lies on the main taskbar rather than in the flyout.
pub(crate) fn icon_on_taskbar(rc: RECT) -> bool {
    let mut tb = RECT::default();
    unsafe {
        let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) else { return false };
        if GetWindowRect(tray, &mut tb).is_err() {
            return false;
        }
    }
    rc.left >= tb.left && rc.right <= tb.right && rc.top >= tb.top && rc.bottom <= tb.bottom
}

/// The exe's icon resource 1 at `size` px, or the drawn 「墨」.
fn load_icon(size: i32) -> HICON {
    unsafe {
        if let Ok(module) = GetModuleHandleW(None)
            && let Ok(h) = LoadImageW(Some(module.into()), PCWSTR(std::ptr::without_provenance(1)), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
        {
            return HICON(h.0);
        }
    }
    make_icon(size).unwrap_or_default()
}

pub(crate) const CMD_TOGGLE: u32 = 1001;
pub(crate) const CMD_APPBAR: u32 = 1002;
pub(crate) const CMD_QUIT: u32 = 1003;

pub(crate) struct Tray {
    hwnd: HWND,
    callback: u32,
    icon: HICON,
    /// Icon size in px.
    size: i32,
    tip: String,
}

impl Tray {
    pub(crate) fn new(hwnd: HWND, callback: u32, tip: &str, dpi: u32) -> Self {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.max(16);
        let tray = Self { hwnd, callback, icon: load_icon(size), size, tip: tip.to_owned() };
        tray.add();
        OWNER.store(hwnd.0 as isize, Ordering::Relaxed);
        tray
    }

    /// Reloads the icon at the small-icon size for `dpi` (after a DPI change).
    pub(crate) fn set_dpi(&mut self, dpi: u32) {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.max(16);
        if size == self.size {
            return;
        }
        let old = std::mem::replace(&mut self.icon, load_icon(size));
        self.size = size;
        let mut d = self.data();
        d.uFlags = NIF_ICON;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
            if !old.is_invalid() {
                let _ = DestroyIcon(old);
            }
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        };
        d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        d.uCallbackMessage = self.callback;
        d.hIcon = self.icon;
        for (dst, src) in d.szTip.iter_mut().zip(self.tip.encode_utf16().take(127)) {
            *dst = src;
        }
        d
    }

    /// NIM_ADD + NOTIFYICON_VERSION_4. Called again after Explorer restarts (`TaskbarCreated`).
    pub(crate) fn add(&self) {
        let mut d = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &d);
            d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
        }
    }

}

/// An app-defined tray menu entry ([`crate::HostOptions::tray_menu`],
/// [`crate::HostControl::set_tray_menu`]). Chosen commands arrive in [`crate::App::on_tray_command`].
/// App items are listed above the built-in ones (显示/隐藏, AppBar, 退出).
#[derive(Clone, Debug, PartialEq)]
pub enum TrayItem {
    Command { id: u32, label: String, checked: bool },
    Separator,
    /// A submenu (e.g. 布局 ▸ 全拼 / 小鹤 / 九宫格).
    Submenu { label: String, items: Vec<TrayItem> },
}

/// What the user picked in the tray menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    None,
    Builtin(u32),
    App(u32),
}

/// Menu ids of app items: `APP_BASE + index` in a depth-first walk (ids stay below 0x10000 for
/// `TrackPopupMenu`), mapped back to the app's `id` afterwards.
const APP_BASE: u32 = 2000;

/// Appends `items` to `menu`; `ids` collects the app ids in menu-id order.
unsafe fn append(menu: HMENU, items: &[TrayItem], ids: &mut Vec<u32>) {
    for item in items {
        unsafe {
            match item {
                TrayItem::Command { id, label, checked } => {
                    if APP_BASE as usize + ids.len() >= 0xFFFF {
                        continue;
                    }
                    let check = if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    let text = HSTRING::from(label.as_str());
                    let _ = AppendMenuW(menu, MF_STRING | check, (APP_BASE as usize) + ids.len(), &text);
                    ids.push(*id);
                }
                TrayItem::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                TrayItem::Submenu { label, items } => {
                    let Ok(sub) = CreatePopupMenu() else { continue };
                    append(sub, items, ids);
                    let text = HSTRING::from(label.as_str());
                    // The parent menu owns (and destroys) the submenu.
                    let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &text);
                }
            }
        }
    }
}

/// Shows the tray menu at the cursor and returns the choice. `hwnd` is the (hidden) tray owner
/// window. Runs a modal loop: call without holding host state.
pub(crate) fn menu(hwnd: HWND, visible: bool, appbar: bool, items: &[TrayItem]) -> Choice {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return Choice::None };
        let mut ids = Vec::new();
        if !items.is_empty() {
            append(menu, items, &mut ids);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }
        let toggle = if visible { w!("隐藏键盘") } else { w!("显示键盘") };
        let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE as usize, toggle);
        let check = if appbar { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | check, CMD_APPBAR as usize, w!("让出屏幕空间（最大化窗口上移）"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, w!("退出点墨"));
        let mut pt = Default::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user taps elsewhere (documented quirk). This
        // activates only the hidden tray window, after the user already left the target app
        // by tapping the taskbar.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu);
        let cmd = cmd.0 as u32;
        match cmd {
            0 => Choice::None,
            CMD_TOGGLE | CMD_APPBAR | CMD_QUIT => Choice::Builtin(cmd),
            c if c >= APP_BASE && ((c - APP_BASE) as usize) < ids.len() => Choice::App(ids[(c - APP_BASE) as usize]),
            _ => Choice::None,
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        OWNER.store(0, Ordering::Relaxed);
        let d = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &d);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

/// A rounded ink-blue square with a white 「墨」, drawn with GDI.
fn make_icon(size: i32) -> Option<HICON> {
    unsafe {
        let dc = CreateCompatibleDC(None);
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
        let color = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        px.fill(0x0025_5FB0); // BGRA in memory = 0x00RRGGBB as u32
        let old = SelectObject(dc, HGDIOBJ(color.0));
        let font = CreateFontW(
            -(size * 4 / 5),
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(1),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            CLEARTYPE_QUALITY,
            0,
            w!("Microsoft YaHei UI"),
        );
        let old_font = SelectObject(dc, HGDIOBJ(font.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00FF_FFFF));
        let mut text: Vec<u16> = "墨".encode_utf16().collect();
        let mut rc = RECT { left: 0, top: 0, right: size, bottom: size };
        DrawTextW(dc, &mut text, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        SelectObject(dc, old);
        let _ = DeleteDC(dc);

        // Opaque inside a rounded square, transparent outside (hard edge).
        let r = (size as f32 * 0.22).max(2.0);
        for y in 0..size {
            for x in 0..size {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let cx = fx.clamp(r, size as f32 - r);
                let cy = fy.clamp(r, size as f32 - r);
                let inside = (fx - cx).powi(2) + (fy - cy).powi(2) <= r * r;
                let p = &mut px[(y * size + x) as usize];
                *p = if inside { (*p & 0x00FF_FFFF) | 0xFF00_0000 } else { 0 };
            }
        }

        let mask_bits = vec![0u8; ((size + 15) / 16 * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}
