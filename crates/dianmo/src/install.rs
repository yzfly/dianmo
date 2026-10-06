//! Installing and uninstalling (docs/PRODUCT.md P1 / P2), and closing a running 点墨 for both.
//!
//!   dianmo.exe --quit                          close the running 点墨 gracefully (exit codes below)
//!   dianmo.exe --install [--quiet] [--no-run]  register the copy this exe is in (run by
//!                                              DianmoSetup.exe after it placed the files)
//!   dianmo.exe --uninstall [--keep-data | --delete-data] [--quiet | /S]
//!
//! `--install` (the files are already in place, this exe's directory is the install root):
//! - shortcuts 「点墨」 on the desktop and in the Start menu, 「点墨设置」 in the Start menu
//!   (`dianmo.exe --settings`);
//! - the 「应用和功能」 entry `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Dianmo`;
//! - the elevated scheduled task (`--register-task`, see elevate.rs) unless it already runs this
//!   exe: run directly when we are elevated, else through a UAC prompt (declined → exit code 2,
//!   点墨 then works without elevation);
//! - starts 点墨 (`--hidden` with `--quiet`, i.e. after a silent update) unless `--no-run`.
//!
//! `--uninstall`: asks (unless quiet) whether to keep the user data (default: keep), closes 点墨,
//! puts the system touch keyboard settings back if a crashed 点墨 left them changed, deletes the
//! task (UAC prompt if needed), the old HKCU Run entry, the shortcuts and the uninstall entry, and
//! finally — from a `cmd` that waits for this process to exit — the program directory and, if
//! asked, `%APPDATA%\Dianmo`. The program directory is only deleted when it is the registered
//! install location (or the default one), never a build directory.
//!
//! `--instance <name>` (tests) suffixes everything: task / uninstall entry `Dianmo<name>`,
//! shortcuts 「点墨 <name>」, data dir `%APPDATA%\Dianmo-<name>`.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use dianmo_win::tabtip::SystemKeyboardSettings;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, HWND, LPARAM,
    WAIT_OBJECT_0, WIN32_ERROR, WPARAM,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC,
    SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, IDNO, IDOK, IDYES, IsWindow, MB_ICONERROR, MB_ICONINFORMATION,
    MB_ICONQUESTION, MB_OK, MB_OKCANCEL, MB_SETFOREGROUND, MB_YESNOCANCEL, MESSAGEBOX_STYLE, MessageBoxW, PostMessageW,
    SW_HIDE, WM_CLOSE,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, w};

use crate::settings::Settings;
use crate::{elevate, log, platform};

pub const PUBLISHER: &str = "云中江树";
pub const HOMEPAGE: &str = "https://github.com/yzfly/dianmo";
/// Opens the settings window (the 「点墨设置」 Start menu shortcut).
pub const SETTINGS_ARG: &str = "--settings";

/// `--quit` exit codes (DianmoSetup relies on them).
pub const QUIT_OK: i32 = 0;
pub const QUIT_STILL_RUNNING: i32 = 1;
/// The running 点墨 is elevated and refused our messages: run `--quit` elevated.
pub const QUIT_ACCESS_DENIED: i32 = 3;
/// `--install`: everything done except the scheduled task (UAC declined or failed).
pub const INSTALL_NO_TASK: i32 = 2;
/// `--uninstall` cancelled by the user (= ERROR_INSTALL_USEREXIT, what uninstallers return).
const UNINSTALL_CANCELLED: i32 = 1602;

const UNINSTALL_ROOT: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn has(args: &[String], names: &[&str]) -> bool {
    args.iter().any(|a| names.iter().any(|n| a.eq_ignore_ascii_case(n)))
}

/// 「点墨」, 「点墨 Test」 for `--instance Test`.
fn suffixed_name(base: &str) -> String {
    match platform::instance_name() {
        "" => base.to_owned(),
        i => format!("{base} {i}"),
    }
}

/// `--instance <name>` to pass on to child processes.
fn instance_args() -> Vec<String> {
    match platform::instance_name() {
        "" => vec![],
        i => vec!["--instance".into(), i.into()],
    }
}

fn uninstall_key() -> String {
    format!("{UNINSTALL_ROOT}\\{}", platform::task_name())
}

pub fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        (!s.is_empty()).then(|| PathBuf::from(s))
    }
}

