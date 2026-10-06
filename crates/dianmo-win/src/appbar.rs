//! Reserving the bottom of the screen with an AppBar (DESIGN.md §2 不遮挡): while the keyboard is
//! visible, maximized windows shrink to sit above it, like a phone pushing the page up.
//!
//! Registered only while visible; `ABM_REMOVE` on hide, exit and panic. After Explorer restarts
//! (`TaskbarCreated`) the registration is gone and must be redone.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABM_WINDOWPOSCHANGED, APPBARDATA, SHAppBarMessage,
};

/// For the panic hook: the AppBar window, if registered.
static REGISTERED: AtomicIsize = AtomicIsize::new(0);
static HOOKED: AtomicBool = AtomicBool::new(false);

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

#[derive(Debug, Default)]
pub(crate) struct AppBar {
    registered: bool,
    /// Last rectangle passed to `ABM_SETPOS`, to avoid feedback loops with work-area broadcasts.
    last: Option<RECT>,
}

impl AppBar {
    pub(crate) fn is_registered(&self) -> bool {
        self.registered
    }

    /// `ABM_NEW`. `callback` is the window message the shell uses for `ABN_*` notifications.
    pub(crate) fn register(&mut self, hwnd: HWND, callback: u32) -> bool {
        if self.registered {
            return true;
        }
        install_panic_hook();
        let mut d = data(hwnd);
        d.uCallbackMessage = callback;
        // FALSE if this window is already registered (e.g. a second call): treat as registered.
        unsafe { SHAppBarMessage(ABM_NEW, &mut d) };
        self.registered = true;
        self.last = None;
        REGISTERED.store(hwnd.0 as isize, Ordering::SeqCst);
        true
    }

    /// After Explorer restarted: its AppBar list is empty, register again.
    pub(crate) fn reregister(&mut self, hwnd: HWND, callback: u32) {
        if self.registered {
            self.registered = false;
            self.register(hwnd, callback);
        }
    }

    pub(crate) fn remove(&mut self, hwnd: HWND) {
        if !self.registered {
            return;
        }
        let mut d = data(hwnd);
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
        self.registered = false;
        self.last = None;
        REGISTERED.store(0, Ordering::SeqCst);
    }

    /// Negotiates a `height` px strip at the bottom of `monitor` (the shell moves it above the
    /// taskbar and other bottom AppBars) and reserves it. Returns the rectangle to occupy.
    pub(crate) fn place_bottom(&mut self, hwnd: HWND, monitor: RECT, height: i32) -> RECT {
        let mut d = data(hwnd);
        d.uEdge = ABE_BOTTOM;
        d.rc = RECT { top: monitor.bottom - height, ..monitor };
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut d) };
        d.rc.top = d.rc.bottom - height;
        if self.last != Some(d.rc) {
            let want = d.rc;
            unsafe { SHAppBarMessage(ABM_SETPOS, &mut d) };
            // SETPOS may adjust again; we take what we're given (height preserved from bottom).
            d.rc.top = d.rc.bottom.min(want.bottom) - height;
            self.last = Some(d.rc);
        }
        self.last.unwrap_or(d.rc)
    }

    pub(crate) fn window_pos_changed(&self, hwnd: HWND) {
        if self.registered {
            let mut d = data(hwnd);
            unsafe { SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut d) };
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.last = None;
    }
}

/// Removes a still-registered AppBar if the process panics (release builds abort on panic, so
/// `Drop` won't run). Without this the reserved strip stays until Explorer notices.
fn install_panic_hook() {
    if HOOKED.swap(true, Ordering::SeqCst) {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let raw = REGISTERED.swap(0, Ordering::SeqCst);
        if raw != 0 {
            let mut d = data(HWND(raw as *mut _));
            unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
        }
        prev(info);
    }));
}

/// `ABN_*` notification codes (wParam of the callback message).
pub(crate) const ABN_STATECHANGE: usize = 0;
pub(crate) const ABN_POSCHANGED: usize = 1;

