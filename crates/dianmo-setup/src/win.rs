//! The installation itself (see main.rs for the steps).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use dianmo_setup::payload;
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, WAIT_OBJECT_0};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    SHGetKnownFolderPath, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `dianmo.exe --quit` exit codes (crates/dianmo/src/install.rs).
const QUIT_STILL_RUNNING: u32 = 1;
const QUIT_ACCESS_DENIED: u32 = 3;
/// `dianmo.exe --install`: installed, but the scheduled task couldn't be registered (UAC declined).
const INSTALL_NO_TASK: u32 = 2;

pub struct Opts {
    pub silent: bool,
    pub no_run: bool,
    pub instance: String,
    pub root: PathBuf,
}

impl Opts {
    fn parse() -> Opts {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut o = Opts { silent: false, no_run: false, instance: String::new(), root: PathBuf::new() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.to_ascii_lowercase().as_str() {
                "/s" | "-s" | "/silent" | "/verysilent" | "--silent" | "/quiet" | "--quiet" => o.silent = true,
                "/norun" | "--no-run" => o.no_run = true,
                "--instance" => {
                    o.instance =
                        it.next().map(|s| s.chars().filter(char::is_ascii_alphanumeric).collect()).unwrap_or_default()
                }
                other => log(&format!("ignoring argument {other:?}")),
            }
        }
        o.root = match std::env::var_os("DIANMO_INSTALL_ROOT").filter(|r| !r.is_empty()) {
            Some(r) => PathBuf::from(r),
            None => {
                let name = if o.instance.is_empty() { "Dianmo".to_owned() } else { format!("Dianmo-{}", o.instance) };
                local_app_data().join(name)
            }
        };
        o
    }

    fn instance_args(&self) -> Vec<String> {
        if self.instance.is_empty() { vec![] } else { vec!["--instance".into(), self.instance.clone()] }
    }
}

pub fn main() -> i32 {
    let opts = Opts::parse();
    log(&format!(
        "DianmoSetup {VERSION} → {} (silent {}, instance {:?})",
        opts.root.display(),
        opts.silent,
        opts.instance
    ));
    if opts.silent {
        match install(&opts, &|_| {}) {
            Ok(note) => {
                log(&format!("done{}", note.map(|n| format!(" ({n})")).unwrap_or_default()));
                0
            }
            Err(e) => {
                log(&format!("failed: {e}"));
                1
            }
        }
    } else {
        crate::ui::run(opts)
    }
}

/// Installs; `progress` gets 0..=1000. `Ok(Some(note))` = installed with a caveat for the user.
pub fn install(opts: &Opts, progress: &(dyn Fn(u32) + Sync)) -> Result<Option<String>, String> {
    let root = &opts.root;
    let new = sibling(root, ".new");
    cleanup_leftovers(root);

    // 1. Extract.
    let exe = std::env::current_exe().map_err(|e| format!("找不到安装包自身：{e}"))?;
    let mut f = File::open(&exe).map_err(|e| format!("读取安装包失败：{e}"))?;
    if new.exists() {
        std::fs::remove_dir_all(&new).map_err(|e| format!("清理 {} 失败：{e}", new.display()))?;
    }
    let extracted = payload::extract(&mut f, &new, |done, total| {
        progress(done.checked_mul(800).and_then(|d| d.checked_div(total)).unwrap_or(800) as u32);
    });
    let info = match extracted {
        Ok(i) => i,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&new);
            return Err(if e.kind() == std::io::ErrorKind::InvalidData {
                format!("安装包已损坏，请重新下载（{e}）")
            } else {
                format!("解压失败：{e}")
            });
        }
    };
    log(&format!("extracted {} files ({} bytes), version {}", info.count, info.total, info.version));
    let new_exe = new.join("dianmo.exe");
    if !new_exe.is_file() {
        let _ = std::fs::remove_dir_all(&new);
        return Err("安装包里没有 dianmo.exe".into());
    }

    // 2. Close the running 点墨.
    progress(840);
    let mut quit_args = vec!["--quit".to_owned()];
    quit_args.extend(opts.instance_args());
    let mut code = run_wait(&new_exe, &quit_args);
    if code == Ok(QUIT_ACCESS_DENIED) {
        log("the running 点墨 is elevated; closing it with administrator rights");
        code = runas_wait(&new_exe, &quit_args);
    }
    match code {
        Ok(0) => {}
        Ok(c) => {
            let _ = std::fs::remove_dir_all(&new);
            log(&format!("--quit exit {c}"));
            return Err(if c == QUIT_STILL_RUNNING || c == QUIT_ACCESS_DENIED {
                "点墨正在运行，没能关闭它。请在托盘菜单里选「退出」后重新运行安装程序。".into()
            } else {
                format!("关闭正在运行的点墨失败（{c}）")
            });
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&new);
            return Err(e);
        }
    }

    // 3. Swap directories.
    progress(880);
    swap(root, &new)?;
    progress(920);

    // 4. Register (shortcuts, uninstall entry, task) and start.
    let mut args = vec!["--install".to_owned()];
    if opts.silent {
        args.push("--quiet".into());
    }
    if opts.no_run {
        args.push("--no-run".into());
    }
    args.extend(opts.instance_args());
    let code = run_wait(&root.join("dianmo.exe"), &args)?;
    progress(1000);
    match code {
        0 => Ok(None),
        INSTALL_NO_TASK => Ok(Some("没有获得管理员权限，点墨暂时不能给管理员窗口输入；可以在设置里「修复」。".into())),
        c => Err(format!("登记点墨失败（{c}），详见 %APPDATA%\\Dianmo\\dianmo.log")),
    }
}