/// Where DianmoSetup installs by default: `%LOCALAPPDATA%\Dianmo` (`Dianmo-<instance>`).
pub fn default_root() -> Option<PathBuf> {
    let name = match platform::instance_name() {
        "" => "Dianmo".to_owned(),
        i => format!("Dianmo-{i}"),
    };
    known_folder(&FOLDERID_LocalAppData)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .map(|d| d.join(name))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_owned())
            .to_string_lossy()
            .trim_end_matches('\\')
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn message(text: &str, style: MESSAGEBOX_STYLE) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("点墨"), style | MB_SETFOREGROUND) }
}

// ---------------------------------------------------------------------------------------------
// Closing the running instance
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    NotRunning,
    Closed,
    /// It is elevated and UIPI dropped our messages (an older version that doesn't let the quit
    /// message through). Retry elevated.
    AccessDenied,
    StillRunning,
}

/// Asks the running 点墨 (this `--instance`) to exit and waits up to `timeout`. Never kills it: a
/// killed 点墨 leaves its AppBar screen space reserved and the system keyboard settings changed.
pub fn quit_running(timeout: Duration) -> Quit {
    let Some(hwnd) = platform::find_instance_window() else { return Quit::NotRunning };
    let start = Instant::now();
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return Quit::NotRunning;
    }
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok();
    let wait = |limit: Duration| wait_exit(process, hwnd, limit.saturating_sub(start.elapsed()));

    let post = |w: HWND, msg: u32| unsafe {
        match PostMessageW(Some(w), msg, WPARAM(0), LPARAM(0)) {
            Ok(()) => Ok(()),
            Err(_) => Err(GetLastError()),
        }
    };
    let mut denied = false;
    match post(hwnd, platform::WM_APP_QUIT) {
        Ok(()) => {
            if wait(timeout.min(Duration::from_secs(4))) {
                close_handle(process);
                return Quit::Closed;
            }
        }
        Err(e) => denied = e == ERROR_ACCESS_DENIED,
    }
    // Versions before the installer don't know the quit message: close their keyboard window.
    let mut sent = false;
    for w in keyboard_windows(pid) {
        match post(w, WM_CLOSE) {
            Ok(()) => sent = true,
            Err(e) => denied |= e == ERROR_ACCESS_DENIED,
        }
    }
    let result = if sent && wait(timeout) {
        Quit::Closed
    } else if denied && !sent {
        Quit::AccessDenied
    } else {
        Quit::StillRunning
    };
    close_handle(process);
    result
}

fn close_handle(h: Option<HANDLE>) {
    if let Some(h) = h {
        unsafe {
            let _ = CloseHandle(h);
        }
    }
}

/// Top-level `DianmoKeyboard` windows (dianmo-win's keyboard) of process `pid`.
fn keyboard_windows(pid: u32) -> Vec<HWND> {
    let mut out = Vec::new();
    let mut after: Option<HWND> = None;
    unsafe {
        while let Ok(w) = FindWindowExW(None, after, w!("DianmoKeyboard"), PCWSTR::null()) {
            let mut owner = 0u32;
            GetWindowThreadProcessId(w, Some(&mut owner));
            if owner == pid {
                out.push(w);
            }
            after = Some(w);
        }
    }
    out
}

/// Waits for the process to end (handle), or — if it couldn't be opened — for its window to go.
fn wait_exit(process: Option<HANDLE>, hwnd: HWND, timeout: Duration) -> bool {
    if let Some(h) = process {
        return unsafe { WaitForSingleObject(h, timeout.as_millis() as u32) } == WAIT_OBJECT_0;
    }
    let start = Instant::now();
    while unsafe { IsWindow(Some(hwnd)).as_bool() } {
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // The window goes when the process ends; give the system a moment to release its files.
    std::thread::sleep(Duration::from_millis(500));
    true
}

/// `--quit`.
pub fn cmd_quit() -> i32 {
    platform::attach_parent_console();
    let r = quit_running(Duration::from_secs(10));
    log!("--quit: {r:?}");
    println!("{r:?}");
    match r {
        Quit::NotRunning | Quit::Closed => QUIT_OK,
        Quit::AccessDenied => QUIT_ACCESS_DENIED,
        Quit::StillRunning => QUIT_STILL_RUNNING,
    }
}

// ---------------------------------------------------------------------------------------------
// Child processes
// ---------------------------------------------------------------------------------------------

fn quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        a.to_owned()
    } else {
        format!("\"{}\"", a.replace('"', "\\\""))
    }
}

