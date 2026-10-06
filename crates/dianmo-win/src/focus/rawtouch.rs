//! Touch contacts from Raw Input (HID touch screens, Usage Page 0x0D Digitizer / Usage 0x04
//! Touch Screen, `RIDEV_INPUTSINK`), on the focus watcher thread.
//!
//! Chromium/Edge/Electron consume `WM_POINTER` themselves, so the low-level mouse hook never sees
//! their touches; raw input sees every contact on the screen whichever app gets it. A message-only
//! window receives `WM_INPUT` only while a finger is down: idle cost stays zero.
//!
//! Reports are parsed with the HID parser (`hid.dll`, loaded on the first report so the exe's
//! import table doesn't change): every finger is a link collection with Tip Switch (0x0D/0x42),
//! Contact Identifier (0x0D/0x51) and X/Y (0x01/0x30, 0x31); Contact Count (0x0D/0x54) says how
//! many fingers of a report are valid (hybrid mode spreads contacts over several reports).
//! Logical X/Y are normalized and mapped onto the display the digitizer belongs to
//! (`GetPointerDevices`), rotated by the display orientation. Touches Windows also turns into
//! mouse events (Win32 apps) calibrate the rotation if it ever disagrees.
//!
//! On the Surface, `InjectTouchInput` contacts arrive here too (from the `VIRTUAL_DIGITIZER`
//! device, logical range = screen pixels), so tests exercise this path.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_REPORT_TYPE, HIDP_VALUE_CAPS, HidP_Input, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::Controls::{POINTER_DEVICE_INFO, POINTER_DEVICE_TYPE_TOUCH};
use windows::Win32::UI::Input::Pointer::{GetPointerDeviceRects, GetPointerDevices};
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK,
    RIDEV_REMOVE, RIDI_PREPARSEDDATA, RIM_TYPEHID, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_INPUT, WNDCLASSW,
};
use windows::core::{PCSTR, s, w};

use super::{log_line, touch_down, touch_up};
use crate::clock::now_ms;

/// The registered message-only window. Create and drop on the watcher thread.
pub(super) struct RawTouch {
    hwnd: HWND,
}