/// `<root>` → `<root>.old`, `<new>` → `<root>`; on failure the old directory is put back.
fn swap(root: &Path, new: &Path) -> Result<(), String> {
    if !root.exists() {
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
        }
        return std::fs::rename(new, root).map_err(|e| format!("安装到 {} 失败：{e}", root.display()));
    }
    let old = free_name(&sibling(root, ".old"));
    match std::fs::rename(root, &old) {
        Ok(()) => match std::fs::rename(new, root) {
            Ok(()) => {
                if let Err(e) = std::fs::remove_dir_all(&old) {
                    log(&format!("leaving {} (removed next time): {e}", old.display()));
                }
                Ok(())
            }
            Err(e) => {
                let back = std::fs::rename(&old, root);
                let _ = std::fs::remove_dir_all(new);
                log(&format!("rename new → root failed: {e}; restored old: {back:?}"));
                Err(format!("安装到 {} 失败：{e}", root.display()))
            }
        },
        Err(e) => {
            // Something holds the directory (e.g. a command prompt inside it): copy over instead.
            log(&format!("can't rename {} ({e}); copying over it", root.display()));
            let r = copy_tree(new, root);
            let _ = std::fs::remove_dir_all(new);
            r.map_err(|e| format!("覆盖 {} 失败：{e}（是否有程序正在使用点墨的文件？）", root.display()))
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn sibling(root: &Path, suffix: &str) -> PathBuf {
    let mut name = root.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "Dianmo".into());
    name.push(suffix);
    root.with_file_name(name)
}

/// `path`, or `path.<n>` if `path` exists and can't be removed.
fn free_name(path: &Path) -> PathBuf {
    if !path.exists() || std::fs::remove_dir_all(path).is_ok() {
        return path.to_owned();
    }
    (1..).map(|n| sibling(path, &format!(".{n}"))).find(|p| !p.exists()).unwrap()
}

/// Removes `<root>.old*` left by earlier runs.
fn cleanup_leftovers(root: &Path) {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else { return };
    let prefix = format!("{}.old", name.to_string_lossy());
    if let Ok(entries) = std::fs::read_dir(parent) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------------------------

/// Runs `exe args` and waits; returns its exit code.
fn run_wait(exe: &Path, args: &[String]) -> Result<u32, String> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    log(&format!("run {} {}", exe.display(), args.join(" ")));
    let status = Command::new(exe)
        .args(args)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("运行 {} 失败：{e}", exe.display()))?;
    let code = status.code().unwrap_or(-1) as u32;
    log(&format!("  exit {code}"));
    Ok(code)
}

/// Runs `exe args` elevated (UAC prompt unless we already are) and waits. A declined prompt is
/// returned as exit code `QUIT_ACCESS_DENIED`.
fn runas_wait(exe: &Path, args: &[String]) -> Result<u32, String> {
    let params = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    log(&format!("runas {} {params}", exe.display()));
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(params);
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
            if e.code() == ERROR_CANCELLED.to_hresult() {
                log("  UAC declined");
                return Ok(QUIT_ACCESS_DENIED);
            }
            return Err(format!("以管理员身份运行失败：{e}"));
        }
        if info.hProcess.is_invalid() {
            return Err("以管理员身份运行失败".into());
        }
        let mut code = 1u32;
        if WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
        log(&format!("  exit {code}"));
        Ok(code)
    }
}

fn quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        a.to_owned()
    } else {
        format!("\"{}\"", a.replace('"', "\\\""))
    }
}

fn local_app_data() -> PathBuf {
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

// ---------------------------------------------------------------------------------------------
// Log: %TEMP%\DianmoSetup.log
// ---------------------------------------------------------------------------------------------

static LOG: Mutex<Option<File>> = Mutex::new(None);

pub fn log(msg: &str) {
    let Ok(mut g) = LOG.lock() else { return };
    if g.is_none() {
        let path = std::env::temp_dir().join("DianmoSetup.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
            let _ = std::fs::remove_file(&path);
        }
        *g = OpenOptions::new().create(true).append(true).open(path).ok();
    }
    if let Some(f) = g.as_mut() {
        let t = unsafe { GetLocalTime() };
        let _ = writeln!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} [{}] {msg}",
            t.wYear,
            t.wMonth,
            t.wDay,
            t.wHour,
            t.wMinute,
            t.wSecond,
            t.wMilliseconds,
            std::process::id()
        );
    }
}
