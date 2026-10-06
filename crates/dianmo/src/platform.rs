//! Small Win32 helpers for the main program: paths, single instance, autostart, console.

use std::cell::Cell;
use std::path::PathBuf;

use dianmo_win::HostProxy;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WIN32_ERROR, WPARAM};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterClassW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

/// `%APPDATA%\Dianmo` (settings, log, Rime user data).
pub fn data_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join("Dianmo"),
        None => exe_dir().join("userdata"),
    }
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Into::into)).unwrap_or_else(|| PathBuf::from("."))
}

/// Lets `println!` reach the console of the process that started us (for `--deploy`, `--help`).
pub fn attach_parent_console() -> bool {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS).is_ok() }
}

// ---------------------------------------------------------------------------------------------
// Single instance
// ---------------------------------------------------------------------------------------------

const MUTEX_NAME: PCWSTR = w!("Local\\Dianmo.SingleInstance");
const INSTANCE_CLASS: PCWSTR = w!("DianmoInstance");
/// Sent by a second `dianmo.exe` to the running one: show the keyboard.
const WM_APP_SHOW_KEYBOARD: u32 = WM_APP + 0x51;

/// Holds the named mutex for the life of the process.
pub struct InstanceLock(#[allow(dead_code)] HANDLE);

/// `Some` if we are the first instance.
pub fn acquire_single_instance() -> Option<InstanceLock> {
    unsafe {
        let handle = CreateMutexW(None, false, MUTEX_NAME).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return None;
        }
        Some(InstanceLock(handle))
    }
}

/// Asks the running instance to show its keyboard. False if it couldn't be found (still starting,
/// or exiting).
pub fn signal_running_instance() -> bool {
    for _ in 0..20 {
        unsafe {
            if let Ok(hwnd) = FindWindowExW(Some(HWND_MESSAGE), None, INSTANCE_CLASS, PCWSTR::null()) {
                return PostMessageW(Some(hwnd), WM_APP_SHOW_KEYBOARD, WPARAM(0), LPARAM(0)).is_ok();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

thread_local! {
    static PROXY: Cell<Option<HostProxy>> = const { Cell::new(None) };
}

/// The host proxy, once the app has seen its first callback (see `DianmoApp::proxy_ready`).
pub fn set_proxy(proxy: HostProxy) {
    PROXY.with(|p| p.set(Some(proxy)));
}

/// Creates the message-only window that receives "show" from later instances. Must run on the UI
/// thread (the host's message loop dispatches it).
pub fn create_instance_window() -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW { lpfnWndProc: Some(instance_proc), hInstance: instance, lpszClassName: INSTANCE_CLASS, ..Default::default() };
        RegisterClassW(&class);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            INSTANCE_CLASS,
            w!("点墨"),
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
    }
}

extern "system" fn instance_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_APP_SHOW_KEYBOARD {
        match PROXY.with(|p| p.get()) {
            Some(p) => {
                p.show();
            }
            None => crate::log!("second instance asked to show the keyboard before the host was ready"),
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

// ---------------------------------------------------------------------------------------------
// Autostart (HKCU\Software\Microsoft\Windows\CurrentVersion\Run)
// ---------------------------------------------------------------------------------------------

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Dianmo");

fn run_command() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    format!("\"{}\" --autostart", exe.display())
}

/// The registered autostart command, if any.
pub fn autostart_command() -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if err != WIN32_ERROR(0) {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned())
}

pub fn autostart_enabled() -> bool {
    autostart_command().is_some()
}

/// True if autostart is on but points at another exe (e.g. an old install location).
pub fn autostart_stale() -> bool {
    autostart_command().is_some_and(|c| !c.eq_ignore_ascii_case(&run_command()))
}

pub fn set_autostart(on: bool) -> windows::core::Result<()> {
    let err = unsafe {
        if on {
            let cmd: Vec<u16> = run_command().encode_utf16().chain(Some(0)).collect();
            RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, REG_SZ.0, Some(cmd.as_ptr().cast()), (cmd.len() * 2) as u32)
        } else {
            match RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE) {
                ERROR_FILE_NOT_FOUND => WIN32_ERROR(0),
                e => e,
            }
        }
    };
    err.ok()
}
