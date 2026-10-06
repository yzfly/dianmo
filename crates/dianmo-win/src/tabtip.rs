//! The Windows touch keyboard's auto-invoke settings (DESIGN.md §3 自动弹出): turn them off while
//! Dianmo runs so two keyboards don't pop up together, and restore them exactly afterwards.
//!
//! Values live under `HKCU\Software\Microsoft\TabletTip\1.7`:
//! - `EnableDesktopModeAutoInvoke` (Windows 10): 1 = show the touch keyboard when tapping a text
//!   field in desktop mode with no keyboard attached, 0 = don't.
//! - `TouchKeyboardTapInvoke` (Windows 11): 0 = never, 1 = when no keyboard attached, 2 = always.
//!
//! Don't disable the TabletInputService: voice typing (Win+H) depends on it.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR, w};

const KEY: PCWSTR = w!("Software\\Microsoft\\TabletTip\\1.7");

pub const DESKTOP_MODE_AUTO_INVOKE: &str = "EnableDesktopModeAutoInvoke";
pub const TOUCH_KEYBOARD_TAP_INVOKE: &str = "TouchKeyboardTapInvoke";

/// Reads a DWORD under the TabletTip key. `Ok(None)` if the value (or key) doesn't exist.
pub fn read_dword(name: &str) -> windows::core::Result<Option<u32>> {
    let name = HSTRING::from(name);
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            &name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    match err {
        WIN32_ERROR(0) => Ok(Some(data)),
        ERROR_FILE_NOT_FOUND => Ok(None),
        e => Err(e.to_hresult().into()),
    }
}

/// Writes (`Some`) or deletes (`None`) a DWORD under the TabletTip key (creating the key).
pub fn write_dword(name: &str, value: Option<u32>) -> windows::core::Result<()> {
    let name = HSTRING::from(name);
    let err = unsafe {
        match value {
            Some(v) => RegSetKeyValueW(
                HKEY_CURRENT_USER,
                KEY,
                &name,
                REG_DWORD.0,
                Some(&v as *const u32 as *const _),
                size_of::<u32>() as u32,
            ),
            None => match RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, &name) {
                ERROR_FILE_NOT_FOUND => WIN32_ERROR(0),
                e => e,
            },
        }
    };
    err.ok()
}

/// A snapshot of both settings (`None` = value absent), so they can be restored exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemKeyboardSettings {
    pub desktop_mode_auto_invoke: Option<u32>,
    pub tap_invoke: Option<u32>,
}

impl SystemKeyboardSettings {
    pub fn read() -> windows::core::Result<Self> {
        Ok(Self {
            desktop_mode_auto_invoke: read_dword(DESKTOP_MODE_AUTO_INVOKE)?,
            tap_invoke: read_dword(TOUCH_KEYBOARD_TAP_INVOKE)?,
        })
    }

    /// Writes this snapshot back, deleting values that were absent.
    pub fn apply(&self) -> windows::core::Result<()> {
        write_dword(DESKTOP_MODE_AUTO_INVOKE, self.desktop_mode_auto_invoke)?;
        write_dword(TOUCH_KEYBOARD_TAP_INVOKE, self.tap_invoke)
    }

    /// True if the system touch keyboard may pop up on its own when a text field is tapped.
    /// Absent values mean the Windows default, which is "auto-invoke on" for touch-only devices.
    pub fn auto_invoke_enabled(&self) -> bool {
        self.desktop_mode_auto_invoke != Some(0) || self.tap_invoke.is_some_and(|v| v != 0)
    }
}

/// Stops the system touch keyboard from auto-popping up (sets both values to 0) and returns the
/// previous settings so the caller can [`SystemKeyboardSettings::apply`] them on exit.
pub fn disable_system_keyboard_auto_invoke() -> windows::core::Result<SystemKeyboardSettings> {
    let prev = SystemKeyboardSettings::read()?;
    SystemKeyboardSettings { desktop_mode_auto_invoke: Some(0), tap_invoke: Some(0) }.apply()?;
    Ok(prev)
}
