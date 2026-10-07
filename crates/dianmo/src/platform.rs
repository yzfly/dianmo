//! Small Win32 helpers for the main program: paths, single instance, autostart, console.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use dianmo_win::HostProxy;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, HWND, LPARAM, LRESULT,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilter, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, FindWindowExW,
    HWND_MESSAGE, MSGFLT_ADD, MSGFLT_ALLOW, PostMessageW, RegisterClassW, RegisterWindowMessageW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::elevate::{self, TaskInfo};

// ---------------------------------------------------------------------------------------------
// Instance name (tests)
// ---------------------------------------------------------------------------------------------

static INSTANCE: OnceLock<String> = OnceLock::new();

/// `--instance <name>` (tests): a separate 点墨 next to the installed one, with its own mutex,
/// instance window, data dir (`%APPDATA%\Dianmo-<name>`) and scheduled task (`Dianmo<name>`).
/// Call before anything else. Only ASCII letters and digits are kept.
pub fn set_instance(name: &str) {
    let name: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let _ = INSTANCE.set(name);
}

fn instance() -> &'static str {
    INSTANCE.get().map(String::as_str).unwrap_or("")
}

/// The `--instance` name ("" for the normal one).
pub fn instance_name() -> &'static str {
    instance()
}

fn suffixed(base: &str, sep: &str) -> String {
    match instance() {
        "" => base.to_owned(),
        i => format!("{base}{sep}{i}"),
    }
}

/// The scheduled task that runs 点墨 elevated: `Dianmo` (or `Dianmo<instance>`).
pub fn task_name() -> String {
    suffixed("Dianmo", "")
}