/// Runs this exe with `args` and waits: directly when we are elevated, else through UAC.
/// `Err(true)` = the user declined the prompt.
fn run_self_elevated(args: &[String]) -> Result<i32, bool> {
    let exe = std::env::current_exe().map_err(|_| false)?;
    if elevate::is_elevated() {
        return Command::new(&exe).args(args).status().map(|s| s.code().unwrap_or(-1)).map_err(|e| {
            log!("running {} failed: {e}", exe.display());
            false
        });
    }
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            let declined = e.code() == ERROR_CANCELLED.to_hresult();
            log!("runas {args:?}: {}", if declined { "declined".to_owned() } else { e.to_string() });
            return Err(declined);
        }
        if info.hProcess.is_invalid() {
            return Err(false);
        }
        let mut code = 1u32;
        if WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
        Ok(code as i32)
    }
}

// ---------------------------------------------------------------------------------------------
// Shortcuts
// ---------------------------------------------------------------------------------------------

/// (folder, name) of every shortcut we create or remove.
fn shortcut_paths() -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    for folder in [&FOLDERID_Desktop, &FOLDERID_Programs] {
        if let Some(dir) = known_folder(folder) {
            out.push((dir.clone(), "点墨"));
            out.push((dir, "点墨设置"));
        }
    }
    out
}

fn create_shortcut(lnk: &Path, exe: &Path, args: &str, description: &str) -> windows::core::Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetArguments(&HSTRING::from(args))?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from(description))?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(lnk.as_os_str()), true)
    }
}

fn create_shortcuts(exe: &Path) -> Vec<String> {
    let inst = instance_args().iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let with =
        |extra: &str| [extra, inst.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    let programs = known_folder(&FOLDERID_Programs);
    let mut failed = Vec::new();
    for (dir, base) in shortcut_paths() {
        let is_settings = base == "点墨设置";
        // 「点墨设置」 only in the Start menu.
        if is_settings && programs.as_deref() != Some(dir.as_path()) {
            continue;
        }
        let lnk = dir.join(format!("{}.lnk", suffixed_name(base)));
        let (args, desc) =
            if is_settings { (with(SETTINGS_ARG), "点墨 设置") } else { (with(""), "点墨 · 触屏输入法") };
        match create_shortcut(&lnk, exe, &args, desc) {
            Ok(()) => log!("shortcut {}", lnk.display()),
            Err(e) => {
                log!("creating {} failed: {e}", lnk.display());
                failed.push(format!("快捷方式 {}", lnk.display()));
            }
        }
    }
    failed
}

fn remove_shortcuts() {
    for (dir, base) in shortcut_paths() {
        let lnk = dir.join(format!("{}.lnk", suffixed_name(base)));
        match std::fs::remove_file(&lnk) {
            Ok(()) => log!("removed {}", lnk.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log!("removing {} failed: {e}", lnk.display()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Uninstall entry (「应用和功能」)
// ---------------------------------------------------------------------------------------------

enum RegValue {
    Sz(String),
    Dword(u32),
}

fn write_uninstall_entry(dir: &Path, exe: &Path) -> windows::core::Result<()> {
    let exe_q = format!("\"{}\"", exe.display());
    let inst = instance_args().iter().map(|a| format!(" {}", quote(a))).collect::<String>();
    let size_kb = (dir_size(dir) / 1024).min(u32::MAX as u64) as u32;
    let display = match platform::instance_name() {
        "" => "点墨".to_owned(),
        i => format!("点墨 ({i})"),
    };
    let values = [
        ("DisplayName", RegValue::Sz(display)),
        ("DisplayIcon", RegValue::Sz(format!("{},0", exe.display()))),
        ("DisplayVersion", RegValue::Sz(env!("CARGO_PKG_VERSION").into())),
        ("Publisher", RegValue::Sz(PUBLISHER.into())),
        ("URLInfoAbout", RegValue::Sz(HOMEPAGE.into())),
        ("HelpLink", RegValue::Sz(format!("{HOMEPAGE}/issues"))),
        ("InstallLocation", RegValue::Sz(dir.display().to_string())),
        ("InstallDate", RegValue::Sz(today_yyyymmdd())),
        ("UninstallString", RegValue::Sz(format!("{exe_q} --uninstall{inst}"))),
        ("QuietUninstallString", RegValue::Sz(format!("{exe_q} --uninstall --quiet{inst}"))),
        ("EstimatedSize", RegValue::Dword(size_kb)),
        ("NoModify", RegValue::Dword(1)),
        ("NoRepair", RegValue::Dword(1)),
    ];
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(uninstall_key()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let mut result = Ok(());
        for (name, value) in values {
            let err = match value {
                RegValue::Sz(s) => {
                    let wide: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
                    let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                    RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes))
                }
                RegValue::Dword(d) => {
                    RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&d.to_le_bytes()))
                }
            };
            if let Err(e) = err.ok() {
                result = Err(e);
            }
        }
        let _ = RegCloseKey(key);
        result
    }
}

/// `InstallLocation` of our uninstall entry, if registered.
fn registered_location() -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(uninstall_key()),
            w!("InstallLocation"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if err != WIN32_ERROR(0) {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

fn remove_uninstall_entry() {
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(uninstall_key())) } {
        WIN32_ERROR(0) => log!("removed uninstall entry {}", uninstall_key()),
        ERROR_FILE_NOT_FOUND => {}
        e => log!("removing uninstall entry failed: {e:?}"),
    }
}

fn remove_run_entry() {
    let err = unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from(platform::task_name()))
    };
    if err != WIN32_ERROR(0) && err != ERROR_FILE_NOT_FOUND {
        log!("removing the Run entry failed: {err:?}");
    }
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_size(&e.path()),
                    _ => e.metadata().map(|m| m.len()).unwrap_or(0),
                })
                .sum()
        })
        .unwrap_or(0)
}