impl RawTouch {
    /// Creates the window and registers for touch-screen raw input. `None` if that fails.
    pub(super) fn start() -> Option<Self> {
        unsafe {
            let instance = GetModuleHandleW(None).ok()?.into();
            let class = WNDCLASSW {
                lpfnWndProc: Some(raw_proc),
                hInstance: instance,
                lpszClassName: w!("DianmoRawTouch"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("DianmoRawTouch"),
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
            .ok()?;
            let dev = RAWINPUTDEVICE { usUsagePage: 0x0D, usUsage: 0x04, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
            if let Err(e) = RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32) {
                log_line(format_args!("raw input registration failed: {e}"));
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(Self { hwnd })
        }
    }
}

impl Drop for RawTouch {
    fn drop(&mut self) {
        unsafe {
            let dev =
                RAWINPUTDEVICE { usUsagePage: 0x0D, usUsage: 0x04, dwFlags: RIDEV_REMOVE, hwndTarget: HWND::default() };
            let _ = RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32);
            let _ = DestroyWindow(self.hwnd);
        }
        STATE.with(|s| *s.borrow_mut() = State::default());
    }
}

unsafe extern "system" fn raw_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        on_input(HRAWINPUT(lp.0 as *mut _));
    }
    // DefWindowProc does the WM_INPUT cleanup.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

// ---------------------------------------------------------------------------------------------
// hid.dll

const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

type GetCapsFn = unsafe extern "system" fn(PHIDP_PREPARSED_DATA, *mut HIDP_CAPS) -> i32;
type GetValueCapsFn =
    unsafe extern "system" fn(HIDP_REPORT_TYPE, *mut HIDP_VALUE_CAPS, *mut u16, PHIDP_PREPARSED_DATA) -> i32;
type GetButtonCapsFn =
    unsafe extern "system" fn(HIDP_REPORT_TYPE, *mut HIDP_BUTTON_CAPS, *mut u16, PHIDP_PREPARSED_DATA) -> i32;
type GetUsageValueFn =
    unsafe extern "system" fn(HIDP_REPORT_TYPE, u16, u16, u16, *mut u32, PHIDP_PREPARSED_DATA, *mut u8, u32) -> i32;
type GetUsageValueArrayFn =
    unsafe extern "system" fn(HIDP_REPORT_TYPE, u16, u16, u16, *mut u8, u16, PHIDP_PREPARSED_DATA, *mut u8, u32) -> i32;
type GetUsagesFn =
    unsafe extern "system" fn(HIDP_REPORT_TYPE, u16, u16, *mut u16, *mut u32, PHIDP_PREPARSED_DATA, *mut u8, u32) -> i32;

/// What `GetProcAddress` returns.
type RawProc = unsafe extern "system" fn() -> isize;

struct Hid {
    get_caps: GetCapsFn,
    get_value_caps: GetValueCapsFn,
    get_button_caps: GetButtonCapsFn,
    get_usage_value: GetUsageValueFn,
    get_usage_value_array: GetUsageValueArrayFn,
    get_usages: GetUsagesFn,
}

fn hid() -> Option<&'static Hid> {
    static HID: OnceLock<Option<Hid>> = OnceLock::new();
    HID.get_or_init(|| unsafe {
        let m = LoadLibraryExW(w!("hid.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let f = |name: PCSTR| GetProcAddress(m, name);
        Some(Hid {
            get_caps: std::mem::transmute::<RawProc, GetCapsFn>(f(s!("HidP_GetCaps"))?),
            get_value_caps: std::mem::transmute::<RawProc, GetValueCapsFn>(f(s!("HidP_GetValueCaps"))?),
            get_button_caps: std::mem::transmute::<RawProc, GetButtonCapsFn>(f(s!("HidP_GetButtonCaps"))?),
            get_usage_value: std::mem::transmute::<RawProc, GetUsageValueFn>(f(s!("HidP_GetUsageValue"))?),
            get_usage_value_array: std::mem::transmute::<RawProc, GetUsageValueArrayFn>(f(s!("HidP_GetUsageValueArray"))?),
            get_usages: std::mem::transmute::<RawProc, GetUsagesFn>(f(s!("HidP_GetUsages"))?),
        })
    })
    .as_ref()
}

// ---------------------------------------------------------------------------------------------
// Devices

const PAGE_GENERIC: u16 = 0x01;
const PAGE_DIGITIZER: u16 = 0x0D;
const USAGE_X: u16 = 0x30;
const USAGE_Y: u16 = 0x31;
const USAGE_TIP: u16 = 0x42;
const USAGE_CONTACT_ID: u16 = 0x51;
const USAGE_CONTACT_COUNT: u16 = 0x54;

#[derive(Clone, Copy)]
struct Axis {
    min: i64,
    max: i64,
    /// Value array (report count > 1): read with `HidP_GetUsageValueArray`, first element.
    array: Option<(u16, u16)>, // (bit size, report count)
}

impl Axis {
    fn new(c: &HIDP_VALUE_CAPS) -> Self {
        let bits = c.BitSize.clamp(1, 32) as u32;
        let mask = if bits >= 32 { u32::MAX as i64 } else { (1i64 << bits) - 1 };
        let min = c.LogicalMin as i64;
        let mut max = c.LogicalMax as i64;
        if max <= min {
            // Logical maximum with the sign bit set, meant unsigned.
            max = (c.LogicalMax as u32 as i64) & mask;
        }
        let array = (c.ReportCount > 1).then_some((c.BitSize, c.ReportCount));
        Self { min, max, array }
    }

    fn normalize(&self, v: u32) -> f32 {
        if self.max <= self.min {
            return 0.0;
        }
        ((v as i64 - self.min) as f32 / (self.max - self.min) as f32).clamp(0.0, 1.0)
    }
}

struct Finger {
    link: u16,
    x: Axis,
    y: Axis,
    has_id: bool,
}

struct Device {
    preparsed: Vec<u64>,
    fingers: Vec<Finger>,
    has_count: bool,
    /// Valid contacts still expected in the following reports (hybrid mode).
    remaining: usize,
}

impl Device {
    fn pp(&self) -> PHIDP_PREPARSED_DATA {
        PHIDP_PREPARSED_DATA(self.preparsed.as_ptr() as isize)
    }

    fn load(hid: &Hid, handle: HANDLE) -> Option<Self> {
        unsafe {
            let mut size = 0u32;
            GetRawInputDeviceInfoW(Some(handle), RIDI_PREPARSEDDATA, None, &mut size);
            if size == 0 || size > 1 << 20 {
                return None;
            }
            let mut preparsed = vec![0u64; (size as usize).div_ceil(8)];
            let got = GetRawInputDeviceInfoW(Some(handle), RIDI_PREPARSEDDATA, Some(preparsed.as_mut_ptr() as _), &mut size);
            if got == u32::MAX || got == 0 {
                return None;
            }
            let pp = PHIDP_PREPARSED_DATA(preparsed.as_ptr() as isize);
            let mut caps = HIDP_CAPS::default();
            if (hid.get_caps)(pp, &mut caps) != HIDP_STATUS_SUCCESS {
                return None;
            }
            let mut n = caps.NumberInputValueCaps;
            let mut values = vec![HIDP_VALUE_CAPS::default(); n as usize];
            if n == 0 || (hid.get_value_caps)(HidP_Input, values.as_mut_ptr(), &mut n, pp) != HIDP_STATUS_SUCCESS {
                return None;
            }
            values.truncate(n as usize);
            let mut n = caps.NumberInputButtonCaps;
            let mut buttons = vec![HIDP_BUTTON_CAPS::default(); n as usize];
            if n > 0 && (hid.get_button_caps)(HidP_Input, buttons.as_mut_ptr(), &mut n, pp) != HIDP_STATUS_SUCCESS {
                n = 0;
            }
            buttons.truncate(n as usize);

            let usage = |c: &HIDP_VALUE_CAPS| {
                if c.IsRange { c.Anonymous.Range.UsageMin } else { c.Anonymous.NotRange.Usage }
            };
            let find = |link: u16, page: u16, u: u16| {
                values.iter().find(|c| c.LinkCollection == link && c.UsagePage == page && usage(c) == u)
            };
            let has_tip = |link: u16| {
                buttons.iter().any(|b| {
                    let (lo, hi) = if b.IsRange {
                        (b.Anonymous.Range.UsageMin, b.Anonymous.Range.UsageMax)
                    } else {
                        (b.Anonymous.NotRange.Usage, b.Anonymous.NotRange.Usage)
                    };
                    b.LinkCollection == link && b.UsagePage == PAGE_DIGITIZER && (lo..=hi).contains(&USAGE_TIP)
                })
            };
            let mut links: Vec<u16> = values.iter().map(|c| c.LinkCollection).collect();
            links.sort_unstable();
            links.dedup();
            let fingers: Vec<Finger> = links
                .into_iter()
                .filter_map(|link| {
                    let x = find(link, PAGE_GENERIC, USAGE_X)?;
                    let y = find(link, PAGE_GENERIC, USAGE_Y)?;
                    has_tip(link).then(|| Finger {
                        link,
                        x: Axis::new(x),
                        y: Axis::new(y),
                        has_id: find(link, PAGE_DIGITIZER, USAGE_CONTACT_ID).is_some(),
                    })
                })
                .collect();
            if fingers.is_empty() {
                return None;
            }
            let has_count = values.iter().any(|c| c.UsagePage == PAGE_DIGITIZER && usage(c) == USAGE_CONTACT_COUNT);
            Some(Self { preparsed, fingers, has_count, remaining: 0 })
        }
    }

    fn value(&self, hid: &Hid, page: u16, link: u16, usage: u16, array: Option<(u16, u16)>, report: &mut [u8]) -> Option<u32> {
        unsafe {
            let mut v = 0u32;
            let st = (hid.get_usage_value)(HidP_Input, page, link, usage, &mut v, self.pp(), report.as_mut_ptr(), report.len() as u32);
            if st == HIDP_STATUS_SUCCESS {
                return Some(v);
            }
            let (bits, count) = array?;
            let len = (bits as usize * count as usize).div_ceil(8);
            let mut buf = [0u8; 64];
            if len > buf.len() {
                return None;
            }
            let st = (hid.get_usage_value_array)(
                HidP_Input,
                page,
                link,
                usage,
                buf.as_mut_ptr(),
                len as u16,
                self.pp(),
                report.as_mut_ptr(),
                report.len() as u32,
            );
            if st != HIDP_STATUS_SUCCESS {
                return None;
            }
            let first = (bits as usize).div_ceil(8).min(4);
            let mut b = [0u8; 4];
            b[..first].copy_from_slice(&buf[..first]);
            Some(u32::from_le_bytes(b))
        }
    }

    fn tip(&self, hid: &Hid, link: u16, report: &mut [u8]) -> Option<bool> {
        let mut usages = [0u16; 16];
        let mut n = usages.len() as u32;
        let st = unsafe {
            (hid.get_usages)(
                HidP_Input,
                PAGE_DIGITIZER,
                link,
                usages.as_mut_ptr(),
                &mut n,
                self.pp(),
                report.as_mut_ptr(),
                report.len() as u32,
            )
        };
        (st == HIDP_STATUS_SUCCESS).then(|| usages[..(n as usize).min(usages.len())].contains(&USAGE_TIP))
    }
}

// ---------------------------------------------------------------------------------------------
// Contacts and screen mapping

#[derive(Default)]
struct State {
    /// `None`: not a usable touch screen.
    devices: HashMap<isize, Option<Device>>,
    /// (device, contact id) → last normalized position, for contacts that are down.
    down: HashMap<(isize, u32), (f32, f32)>,
    /// Last contact down: normalized position, display rectangle and rotation, time (for
    /// calibration).
    last_down: Option<(f32, f32, RECT, u8, u64)>,
    /// Rotation found by calibration (overrides the display orientation).
    rotation: Option<u8>,
    buf: Vec<u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Maps a normalized panel position to the screen: `rotation` 0–3 = 0°, 90°, 180°, 270°
/// (display rotated clockwise from the panel's native orientation).
fn map(u: f32, v: f32, rotation: u8, r: RECT) -> POINT {
    let (x, y) = match rotation & 3 {
        0 => (u, v),
        1 => (v, 1.0 - u),
        2 => (1.0 - u, 1.0 - v),
        _ => (1.0 - v, u),
    };
    let w = (r.right - r.left - 1).max(0) as f32;
    let h = (r.bottom - r.top - 1).max(0) as f32;
    POINT { x: r.left + (x * w).round() as i32, y: r.top + (y * h).round() as i32 }
}

/// Screen rectangle and rotation of the display the touch device `handle` belongs to: the
/// pointer device with the same handle, else the only touch device, else the primary monitor.
fn display_of(handle: isize) -> (RECT, u8) {
    unsafe {
        let mut n = 0u32;
        let mut infos = Vec::new();
        if GetPointerDevices(&mut n, None).is_ok() && n > 0 && n < 64 {
            infos = vec![POINTER_DEVICE_INFO::default(); n as usize];
            if GetPointerDevices(&mut n, Some(infos.as_mut_ptr())).is_err() {
                infos.clear();
            }
            infos.truncate(n as usize);
        }
        let touch: Vec<&POINTER_DEVICE_INFO> = infos.iter().filter(|i| i.pointerDeviceType == POINTER_DEVICE_TYPE_TOUCH).collect();
        let dev = infos.iter().find(|i| i.device.0 as isize == handle).or(if touch.len() == 1 { Some(touch[0]) } else { None });
        if let Some(d) = dev {
            let mut device_rc = RECT::default();
            let mut display_rc = RECT::default();
            // DISPLAYCONFIG_ROTATION: 1 = identity, 2 = 90°, 3 = 180°, 4 = 270°.
            let rot = d.displayOrientation.saturating_sub(1).min(3) as u8;
            if GetPointerDeviceRects(d.device, &mut device_rc, &mut display_rc).is_ok() && display_rc.right > display_rc.left {
                return (display_rc, rot);
            }
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(d.monitor, &mut mi).as_bool() {
                return (mi.rcMonitor, rot);
            }
        }
        let mon = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let rc = if GetMonitorInfoW(mon, &mut mi).as_bool() { mi.rcMonitor } else { RECT::default() };
        let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        let rot = if EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
            (dm.Anonymous1.Anonymous2.dmDisplayOrientation.0 & 3) as u8
        } else {
            0
        };
        (rc, rot)
    }
}

fn on_input(h: HRAWINPUT) {
    let Some(hid) = hid() else { return };
    STATE.with(|cell| {
        let Ok(mut st) = cell.try_borrow_mut() else { return };
        let st = &mut *st;
        let header = size_of::<RAWINPUTHEADER>() as u32;
        let mut size = 0u32;
        unsafe { GetRawInputData(h, RID_INPUT, None, &mut size, header) };
        if size < header + 8 || size > 1 << 16 {
            return;
        }
        st.buf.resize((size as usize).div_ceil(8), 0);
        let got = unsafe { GetRawInputData(h, RID_INPUT, Some(st.buf.as_mut_ptr() as _), &mut size, header) };
        if got == u32::MAX || got < header + 8 {
            return;
        }
        let bytes = unsafe { std::slice::from_raw_parts_mut(st.buf.as_mut_ptr() as *mut u8, got as usize) };
        let head = unsafe { &*(bytes.as_ptr() as *const RAWINPUTHEADER) };
        if head.dwType != RIM_TYPEHID.0 {
            return;
        }
        let handle = head.hDevice.0 as isize;
        let off = header as usize;
        let size_hid = u32::from_ne_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let count = u32::from_ne_bytes(bytes[off + 4..off + 8].try_into().unwrap()) as usize;
        let data = off + 8;
        if size_hid == 0 || data + size_hid * count > bytes.len() {
            return;
        }
        let dev = st.devices.entry(handle).or_insert_with(|| {
            let d = Device::load(hid, head.hDevice);
            log_line(format_args!(
                "raw touch device {handle:#x}: {}",
                match &d {
                    Some(d) => format!("{} contacts, contact count {}", d.fingers.len(), d.has_count),
                    None => "not usable".to_owned(),
                }
            ));
            d
        });
        let Some(dev) = dev.as_mut() else { return };
        let mut events = Vec::new();
        for i in 0..count {
            let report = &mut bytes[data + i * size_hid..data + (i + 1) * size_hid];
            let valid = if dev.has_count {
                if let Some(c) = dev.value(hid, PAGE_DIGITIZER, 0, USAGE_CONTACT_COUNT, None, report)
                    && c > 0
                {
                    dev.remaining = c as usize;
                }
                let v = dev.remaining.min(dev.fingers.len());
                dev.remaining -= v;
                v
            } else {
                dev.fingers.len()
            };
            for f in &dev.fingers[..valid] {
                let Some(tip) = dev.tip(hid, f.link, report) else { continue };
                let id = if f.has_id {
                    dev.value(hid, PAGE_DIGITIZER, f.link, USAGE_CONTACT_ID, None, report).unwrap_or(f.link as u32)
                } else {
                    f.link as u32
                };
                let x = dev.value(hid, PAGE_GENERIC, f.link, USAGE_X, f.x.array, report);
                let y = dev.value(hid, PAGE_GENERIC, f.link, USAGE_Y, f.y.array, report);
                let pos = x.zip(y).map(|(x, y)| (f.x.normalize(x), f.y.normalize(y)));
                events.push((id, tip, pos));
            }
        }
        for (id, tip, pos) in events {
            let key = (handle, id);
            match (tip, st.down.get(&key).copied()) {
                (true, prev) => {
                    let Some(p) = pos.or(prev) else { continue };
                    st.down.insert(key, p);
                    if prev.is_none() {
                        if st.down.len() > 32 {
                            st.down.clear();
                        }
                        let (rc, rot) = display_of(handle);
                        let pt = map(p.0, p.1, st.rotation.unwrap_or(rot), rc);
                        st.last_down = Some((p.0, p.1, rc, rot, now_ms()));
                        log_line(format_args!("raw down {id} at ({},{})", pt.x, pt.y));
                        touch_down(pt, true);
                    }
                }
                (false, Some(prev)) => {
                    st.down.remove(&key);
                    let p = pos.filter(|&(u, v)| u > 0.0 || v > 0.0).unwrap_or(prev);
                    let (rc, rot) = display_of(handle);
                    let pt = map(p.0, p.1, st.rotation.unwrap_or(rot), rc);
                    log_line(format_args!("raw up {id} at ({},{})", pt.x, pt.y));
                    touch_up(pt, "raw");
                }
                (false, None) => {}
            }
        }
    });
}

/// From the mouse hook: Windows synthesized a mouse press from a touch at `pt`. If raw input saw
/// that contact go down just before, check the rotation mapping against it.
pub(super) fn calibrate(pt: POINT) {
    STATE.with(|cell| {
        let Ok(mut st) = cell.try_borrow_mut() else { return };
        let Some((u, v, rc, rot, t)) = st.last_down else { return };
        if now_ms().saturating_sub(t) > 400 {
            return;
        }
        let dist = |r: u8| {
            let p = map(u, v, r, rc);
            (((p.x - pt.x) as i64).pow(2) + ((p.y - pt.y) as i64).pow(2)) as f64
        };
        let best = (0..4u8).min_by(|&a, &b| dist(a).total_cmp(&dist(b))).unwrap_or(0);
        let diag2 = (((rc.right - rc.left) as i64).pow(2) + ((rc.bottom - rc.top) as i64).pow(2)) as f64;
        // Only a clear winner within 3% of the diagonal.
        if dist(best) < diag2 * 0.0009 && st.rotation != Some(best) {
            let current = st.rotation.unwrap_or(rot);
            if dist(current) > diag2 * 0.0009 {
                log_line(format_args!("raw touch rotation calibrated: {best}"));
                st.rotation = Some(best);
            }
        }
    });
}
