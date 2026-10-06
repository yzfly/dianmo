//! Focus tracking for auto show/hide (DESIGN.md §2 自动弹出 / §3 自动弹出（M2）).
//!
//! A background thread (COM MTA) registers a UI Automation focus-changed handler with a cache
//! request, so every focus event arrives with control type, read-only, password, … already
//! fetched in one cross-process round trip. Each change is classified into a [`FocusEvent`] and
//! posted to the UI thread with [`HostProxy::post`] (`App::on_event` gets a `Box<dyn Any>` that
//! downcasts to `FocusEvent`).
//!
//! "Was it a touch?" comes from two sources on the same thread:
//! - a low-level mouse hook: mouse events Windows synthesizes from touch/pen carry the
//!   `MI_WP_SIGNATURE` (0xFF515700) in `dwExtraInfo` (Win32 apps, Explorer, XAML);
//! - Raw Input from HID touch screens ([`rawtouch`], `RIDEV_INPUTSINK`): contact down/up with
//!   screen positions, also for apps that take `WM_POINTER` themselves and never get synthesized
//!   mouse events (Chromium/Edge/Electron). Messages arrive only while a finger is down.
//!
//! Neither ever calls UI Automation or another process (a blocked hook stalls the whole system's
//! mouse). Both cover tapping a field that already has focus (no focus event): a touch released
//! inside the focused editable element re-sends `Editable { by_touch }`; one released on the
//! taskbar while the taskbar already has focus re-sends `NotEditable { by_touch }`.
//!
//! Interacting with our own tray icon moves focus to the taskbar; that focus change is not
//! reported (it is the 点墨 UI, like our own windows), so opening the tray menu doesn't hide the
//! keyboard.
//!
//! Nothing polls: the thread sleeps in `GetMessage`, UIA calls the handler on its own threads.
//! With `DIANMO_FOCUS_LOG=<file>` every focus event and its raw properties are logged.

mod rawtouch;

use std::fs::File;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, VARIANT_FALSE, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_DISABLE_OLE1DDE, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, SAFEARRAY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::SafeArrayDestroy;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationElement,
    IUIAutomationFocusChangedEventHandler, IUIAutomationFocusChangedEventHandler_Impl, IUIAutomationTextPattern,
    UIA_AriaRolePropertyId, UIA_AutomationIdPropertyId, UIA_BoundingRectanglePropertyId, UIA_CONTROLTYPE_ID,
    UIA_ClassNamePropertyId, UIA_ComboBoxControlTypeId, UIA_ControlTypePropertyId, UIA_CustomControlTypeId,
    UIA_DataItemControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_GroupControlTypeId,
    UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_IsPasswordPropertyId, UIA_IsReadOnlyAttributeId,
    UIA_IsTextPatternAvailablePropertyId, UIA_IsValuePatternAvailablePropertyId, UIA_NativeWindowHandlePropertyId,
    UIA_PROPERTY_ID, UIA_PaneControlTypeId, UIA_ProcessIdPropertyId, UIA_SpinnerControlTypeId, UIA_TextPatternId,
    UIA_ValueIsReadOnlyPropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, CallNextHookEx, DispatchMessageW, FindWindowW, GetClientRect, GetForegroundWindow, GetWindowRect, GetCursorInfo, GetCursorPos, GWL_STYLE, GetClassNameW, GetMessageW, GetParent,
    GetWindowLongW, GetWindowThreadProcessId, HHOOK, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_QUIT,
    WM_RBUTTONDOWN,
    WindowFromPoint,
};
use windows::core::{BSTR, Interface, Ref, Result, implement, w};

use crate::clock::now_ms;
use crate::host::HostProxy;
use crate::sink::LAST_SEND_TICK;

/// What kind of text a focused field takes (best effort: UI Automation exposes little of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// Spinner / `input type=number` / Win32 `ES_NUMBER` edit.
    Number,
    Password,
    /// Browser address bar, Explorer address bar.
    Url,
    /// `role=searchbox` / `input type=search`, search boxes (Start, Explorer, apps).
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    /// An editable field got focus, or a field that already had focus was tapped again.
    /// `by_touch`: a touch/pen contact happened within about 500ms (only then should the keyboard
    /// pop up automatically, like the system touch keyboard).
    Editable { kind: FieldKind, by_touch: bool },
    /// Focus moved to something that isn't editable (or the focused window isn't a text field).
    NotEditable { by_touch: bool },
}

/// Recent-touch window for `by_touch`.
pub const TOUCH_WINDOW_MS: u64 = 500;