fn today_yyyymmdd() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}{:02}{:02}", t.wYear, t.wMonth, t.wDay)
}

// ---------------------------------------------------------------------------------------------
// --install
// ---------------------------------------------------------------------------------------------

/// `--install [--quiet] [--no-run]`: registers the copy this exe is in. Exit 0, `INSTALL_NO_TASK`,
/// or 1 if shortcuts / the uninstall entry failed.
pub fn cmd_install(args: &[String]) -> i32 {
    platform::attach_parent_console();
    let quiet = has(args, &["--quiet", "/s"]);
    let no_run = has(args, &["--no-run"]);
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(Path::to_owned).unwrap_or_default();
    log!("install {} in {} (quiet {quiet}, run {})", env!("CARGO_PKG_VERSION"), dir.display(), !no_run);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let mut failed = create_shortcuts(&exe);
    if let Err(e) = write_uninstall_entry(&dir, &exe) {
        log!("writing the uninstall entry failed: {e}");
        failed.push("「应用和功能」登记".into());
    }

    let task = platform::task_name();
    let task_ok = if platform::own_task().is_some() {
        log!("task {task} already runs this exe");
        true
    } else {
        let mut a = vec!["--register-task".to_owned()];
        a.extend(instance_args());
        match run_self_elevated(&a) {
            Ok(0) => true,
            Ok(c) => {
                log!("--register-task exit {c}");
                false
            }
            Err(declined) => {
                log!("task {task} not registered ({})", if declined { "UAC declined" } else { "failed" });
                false
            }
        }
    };

    if !no_run {
        let mut a = instance_args();
        if quiet {
            a.push("--hidden".into());
        }
        match Command::new(&exe).args(&a).current_dir(&dir).spawn() {
            Ok(_) => log!("started {} {a:?}", exe.display()),
            Err(e) => log!("starting 点墨 failed: {e}"),
        }
    }

    let code = if !failed.is_empty() {
        log!("install: failed: {}", failed.join(", "));
        1
    } else if !task_ok {
        INSTALL_NO_TASK
    } else {
        0
    };
    println!("install: exit {code}");
    code
}

// ---------------------------------------------------------------------------------------------
// --uninstall
// ---------------------------------------------------------------------------------------------