/// `%APPDATA%\Dianmo` (settings, log, Rime user data).
pub fn data_dir() -> PathBuf {
    let name = suffixed("Dianmo", "-");
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join(name),
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

/// Sent by a second `dianmo.exe` to the running one: show the keyboard.
const WM_APP_SHOW_KEYBOARD: u32 = WM_APP + 0x51;
/// Sent by `dianmo.exe --quit` (the installer runs the new exe's `--quit`; `--uninstall` sends it
/// too): exit gracefully. Let through UIPI like the show message.
pub const WM_APP_QUIT: u32 = WM_APP + 0x52;
/// Sent by `dianmo.exe --settings` (Start menu 「点墨设置」) to the running one: open the settings
/// window. Let through UIPI like the show message.
const WM_APP_OPEN_SETTINGS: u32 = WM_APP + 0x53;

/// Posted to the app (`HostProxy::post`) when another `dianmo.exe --settings` asked for the
/// settings window.
pub struct OpenSettings;

fn mutex_name() -> HSTRING {
    HSTRING::from(suffixed("Local\\Dianmo.SingleInstance", "."))
}

fn instance_class() -> HSTRING {
    HSTRING::from(suffixed("DianmoInstance", "."))
}

/// The running instance's message-only window, if any.
pub fn find_instance_window() -> Option<HWND> {
    unsafe { FindWindowExW(Some(HWND_MESSAGE), None, &instance_class(), PCWSTR::null()).ok() }
}

/// Holds the named mutex for the life of the process.
pub struct InstanceLock(#[allow(dead_code)] HANDLE);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// `Some` if we are the first instance.
pub fn acquire_single_instance() -> Option<InstanceLock> {
    unsafe {
        // An elevated instance's mutex can't be opened from a normal process: ACCESS_DENIED also
        // means "already running".
        let handle = CreateMutexW(None, false, &mutex_name()).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            return None;
        }
        Some(InstanceLock(handle))
    }
}

/// Asks the running instance to show its keyboard (`settings`: open its settings window
/// instead). False if it couldn't be found (still starting, or exiting).
pub fn signal_running_instance(settings: bool) -> bool {
    wait_for_instance(Duration::from_secs(2), true, settings)
}

/// Waits up to `timeout` for the running instance's message window; with `show`, asks it to show
/// the keyboard, or with `settings` to open the settings window. False if it didn't appear (or
/// the message was refused).
pub fn wait_for_instance(timeout: Duration, show: bool, settings: bool) -> bool {
    let class = instance_class();
    let start = Instant::now();
    let msg = if settings { Some(WM_APP_OPEN_SETTINGS) } else { show.then_some(WM_APP_SHOW_KEYBOARD) };
    loop {
        unsafe {
            if let Ok(hwnd) = FindWindowExW(Some(HWND_MESSAGE), None, &class, PCWSTR::null()) {
                // We were just started by the user (Start menu, shortcut): hand the right to come
                // to the front over to the running instance, or Windows' foreground lock leaves
                // the keyboard / settings window behind the active app.
                let mut pid = 0u32;
                windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 {
                    let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
                }
                return msg.is_none_or(|m| PostMessageW(Some(hwnd), m, WPARAM(0), LPARAM(0)).is_ok());
            }
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

thread_local! {
    static PROXY: Cell<Option<HostProxy>> = const { Cell::new(None) };
    /// "Show" arrived before the host was ready (an instance started hidden by the task, then
    /// asked to show by the `dianmo.exe` that started the task).
    static PENDING_SHOW: Cell<bool> = const { Cell::new(false) };
    /// "Quit" arrived before the host was ready.
    static PENDING_QUIT: Cell<bool> = const { Cell::new(false) };
    /// "Open the settings" arrived before the host was ready (or `--settings` on the first start).
    static PENDING_SETTINGS: Cell<bool> = const { Cell::new(false) };
}

/// Opens the settings window once the host is ready (`dianmo.exe --settings` starting 点墨).
pub fn request_settings() {
    match PROXY.with(|p| p.get()) {
        Some(p) => {
            p.post(OpenSettings);
        }
        None => PENDING_SETTINGS.with(|p| p.set(true)),
    }
}

/// The host proxy, once the app has seen its first callback (see `DianmoApp::proxy_ready`).
pub fn set_proxy(proxy: HostProxy) {
    PROXY.with(|p| p.set(Some(proxy)));
    if PENDING_QUIT.with(|p| p.replace(false)) {
        proxy.quit();
        return;
    }
    if PENDING_SHOW.with(|p| p.replace(false)) {
        proxy.show();
    }
    if PENDING_SETTINGS.with(|p| p.replace(false)) {
        proxy.post(OpenSettings);
    }
}

/// Creates the message-only window that receives "show" from later instances. Must run on the UI
/// thread (the host's message loop dispatches it). When elevated, lets normal processes (a later
/// `dianmo.exe`, Explorer's tray and AppBar notifications) through UIPI.
pub fn create_instance_window() -> windows::core::Result<HWND> {
    let class_name = instance_class();
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(instance_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &class_name,
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
        )?
    };
    if elevate::is_elevated() {
        unsafe {
            for msg in [WM_APP_SHOW_KEYBOARD, WM_APP_QUIT, WM_APP_OPEN_SETTINGS] {
                if let Err(e) = ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, None) {
                    crate::log!("allowing message {msg:#x} through UIPI failed: {e}");
                }
            }
        }
        allow_shell_messages();
    }
    Ok(hwnd)
}

/// An elevated process doesn't get window messages from normal-integrity Explorer unless they are
/// let through: `TaskbarCreated` and the tray / AppBar callback messages of `dianmo_win`'s host
/// (`WM_APP + 3`, `WM_APP + 4`, see dianmo-win/src/host.rs).
fn allow_shell_messages() {
    unsafe {
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        for msg in [taskbar_created, WM_APP + 3, WM_APP + 4] {
            if msg != 0 && let Err(e) = ChangeWindowMessageFilter(msg, MSGFLT_ADD) {
                crate::log!("ChangeWindowMessageFilter({msg:#x}): {e}");
            }
        }
    }
}

extern "system" fn instance_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_APP_SHOW_KEYBOARD {
        match PROXY.with(|p| p.get()) {
            Some(p) => {
                p.show();
            }
            None => {
                crate::log!("asked to show the keyboard before the host was ready; showing once it is");
                PENDING_SHOW.with(|p| p.set(true));
            }
        }
        return LRESULT(0);
    }
    if msg == WM_APP_OPEN_SETTINGS {
        crate::log!("asked to open the settings");
        request_settings();
        return LRESULT(0);
    }
    if msg == WM_APP_QUIT {
        crate::log!("asked to quit (installer / uninstaller)");
        match PROXY.with(|p| p.get()) {
            Some(p) => {
                p.quit();
            }
            None => PENDING_QUIT.with(|p| p.set(true)),
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

// ---------------------------------------------------------------------------------------------
// Autostart: the scheduled task's logon trigger; HKCU\...\Run when there is no task for this exe
// ---------------------------------------------------------------------------------------------

/// The scheduled task, if it exists and starts this exe. Errors are logged.
pub fn own_task() -> Option<TaskInfo> {
    let name = task_name();
    match elevate::query(&name) {
        Ok(Some(t)) => {
            let exe = std::env::current_exe().unwrap_or_default();
            if t.runs(&exe) {
                Some(t)
            } else {
                crate::log!("task {name} starts {}, not this exe", t.exe.display());
                None
            }
        }
        Ok(None) => None,
        Err(e) => {
            crate::log!("reading task {name}: {e}");
            None
        }
    }
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

fn run_value() -> HSTRING {
    HSTRING::from(suffixed("Dianmo", ""))
}

fn run_command() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    format!("\"{}\" --autostart", exe.display())
}

/// The registered HKCU Run command, if any.
pub fn autostart_command() -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, &run_value(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if err != WIN32_ERROR(0) {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned())
}

fn set_run_entry(on: bool) -> windows::core::Result<()> {
    let err = unsafe {
        if on {
            let cmd: Vec<u16> = run_command().encode_utf16().chain(Some(0)).collect();
            RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &run_value(), REG_SZ.0, Some(cmd.as_ptr().cast()), (cmd.len() * 2) as u32)
        } else {
            match RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &run_value()) {
                ERROR_FILE_NOT_FOUND => WIN32_ERROR(0),
                e => e,
            }
        }
    };
    err.ok()
}

#[allow(dead_code)]
pub fn autostart_enabled() -> bool {
    match own_task() {
        Some(t) => t.logon_trigger || autostart_command().is_some(),
        None => autostart_command().is_some(),
    }
}

/// True if autostart needs rewriting: an old HKCU Run entry while the task exists (migrate to the
/// task's logon trigger), or a Run entry that points at another exe (old install location).
#[allow(dead_code)]
pub fn autostart_stale() -> bool {
    match own_task() {
        Some(_) => autostart_command().is_some(),
        None => autostart_command().is_some_and(|c| !c.eq_ignore_ascii_case(&run_command())),
    }
}

/// Turns autostart on/off: the task's logon trigger when the task starts this exe (needs to be
/// elevated), else the HKCU Run entry.
pub fn set_autostart(on: bool) -> windows::core::Result<()> {
    if own_task().is_some() {
        elevate::set_logon_trigger(&task_name(), on)?;
        set_run_entry(false)
    } else {
        set_run_entry(on)
    }
}

/// At startup: fixes up autostart (migrates an HKCU Run entry to the task's logon trigger, or
/// points a stale Run entry at this exe) and returns whether it is on. One task query.
pub fn sync_autostart() -> bool {
    let run = autostart_command();
    match own_task() {
        Some(task) => {
            if run.is_none() {
                return task.logon_trigger;
            }
            if !task.logon_trigger
                && let Err(e) = elevate::set_logon_trigger(&task_name(), true)
            {
                // Not elevated: the Run entry keeps working (it starts dianmo.exe, which hands
                // over to the task).
                crate::log!("moving autostart to the task failed: {e}");
                return true;
            }
            match set_run_entry(false) {
                Ok(()) => crate::log!("autostart moved from HKCU Run to the task's logon trigger"),
                Err(e) => crate::log!("removing the HKCU Run entry failed: {e}"),
            }
            true
        }
        None => {
            if run.as_deref().is_some_and(|c| !c.eq_ignore_ascii_case(&run_command()))
                && let Err(e) = set_run_entry(true)
            {
                crate::log!("updating autostart failed: {e}");
            }
            run.is_some()
        }
    }
}
