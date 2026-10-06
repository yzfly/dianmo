//! `DianmoSetup-<version>.exe`: installs 点墨 for the current user, without questions.
//!
//!   DianmoSetup.exe                 install with a small progress window, then start 点墨
//!   DianmoSetup.exe /S              silent (updates): no window, start 点墨 hidden afterwards
//!   DianmoSetup.exe /NORUN          don't start 点墨 afterwards
//!   DianmoSetup.exe --instance <n>  (tests) separate instance: task `Dianmo<n>`, uninstall entry
//!                                   `Dianmo<n>`, shortcuts 「点墨 <n>」, default root
//!                                   `%LOCALAPPDATA%\Dianmo-<n>`
//!   env DIANMO_INSTALL_ROOT=<dir>   (tests) install there instead of `%LOCALAPPDATA%\Dianmo`
//!
//! Steps (the stub only moves files; everything the system has to know about lives in
//! `dianmo.exe --install`, next to `--uninstall`, in crates/dianmo/src/install.rs):
//! 1. extract the payload to `<root>.new` (a sibling, so the final step is a rename);
//! 2. `<root>.new\dianmo.exe --quit`: the running 点墨 exits gracefully (never killed: that would
//!    leave the AppBar's screen space reserved and the system keyboard settings changed). If it is
//!    elevated and we are not and it can't be reached, the same command runs elevated (UAC);
//! 3. `<root>` → `<root>.old`, `<root>.new` → `<root>` (rolled back on failure; if `<root>` can't
//!    be renamed, files are copied over it instead), then `<root>.old` is deleted;
//! 4. `<root>\dianmo.exe --install`: shortcuts, the 「应用和功能」 entry, the elevated scheduled task
//!    (asks for administrator rights only if the task is missing) and starting 点墨.
//!
//! The installer runs as the invoking user (manifest `asInvoker`): everything is per user, and the
//! only step needing administrator rights (the scheduled task) is elevated on its own, so a
//! declined UAC prompt still leaves a working (non-elevated) 点墨, updates started by the
//! (elevated) running 点墨 never prompt, and 点墨 is started as the user, not as an administrator.
//! Log: `%TEMP%\DianmoSetup.log`.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    std::process::exit(win::main());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("DianmoSetup 只能在 Windows 上运行");
    std::process::exit(1);
}