/// Running focus watcher. Dropping it stops the thread (unregisters the UIA handler and the hook).
pub struct FocusWatcher {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Drop for FocusWatcher {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl std::fmt::Debug for FocusWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusWatcher").field("thread_id", &self.thread_id).finish()
    }
}

/// Starts watching focus on a background MTA thread; events go to `App::on_event` through
/// `proxy`. The first event is always the element focused right now, with `by_touch: false`
/// (focus changes UIA reports while the handler is being registered are dropped, and touches
/// from before the start never count).
/// Returns once the UIA handler and the touch hook are installed (typically tens of ms).
pub fn start_focus_watcher(proxy: HostProxy) -> Result<FocusWatcher> {
    let (tx, rx) = mpsc::channel::<Result<u32>>();
    let join = std::thread::Builder::new()
        .name("dianmo-focus".into())
        .stack_size(256 * 1024)
        .spawn(move || watcher_thread(proxy, tx))
        .map_err(|_| windows::core::Error::from_thread())?;
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(FocusWatcher { thread_id, join: Some(join) }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            let _ = join.join();
            Err(windows::core::Error::from_thread())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Touch tracking (low-level mouse hook + raw input, both on the watcher thread)

const MI_WP_SIGNATURE: usize = 0xFF51_5700;
const SIGNATURE_MASK: usize = 0xFFFF_FF00;

/// `now_ms()` of the last touch/pen contact (down or up) outside our own windows; 0 = never.
static LAST_TOUCH_MS: AtomicU64 = AtomicU64::new(0);
/// `now_ms()` of the last mouse button press that did *not* come from touch/pen; 0 = never.
static LAST_MOUSE_MS: AtomicU64 = AtomicU64::new(0);
/// Screen position (packed, see [`pack`]) and `now_ms()` of the last press of any kind (mouse
/// button, touch or pen contact), to tell what a focus change on the taskbar was about.
static LAST_DOWN_PT: AtomicU64 = AtomicU64::new(0);
static LAST_DOWN_MS: AtomicU64 = AtomicU64::new(0);
/// `now_ms()` of the last re-sent event (a touch reaches us through the hook *and* raw input).
static LAST_RETAP_MS: AtomicU64 = AtomicU64::new(0);
/// `GetTickCount()` when the watcher started: input from before that never counts as a touch.
static START_TICK: AtomicU32 = AtomicU32::new(0);
/// Incremented on every touch down (one per tap), so a repeated focus event after a new tap is
/// not swallowed by the de-duplication.
static TOUCH_SEQ: AtomicU32 = AtomicU32::new(0);
/// Context for the hook (the hook procedure has no user data).
static HOOK_CTX: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

fn pack(pt: POINT) -> u64 {
    ((pt.x as u32 as u64) << 32) | pt.y as u32 as u64
}

fn unpack(v: u64) -> POINT {
    POINT { x: (v >> 32) as u32 as i32, y: v as u32 as i32 }
}

fn record_down(pt: POINT) {
    LAST_DOWN_PT.store(pack(pt), Ordering::Relaxed);
    LAST_DOWN_MS.store(now_ms().max(1), Ordering::Relaxed);
}

/// Where the user last pressed (within `max_age_ms`), else the cursor position.
fn last_press_point(max_age_ms: u64) -> POINT {
    let t = LAST_DOWN_MS.load(Ordering::Relaxed);
    if t != 0 && now_ms().saturating_sub(t) <= max_age_ms {
        return unpack(LAST_DOWN_PT.load(Ordering::Relaxed));
    }
    let mut pt = POINT::default();
    let _ = unsafe { GetCursorPos(&mut pt) };
    pt
}

/// Whether a top-level window at `pt` belongs to this process (keyboard, edge handle, menus).
fn own_window_at(pt: POINT, own_pid: u32) -> bool {
    let mut pid = 0;
    unsafe {
        let hwnd = WindowFromPoint(pt);
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid == own_pid
}

/// Position and time of the last contact raw input saw go down.
static LAST_RAW_DOWN_PT: AtomicU64 = AtomicU64::new(0);
static LAST_RAW_DOWN_MS: AtomicU64 = AtomicU64::new(0);

/// A touch/pen contact went down at `pt`, seen by raw input (`raw`) or the hook.
fn touch_down(pt: POINT, raw: bool) {
    if own_window_at(pt, unsafe { GetCurrentProcessId() }) {
        // Typing on the keyboard isn't a touch on the app (our SendInput is excluded likewise).
        return;
    }
    let now = now_ms().max(1);
    LAST_TOUCH_MS.store(now, Ordering::Relaxed);
    if raw {
        LAST_RAW_DOWN_PT.store(pack(pt), Ordering::Relaxed);
        LAST_RAW_DOWN_MS.store(now, Ordering::Relaxed);
    } else {
        // The mouse press Windows synthesizes for a touch comes up to a few hundred ms after
        // raw input reported the contact: same tap, not a new one.
        let r = unpack(LAST_RAW_DOWN_PT.load(Ordering::Relaxed));
        let t = LAST_RAW_DOWN_MS.load(Ordering::Relaxed);
        if t != 0 && now - t.min(now) <= 600 && (r.x - pt.x).abs() <= 24 && (r.y - pt.y).abs() <= 24 {
            return;
        }
    }
    TOUCH_SEQ.fetch_add(1, Ordering::Relaxed);
    record_down(pt);
}

/// A touch/pen contact was released at `pt` (hook or raw input).
fn touch_up(pt: POINT, source: &str) {
    if let Ok(guard) = HOOK_CTX.try_lock()
        && let Some(shared) = guard.as_ref()
    {
        if own_window_at(pt, shared.own_pid) {
            return;
        }
        LAST_TOUCH_MS.store(now_ms().max(1), Ordering::Relaxed);
        shared.touch_up(pt, source);
    }
}

/// Writes to the focus log from the watcher thread (never blocks).
fn log_line(line: std::fmt::Arguments) {
    if let Ok(guard) = HOOK_CTX.try_lock()
        && let Some(shared) = guard.as_ref()
    {
        shared.log(line);
    }
}

/// Whether a touch/pen contact happened within [`TOUCH_WINDOW_MS`] (and after the watcher
/// started). Sources:
/// - touch down/up seen by the hook (synthesized mouse) or raw input;
/// - for apps that take `WM_POINTER` themselves (Chromium/Edge/Electron) when raw input isn't
///   available: the system cursor is *suppressed* (`CURSOR_SUPPRESSED`, Windows 8+) after
///   touch/pen input until the mouse moves again, and the last input event is recent (and was
///   not our own `SendInput`, nor a mouse click). A physical key press right after a touch also
///   counts as touch.
fn touched_recently() -> bool {
    let now = now_ms();
    let t = LAST_TOUCH_MS.load(Ordering::Relaxed);
    if t != 0 && now.saturating_sub(t) <= TOUCH_WINDOW_MS {
        return true;
    }
    // A mouse click is the newest press: the cursor may still be suppressed from an earlier
    // touch (clicking doesn't move the mouse), but this wasn't a touch.
    let m = LAST_MOUSE_MS.load(Ordering::Relaxed);
    if m != 0 && m >= t && now.saturating_sub(m) <= TOUCH_WINDOW_MS {
        return false;
    }
    let (suppressed, age) = touch_signals();
    suppressed && age <= TOUCH_WINDOW_MS as u32
}

/// (`CURSOR_SUPPRESSED`, ms since the last input event of any kind). The age is `u32::MAX` when
/// the last input was our own `SendInput` or happened before the watcher started.
fn touch_signals() -> (bool, u32) {
    const CURSOR_SUPPRESSED: u32 = 0x2;
    unsafe {
        let mut ci = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
        let suppressed = GetCursorInfo(&mut ci).is_ok() && ci.flags.0 & CURSOR_SUPPRESSED != 0;
        let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if !GetLastInputInfo(&mut li).as_bool() {
            return (suppressed, u32::MAX);
        }
        // The last input was our own SendInput (typing, Enter, Win+H), not a touch on the app.
        let sent = LAST_SEND_TICK.load(Ordering::Relaxed);
        if sent != 0 && (li.dwTime.wrapping_sub(sent) as i32).abs() <= 32 {
            return (false, u32::MAX);
        }
        // E.g. the tap that launched us: not a reason to report the first focus as touched.
        if (li.dwTime.wrapping_sub(START_TICK.load(Ordering::Relaxed)) as i32) < 0 {
            return (suppressed, u32::MAX);
        }
        (suppressed, GetTickCount().wrapping_sub(li.dwTime))
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wp.0 as u32;
        if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN) {
            let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
            let touch = info.dwExtraInfo & SIGNATURE_MASK == MI_WP_SIGNATURE;
            match (msg, touch) {
                (WM_LBUTTONDOWN, true) => {
                    rawtouch::calibrate(info.pt);
                    touch_down(info.pt, false);
                }
                (WM_LBUTTONUP, true) => touch_up(info.pt, "hook"),
                (WM_RBUTTONDOWN, true) => record_down(info.pt), // press-and-hold
                (WM_LBUTTONDOWN | WM_RBUTTONDOWN, false) => {
                    LAST_MOUSE_MS.store(now_ms().max(1), Ordering::Relaxed);
                    record_down(info.pt);
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

// ---------------------------------------------------------------------------------------------
// Taskbar and our tray icon

/// Process id of the shell (Explorer, owner of the taskbar); 0 if there is no taskbar.
fn shell_pid() -> u32 {
    let mut pid = 0;
    unsafe {
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) {
            GetWindowThreadProcessId(tray, Some(&mut pid));
        }
    }
    pid
}

/// Window classes of Explorer's taskbars.
fn is_taskbar_class(class: &str) -> bool {
    matches!(class, "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

/// The notification-area window at `pt` (tray icons, chevron, clock; the hidden-icons flyout):
/// `Some(is_chevron)`, or `None` elsewhere. Only window-manager calls (no messages sent).
fn notification_area_at(pt: POINT) -> Option<bool> {
    let hwnd = unsafe { WindowFromPoint(pt) };
    if hwnd.is_invalid() {
        return None;
    }
    let chevron = win_class(hwnd) == "Button";
    let mut h = hwnd;
    for _ in 0..5 {
        match win_class(h).as_str() {
            "TrayNotifyWnd" => return Some(chevron),
            "NotifyIconOverflowWindow" => return Some(false),
            "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" => return None,
            _ => {}
        }
        match unsafe { GetParent(h) } {
            Ok(p) if !p.is_invalid() => h = p,
            _ => break,
        }
    }
    None
}

/// Whether a press at `pt` was on our own tray icon, or on the "show hidden icons" chevron while
/// our icon sits in the hidden-icons flyout (the way to reach it). Asks the shell for the icon's
/// position (a cross-process call: UIA threads only, never the hook thread).
fn on_own_tray_icon(pt: POINT) -> bool {
    let Some(chevron) = notification_area_at(pt) else { return false };
    match crate::tray::icon_rect() {
        Some(rc) => {
            let inside = pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom;
            // The icon's rect is in the flyout while that is open; on the taskbar otherwise.
            inside || (chevron && !crate::tray::icon_on_taskbar(rc))
        }
        // No rect: the icon is in the closed hidden-icons flyout.
        None => chevron,
    }
}

// ---------------------------------------------------------------------------------------------
// Shared state

#[derive(Default)]
struct State {
    last_id: Vec<i32>,
    last: Option<FocusEvent>,
    last_seq: u32,
    /// Bounding rectangle (screen px) and kind of the focused editable element.
    field: Option<(RECT, FieldKind)>,
    /// Bounding rectangle of the taskbar when it has focus (tapping it again re-sends
    /// `NotEditable`: the focus doesn't change, but the user tapped outside any text field).
    taskbar: Option<RECT>,
}

struct Shared {
    proxy: HostProxy,
    own_pid: u32,
    /// Set once the initial focus event was posted; earlier handler calls are dropped.
    ready: AtomicBool,
    state: Mutex<State>,
    log: Option<Mutex<File>>,
}

impl Shared {
    fn log(&self, line: std::fmt::Arguments) {
        if let Some(f) = &self.log
            && let Ok(mut f) = f.lock()
        {
            let _ = writeln!(f, "{} {}", now_ms(), line);
        }
    }

    /// From the watcher thread (hook or raw input): a touch was released at `pt`, not on our own
    /// windows. Tapping the focused field (or the focused taskbar) again doesn't move focus, so
    /// re-send its event. Must not block: no UIA, no cross-process calls.
    fn touch_up(&self, pt: POINT, source: &str) {
        let inside = |rc: &RECT| pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom;
        let Ok(mut st) = self.state.try_lock() else { return };
        let ev = match (st.field, st.taskbar) {
            (Some((rc, kind)), _) if inside(&rc) => FocusEvent::Editable { kind, by_touch: true },
            // Not the notification area: our tray icon (and the menu it opens) must not hide us.
            (None, Some(rc)) if inside(&rc) && notification_area_at(pt).is_none() => {
                FocusEvent::NotEditable { by_touch: true }
            }
            _ => return,
        };
        // The focus event for this very tap already said so (a tap that moved focus).
        let seq = TOUCH_SEQ.load(Ordering::Relaxed);
        if st.last == Some(ev) && st.last_seq == seq {
            return;
        }
        // The same release often arrives twice (synthesized mouse and raw input).
        let now = now_ms();
        if now.saturating_sub(LAST_RETAP_MS.swap(now, Ordering::Relaxed)) < 250 {
            return;
        }
        st.last = Some(ev);
        st.last_seq = seq;
        drop(st);
        self.log(format_args!("retap({source}) {ev:?} at ({},{})", pt.x, pt.y));
        self.proxy.post(ev);
    }

    fn on_focus(&self, el: &IUIAutomationElement, initial: bool) {
        let p = Props::read(el);
        if p.pid == self.own_pid || p.pid == 0 && p.control_type == 0 {
            return;
        }
        let by_touch = !initial && touched_recently();
        // Consoles (conhost, Windows Terminal) report their text area and their window in turn,
        // in any order, and taps inside don't move focus: the whole window counts as one field.
        let console = console_window(&p);
        let kind = if console.is_some() { Some(FieldKind::Text) } else { classify(el, &p) };
        if kind.is_none() && !initial && p.pid == shell_pid() {
            // Our tray icon (tap = show/hide, press-and-hold = menu) focuses the taskbar: that's
            // our own UI, not leaving the text field.
            let pt = last_press_point(1500);
            if on_own_tray_icon(pt) {
                self.log(format_args!("tray icon at ({},{}), ignored: {}", pt.x, pt.y, p.describe()));
                return;
            }
            if self.log.is_some() {
                let mut chain = Vec::new();
                let mut h = unsafe { WindowFromPoint(pt) };
                for _ in 0..6 {
                    if h.is_invalid() {
                        break;
                    }
                    chain.push(win_class(h));
                    h = unsafe { GetParent(h) }.unwrap_or_default();
                }
                self.log(format_args!(
                    "shell focus, press at ({},{}) on {:?}, notification area {:?}, icon {:?}",
                    pt.x,
                    pt.y,
                    chain,
                    notification_area_at(pt),
                    crate::tray::icon_rect().map(|r| (r.left, r.top, r.right, r.bottom))
                ));
            }
        }
        let ev = match kind {
            Some(kind) => FocusEvent::Editable { kind, by_touch },
            None => FocusEvent::NotEditable { by_touch },
        };
        let id = runtime_id(el);
        let seq = TOUCH_SEQ.load(Ordering::Relaxed);
        {
            let Ok(mut st) = self.state.lock() else { return };
            let same_target = st.last_id == id || matches!(ev, FocusEvent::NotEditable { .. });
            let fresh_tap = by_touch && seq != st.last_seq;
            if same_target && st.last == Some(ev) && !fresh_tap {
                drop(st);
                self.log(format_args!("dup {ev:?} {}", p.describe()));
                return;
            }
            st.last_id = id;
            st.last = Some(ev);
            st.last_seq = seq;
            st.field = kind.map(|k| (console.unwrap_or(p.rect), k));
            st.taskbar = (kind.is_none() && is_taskbar_class(&p.class)).then_some(p.rect);
        }
        if self.log.is_some() {
            let (suppressed, age) = touch_signals();
            let mut cur = POINT::default();
            let _ = unsafe { GetCursorPos(&mut cur) };
            let age_of = |t: u64| if t == 0 { -1 } else { now_ms().saturating_sub(t) as i64 };
            let down = unpack(LAST_DOWN_PT.load(Ordering::Relaxed));
            self.log(format_args!(
                "{ev:?} {} | touch_age={} mouse_age={} suppressed={} input_age={} cursor=({},{}) down=({},{}) down_age={}",
                p.describe(),
                age_of(LAST_TOUCH_MS.load(Ordering::Relaxed)),
                age_of(LAST_MOUSE_MS.load(Ordering::Relaxed)),
                suppressed as u8,
                age as i64,
                cur.x,
                cur.y,
                down.x,
                down.y,
                age_of(LAST_DOWN_MS.load(Ordering::Relaxed)),
            ));
        }
        self.proxy.post(ev);
    }
}

// ---------------------------------------------------------------------------------------------
// Classification

const PROPS: [UIA_PROPERTY_ID; 13] = [
    UIA_ProcessIdPropertyId,
    UIA_ControlTypePropertyId,
    UIA_IsEnabledPropertyId,
    UIA_IsKeyboardFocusablePropertyId,
    UIA_IsPasswordPropertyId,
    UIA_IsValuePatternAvailablePropertyId,
    UIA_ValueIsReadOnlyPropertyId,
    UIA_IsTextPatternAvailablePropertyId,
    UIA_ClassNamePropertyId,
    UIA_AutomationIdPropertyId,
    UIA_AriaRolePropertyId,
    UIA_NativeWindowHandlePropertyId,
    UIA_BoundingRectanglePropertyId,
];

#[derive(Debug, Default)]
struct Props {
    pid: u32,
    control_type: i32,
    enabled: bool,
    focusable: bool,
    password: bool,
    value_pattern: bool,
    value_read_only: bool,
    text_pattern: bool,
    class: String,
    automation_id: String,
    aria_role: String,
    hwnd: isize,
    rect: RECT,
}

fn bstr(r: Result<BSTR>) -> String {
    r.map(|b| b.to_string()).unwrap_or_default()
}

impl Props {
    fn read(el: &IUIAutomationElement) -> Self {
        unsafe {
            let value_pattern = el.GetCachedPropertyValue(UIA_IsValuePatternAvailablePropertyId).map(variant_bool);
            let value_ro = el.GetCachedPropertyValue(UIA_ValueIsReadOnlyPropertyId).map(variant_bool);
            let text_pattern = el.GetCachedPropertyValue(UIA_IsTextPatternAvailablePropertyId).map(variant_bool);
            Self {
                pid: el.CachedProcessId().unwrap_or(0) as u32,
                control_type: el.CachedControlType().map(|c| c.0).unwrap_or(0),
                enabled: el.CachedIsEnabled().map(|b| b.as_bool()).unwrap_or(true),
                focusable: el.CachedIsKeyboardFocusable().map(|b| b.as_bool()).unwrap_or(true),
                password: el.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(false),
                value_pattern: matches!(value_pattern, Ok(Some(true))),
                value_read_only: matches!(value_ro, Ok(Some(true))),
                text_pattern: matches!(text_pattern, Ok(Some(true))),
                class: bstr(el.CachedClassName()),
                automation_id: bstr(el.CachedAutomationId()),
                aria_role: bstr(el.CachedAriaRole()),
                hwnd: el.CachedNativeWindowHandle().map(|h| h.0 as isize).unwrap_or(0),
                rect: el.CachedBoundingRectangle().unwrap_or_default(),
            }
        }
    }

    fn describe(&self) -> String {
        format!(
            "pid={} ct={} en={} kf={} pw={} vp={} ro={} tp={} class={:?} aid={:?} aria={:?} hwnd={:#x} rect=({},{})-({},{})",
            self.pid,
            self.control_type,
            self.enabled as u8,
            self.focusable as u8,
            self.password as u8,
            self.value_pattern as u8,
            self.value_read_only as u8,
            self.text_pattern as u8,
            self.class,
            self.automation_id,
            self.aria_role,
            self.hwnd,
            self.rect.left,
            self.rect.top,
            self.rect.right,
            self.rect.bottom
        )
    }
}

/// `Some(b)` for a `VT_BOOL` variant (clears the variant).
fn variant_bool(mut v: VARIANT) -> Option<bool> {
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let r = (inner.vt == VT_BOOL).then(|| inner.Anonymous.boolVal != VARIANT_FALSE);
        let _ = VariantClear(&mut v);
        r
    }
}

fn is(ct: i32, id: UIA_CONTROLTYPE_ID) -> bool {
    ct == id.0
}

/// Editable or not, and what kind. Mostly from cached properties; a caret check through the text
/// pattern (one more cross-process call) only for documents and containers that may be editable.
fn classify(el: &IUIAutomationElement, p: &Props) -> Option<FieldKind> {
    if !p.enabled {
        return None;
    }
    let ct = p.control_type;
    let writable_value = p.value_pattern && !p.value_read_only;
    let editable = if is(ct, UIA_EditControlTypeId) {
        // Edits without a value pattern (some rich edits) are taken as editable unless their
        // caret says read-only.
        if p.value_pattern { !p.value_read_only } else { !p.text_pattern || caret_writable(el) != Some(false) }
    } else if is(ct, UIA_SpinnerControlTypeId) || is(ct, UIA_ComboBoxControlTypeId) {
        writable_value
    } else if is(ct, UIA_DataItemControlTypeId) {
        // Spreadsheet cells.
        writable_value && p.focusable
    } else if is(ct, UIA_DocumentControlTypeId)
        || is(ct, UIA_GroupControlTypeId)
        || is(ct, UIA_CustomControlTypeId)
        || is(ct, UIA_PaneControlTypeId)
    {
        // Notepad / Word / contenteditable / web page body: editable iff there is a writable
        // caret. A writable value pattern alone counts for documents (Win32 multi-line edits).
        if p.text_pattern {
            match caret_writable(el) {
                Some(w) => w,
                None => writable_value && is(ct, UIA_DocumentControlTypeId),
            }
        } else {
            writable_value && is(ct, UIA_DocumentControlTypeId)
        }
    } else {
        false
    };
    if !editable && !(p.password && is(ct, UIA_EditControlTypeId)) {
        return None;
    }
    Some(kind_of(p))
}

fn kind_of(p: &Props) -> FieldKind {
    if p.password {
        return FieldKind::Password;
    }
    let aria = p.aria_role.to_ascii_lowercase();
    if is(p.control_type, UIA_SpinnerControlTypeId) || aria == "spinbutton" {
        return FieldKind::Number;
    }
    if aria == "searchbox" {
        return FieldKind::Search;
    }
    if p.class == "OmniboxViewViews" || aria == "url" {
        return FieldKind::Url;
    }
    let hwnd = HWND(p.hwnd as *mut _);
    if p.hwnd != 0 {
        if win_class(hwnd).eq_ignore_ascii_case("Edit") {
            const ES_NUMBER: i32 = 0x2000;
            if unsafe { GetWindowLongW(hwnd, GWL_STYLE) } & ES_NUMBER != 0 {
                return FieldKind::Number;
            }
        }
        // Explorer's address bar: Edit in ComboBoxEx32 in "Address Band Root".
        let mut h = hwnd;
        for _ in 0..6 {
            let Ok(parent) = (unsafe { GetParent(h) }) else { break };
            if parent.is_invalid() {
                break;
            }
            match win_class(parent).as_str() {
                "Address Band Root" => return FieldKind::Url,
                "UniversalSearchBand" | "SearchEditBoxWrapperClass" => return FieldKind::Search,
                _ => {}
            }
            h = parent;
        }
    }
    let aid = p.automation_id.to_ascii_lowercase();
    let class = p.class.to_ascii_lowercase();
    if aid.contains("search") || class.contains("search") {
        return FieldKind::Search;
    }
    FieldKind::Text
}

/// Top-level window classes of consoles: conhost (PowerShell, cmd) and Windows Terminal.
fn is_console_class(class: &str) -> bool {
    matches!(class, "ConsoleWindowClass" | "CASCADIA_HOSTING_WINDOW_CLASS")
}

/// If the focused element is a console window or lies inside the foreground console window: that
/// window's client area (screen px).
fn console_window(p: &Props) -> Option<RECT> {
    unsafe {
        let own = HWND(p.hwnd as *mut _);
        let hwnd = if p.hwnd != 0 && is_console_class(&win_class(own)) {
            own
        } else {
            let fg = GetForegroundWindow();
            if fg.is_invalid() || !is_console_class(&win_class(fg)) {
                return None;
            }
            let mut wr = RECT::default();
            GetWindowRect(fg, &mut wr).ok()?;
            let r = p.rect;
            let inside = r.left >= wr.left && r.top >= wr.top && r.right <= wr.right && r.bottom <= wr.bottom;
            if !inside || r.right <= r.left {
                return None;
            }
            fg
        };
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc).ok()?;
        let mut tl = POINT { x: rc.left, y: rc.top };
        let mut br = POINT { x: rc.right, y: rc.bottom };
        let _ = ClientToScreen(hwnd, &mut tl);
        let _ = ClientToScreen(hwnd, &mut br);
        Some(RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y })
    }
}

fn win_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Whether the element's caret (first selection range) is in writable text. `None` when it
/// can't tell (no text pattern, attribute not supported). No selection → not editable.
fn caret_writable(el: &IUIAutomationElement) -> Option<bool> {
    unsafe {
        let tp: IUIAutomationTextPattern = el.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
        let sel = tp.GetSelection().ok()?;
        if sel.Length().ok()? <= 0 {
            return Some(false);
        }
        let range = sel.GetElement(0).ok()?;
        let v = range.GetAttributeValue(UIA_IsReadOnlyAttributeId).ok()?;
        variant_bool(v).map(|ro| !ro)
    }
}

fn runtime_id(el: &IUIAutomationElement) -> Vec<i32> {
    unsafe {
        let Ok(sa) = el.GetRuntimeId() else { return Vec::new() };
        if sa.is_null() {
            return Vec::new();
        }
        let a: &SAFEARRAY = &*sa;
        let n = if a.cDims == 1 && a.cbElements == 4 { a.rgsabound[0].cElements as usize } else { 0 };
        let v = if n > 0 && !a.pvData.is_null() {
            std::slice::from_raw_parts(a.pvData as *const i32, n).to_vec()
        } else {
            Vec::new()
        };
        let _ = SafeArrayDestroy(sa);
        v
    }
}

// ---------------------------------------------------------------------------------------------
// UIA handler and thread

#[implement(IUIAutomationFocusChangedEventHandler)]
struct FocusHandler {
    shared: Arc<Shared>,
}

impl IUIAutomationFocusChangedEventHandler_Impl for FocusHandler_Impl {
    fn HandleFocusChangedEvent(&self, sender: Ref<IUIAutomationElement>) -> Result<()> {
        if let Ok(el) = sender.ok() {
            if !self.shared.ready.load(Ordering::Acquire) {
                // Raised while registering (often for a stale element); the initial event that
                // follows reports what has focus now.
                self.shared.log(format_args!("early focus event dropped"));
                return Ok(());
            }
            self.shared.on_focus(el, false);
        }
        Ok(())
    }
}

fn cache_request(uia: &IUIAutomation) -> Result<IUIAutomationCacheRequest> {
    unsafe {
        let cache = uia.CreateCacheRequest()?;
        for p in PROPS {
            cache.AddProperty(p)?;
        }
        Ok(cache)
    }
}

fn watcher_thread(proxy: HostProxy, ready: mpsc::Sender<Result<u32>>) {
    unsafe {
        // Create the message queue before reporting the thread id (PostThreadMessage needs it).
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        START_TICK.store(GetTickCount(), Ordering::Relaxed);
        let com = CoInitializeEx(None, COINIT_MULTITHREADED | COINIT_DISABLE_OLE1DDE);
        if com.is_err() {
            let _ = ready.send(Err(com.into()));
            return;
        }
        type Setup = (IUIAutomation, IUIAutomationCacheRequest, IUIAutomationFocusChangedEventHandler, Arc<Shared>);
        let setup = || -> Result<Setup> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))?;
            if let Ok(u2) = uia.cast::<IUIAutomation2>() {
                // A hung target app must not stall us for long.
                let _ = u2.SetConnectionTimeout(1000);
                let _ = u2.SetTransactionTimeout(1500);
            }
            let cache = cache_request(&uia)?;
            let log = std::env::var_os("DIANMO_FOCUS_LOG")
                .and_then(|p| File::options().create(true).append(true).open(p).ok())
                .map(Mutex::new);
            let shared = Arc::new(Shared {
                proxy,
                own_pid: GetCurrentProcessId(),
                ready: AtomicBool::new(false),
                state: Mutex::default(),
                log,
            });
            let handler: IUIAutomationFocusChangedEventHandler = FocusHandler { shared: shared.clone() }.into();
            uia.AddFocusChangedEventHandler(&cache, &handler)?;
            Ok((uia, cache, handler, shared))
        };
        let (uia, cache, handler, shared) = match setup() {
            Ok(v) => v,
            Err(e) => {
                let _ = ready.send(Err(e));
                CoUninitialize();
                return;
            }
        };
        let _ = ready.send(Ok(GetCurrentThreadId()));
        shared.log(format_args!("started"));

        // Report what has focus right now. Cross-process, so before the hook exists: from then on
        // this thread must never block (a stalled low-level hook lags the mouse system-wide).
        if let Ok(el) = uia.GetFocusedElementBuildCache(&cache) {
            shared.on_focus(&el, true);
        }
        shared.ready.store(true, Ordering::Release);
        *HOOK_CTX.lock().unwrap_or_else(|e| e.into_inner()) = Some(shared.clone());
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), GetModuleHandleW(None).ok().map(Into::into), 0)
            .unwrap_or_else(|e| {
                shared.log(format_args!("mouse hook failed: {e}"));
                HHOOK::default()
            });
        let raw = rawtouch::RawTouch::start();
        shared.log(format_args!("raw touch input: {}", if raw.is_some() { "registered" } else { "unavailable" }));

        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            DispatchMessageW(&msg);
        }

        drop(raw);
        if !hook.is_invalid() {
            let _ = UnhookWindowsHookEx(hook);
        }
        *HOOK_CTX.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = uia.RemoveFocusChangedEventHandler(&handler);
        shared.log(format_args!("stopped"));
        drop((handler, cache, uia, shared));
        CoUninitialize();
    }
}
