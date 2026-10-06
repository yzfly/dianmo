//! Shell helpers for the settings window: running `dianmo.exe` elevated through UAC and picking a
//! program file. (Links and folders open through `diag::open_url`, never elevated.)
//!
//! Both block: call them on short-lived background threads.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SEE_MASK_NOCLOSEPROCESS, SHCreateItemFromParsingName,
    SHELLEXECUTEINFOW, SIGDN_FILESYSPATH, ShellExecuteExW,
};
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
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let picked = (|| -> windows::core::Result<PathBuf> {
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let filters = [COMDLG_FILTERSPEC { pszName: w!("程序 (*.exe)"), pszSpec: w!("*.exe") }];
            dlg.SetFileTypes(&filters)?;
            dlg.SetTitle(&HSTRING::from(title))?;
            if let Some(dir) = start_dir.filter(|d| d.is_dir())
                && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
            {
                let _ = dlg.SetFolder(&item);
            }
            dlg.Show(None)?;
            let item = dlg.GetResult()?;
            let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = p.to_string().map(PathBuf::from).unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            Ok(path)
        })();
        CoUninitialize();
        picked.ok().filter(|p| !p.as_os_str().is_empty())
    }
}