/// `--uninstall [--keep-data | --delete-data] [--quiet | /S]`.
pub fn cmd_uninstall(args: &[String]) -> i32 {
    platform::attach_parent_console();
    let quiet = has(args, &["--quiet", "/s"]);
    let mut keep = if has(args, &["--delete-data"]) {
        Some(false)
    } else if has(args, &["--keep-data"]) {
        Some(true)
    } else {
        None
    };
    if !quiet {
        match keep {
            None => {
                let text = "要卸载点墨吗？\n\n\
                    你的个人数据（设置、学到的词、固定的剪贴板条目）要保留吗？保留的话，以后重新安装可以接着用。\n\n\
                    「是」：卸载，保留个人数据（推荐）\n「否」：卸载，同时删除个人数据\n「取消」：不卸载";
                match message(text, MB_YESNOCANCEL | MB_ICONQUESTION) {
                    IDYES => keep = Some(true),
                    IDNO => keep = Some(false),
                    _ => return UNINSTALL_CANCELLED,
                }
            }
            Some(_) => {
                if message("要卸载点墨吗？", MB_OKCANCEL | MB_ICONQUESTION) != IDOK {
                    return UNINSTALL_CANCELLED;
                }
            }
        }
    }
    let keep = keep.unwrap_or(true);
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(Path::to_owned).unwrap_or_default();
    log!("uninstall {} (keep data {keep}, quiet {quiet})", dir.display());
    let mut problems: Vec<String> = Vec::new();

    // 1. Close 点墨.
    let mut q = quit_running(Duration::from_secs(10));
    if q == Quit::AccessDenied {
        let mut a = vec!["--quit".to_owned()];
        a.extend(instance_args());
        q = match run_self_elevated(&a) {
            Ok(QUIT_OK) => Quit::Closed,
            _ => Quit::StillRunning,
        };
    }
    log!("uninstall: quit → {q:?}");
    if matches!(q, Quit::StillRunning | Quit::AccessDenied) {
        if !quiet {
            message("点墨还在运行，没能关闭它。请在托盘菜单里选「退出」后再卸载。", MB_OK | MB_ICONERROR);
        }
        return 1;
    }

    // 2. System touch keyboard settings a crashed 点墨 may have left changed.
    let data = platform::data_dir();
    let settings_path = data.join("settings.ini");
    let mut settings = Settings::load(&settings_path);
    if let Some((a, b)) = settings.saved_tabtip.take() {
        match (SystemKeyboardSettings { desktop_mode_auto_invoke: a, tap_invoke: b }).apply() {
            Ok(()) => {
                log!("restored system keyboard settings {a:?}/{b:?}");
                if keep {
                    let _ = settings.save(&settings_path);
                }
            }
            Err(e) => {
                log!("restoring system keyboard settings failed: {e}");
                problems.push("恢复系统触摸键盘设置失败".into());
            }
        }
    }

    // 3. Scheduled task (deleting it needs administrator rights).
    let task = platform::task_name();
    match elevate::query(&task) {
        Ok(Some(_)) => match elevate::unregister(&task) {
            Ok(()) => log!("removed task {task}"),
            Err(e) => {
                log!("removing task {task} here failed ({e}); asking for administrator rights");
                let mut a = vec!["--unregister-task".to_owned()];
                a.extend(instance_args());
                if run_self_elevated(&a) != Ok(0) {
                    problems.push(format!("计划任务「{task}」没有删除（需要管理员权限）"));
                }
            }
        },
        Ok(None) => {}
        Err(e) => log!("reading task {task}: {e}"),
    }

    // 4. Autostart entry, shortcuts, uninstall entry.
    remove_run_entry();
    remove_shortcuts();
    let location = registered_location();
    remove_uninstall_entry();

    // 5. Files: after this process exits.
    let ours = dir.join("dianmo.exe").is_file()
        && (location.as_deref().is_some_and(|l| same_path(l, &dir))
            || default_root().is_some_and(|r| same_path(&r, &dir))
            || std::env::var_os("DIANMO_INSTALL_ROOT").is_some_and(|r| same_path(Path::new(&r), &dir)));
    let mut remove: Vec<PathBuf> = Vec::new();
    if ours {
        remove.push(dir.clone());
    } else {
        log!("not deleting {}: not the install location ({location:?})", dir.display());
    }
    if !keep && data.is_dir() {
        remove.push(data.clone());
    }

    if !quiet {
        let mut text = String::from("点墨已卸载。");
        if keep {
            text.push_str(&format!("\n\n个人数据保留在 {}。", data.display()));
        }
        if !problems.is_empty() {
            text.push_str(&format!("\n\n有几项没有完成：\n· {}", problems.join("\n· ")));
        }
        message(&text, MB_OK | if problems.is_empty() { MB_ICONINFORMATION } else { MB_ICONERROR });
    }
    if !remove.is_empty() {
        delete_after_exit(&remove);
    }
    log!("uninstall done{}", if problems.is_empty() { String::new() } else { format!(": {}", problems.join("; ")) });
    if problems.is_empty() { 0 } else { 1 }
}

/// `cmd /c` that retries (about 1 s apart, 30 s at most) until `dirs` are gone: our exe (and the
/// log we hold open) can only go once this process has exited.
fn delete_after_exit(dirs: &[PathBuf]) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let rm: String = dirs.iter().map(|d| format!("(if exist \"{0}\" rmdir /s /q \"{0}\") & ", d.display())).collect();
    let done: String = dirs.iter().map(|d| format!("if not exist \"{}\" ", d.display())).collect();
    let script = format!("for /l %i in (1,1,30) do @(ping -n 2 127.0.0.1 >nul & {rm}{done}exit 0)");
    log!("cleanup: {script}");
    let r = Command::new("cmd.exe")
        .raw_arg(format!("/d /q /s /c \"{script}\""))
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if let Err(e) = r {
        log!("starting the cleanup failed: {e}");
    }
}
