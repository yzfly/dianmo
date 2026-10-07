//! Shell helpers for the settings window: running `dianmo.exe` elevated through UAC and the
//! system's open / save dialogs (豆包语音's program, user dictionary import / export). (Links and folders open through `diag::open_url`, never elevated.)
//!
//! Both block: call them on short-lived background threads.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FileOpenDialog, FileSaveDialog, IFileDialog,
    IFileOpenDialog, IFileSaveDialog, IShellItem, SEE_MASK_NOCLOSEPROCESS, SHCreateItemFromParsingName, SHELLEXECUTEINFOW,
    SIGDN_FILESYSPATH, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
use windows::core::{HSTRING, PCWSTR, w};

/// Runs `exe args` elevated (UAC prompt), waits for it and returns its exit code.
/// `Err` when the user declined or it couldn't start.
pub fn run_elevated(exe: &Path, args: &str) -> Result<u32, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(args);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: 0, // SW_HIDE: dianmo.exe --register-task has no window anyway
            ..Default::default()
        };
        let r = ShellExecuteExW(&mut info);
        CoUninitialize();
        r.map_err(|e| e.message())?;
        if info.hProcess.is_invalid() {
            return Err("no process".into());
        }
        let waited = WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let ok = waited == WAIT_OBJECT_0 && GetExitCodeProcess(info.hProcess, &mut code).is_ok();
        let _ = CloseHandle(info.hProcess);
        if ok { Ok(code) } else { Err("wait failed".into()) }
    }
}

/// The system's file-open dialog for a program (`*.exe`). `None` if cancelled.
pub fn pick_exe(title: &str, start_dir: Option<&Path>) -> Option<PathBuf> {
    open_file(title, &[("程序 (*.exe)", "*.exe")], start_dir, None)
}

/// A top-level window of this process with this title (the settings window, as the owner of
/// a file dialog). Raw `HWND` value; 0 = none.
pub fn own_window(title: &str) -> isize {
    unsafe {
        let mut after = None;
        while let Ok(h) = FindWindowExW(None, after, PCWSTR::null(), &HSTRING::from(title)) {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                return h.0 as isize;
            }
            after = Some(h);
        }
        0
    }
}

/// `filters`: (name, pattern) pairs such as `("文本文件 (*.txt)", "*.txt")`.
fn filter_specs(filters: &[(&str, &str)]) -> (Vec<HSTRING>, Vec<COMDLG_FILTERSPEC>) {
    let strings: Vec<HSTRING> = filters.iter().flat_map(|(n, p)| [HSTRING::from(*n), HSTRING::from(*p)]).collect();
    let specs = strings
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| COMDLG_FILTERSPEC { pszName: PCWSTR(c[0].as_ptr()), pszSpec: PCWSTR(c[1].as_ptr()) })
        .collect();
    (strings, specs)
}

fn owner_hwnd(owner: Option<isize>) -> Option<HWND> {
    owner.filter(|&h| h != 0).map(|h| HWND(h as *mut _))
}

fn result_path(dlg: &IFileDialog) -> windows::core::Result<PathBuf> {
    unsafe {
        let item = dlg.GetResult()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = p.to_string().map(PathBuf::from).unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        Ok(path)
    }
}

fn set_folder(dlg: &IFileDialog, dir: Option<&Path>) {
    if let Some(dir) = dir.filter(|d| d.is_dir())
        && let Ok(item) = unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) }
    {
        let _ = unsafe { dlg.SetFolder(&item) };
    }
}

/// The system's file-open dialog. `owner`: raw `HWND` of the window it belongs to (modal to
/// it). Blocks: call it on a background thread. `None` if cancelled.
pub fn open_file(title: &str, filters: &[(&str, &str)], start_dir: Option<&Path>, owner: Option<isize>) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let picked = (|| -> windows::core::Result<PathBuf> {
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let (_keep, specs) = filter_specs(filters);
            dlg.SetFileTypes(&specs)?;
            dlg.SetTitle(&HSTRING::from(title))?;
            dlg.SetOptions(dlg.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST)?;
            set_folder(&dlg, start_dir);
            dlg.Show(owner_hwnd(owner))?;
            result_path(&dlg)
        })();
        CoUninitialize();
        picked.ok().filter(|p| !p.as_os_str().is_empty())
    }
}

/// The system's save dialog, suggesting `file_name` (extension `default_ext` is added when the
/// user leaves it out). Asks before overwriting. Blocks like [`open_file`].
pub fn save_file(
    title: &str,
    file_name: &str,
    default_ext: &str,
    filters: &[(&str, &str)],
    start_dir: Option<&Path>,
    owner: Option<isize>,
) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let picked = (|| -> windows::core::Result<PathBuf> {
            let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            let (_keep, specs) = filter_specs(filters);
            dlg.SetFileTypes(&specs)?;
            dlg.SetTitle(&HSTRING::from(title))?;
            dlg.SetFileName(&HSTRING::from(file_name))?;
            dlg.SetDefaultExtension(&HSTRING::from(default_ext))?;
            dlg.SetOptions(dlg.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT)?;
            set_folder(&dlg, start_dir);
            dlg.Show(owner_hwnd(owner))?;
            result_path(&dlg)
        })();
        CoUninitialize();
        picked.ok().filter(|p| !p.as_os_str().is_empty())
    }
}
