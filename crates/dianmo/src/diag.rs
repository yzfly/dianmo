//! Feedback and diagnostics (docs/PRODUCT.md P7).
//!
//! ```ignore
//! diag::open_url(&diag::report_issue_url());   // 「反馈问题」: GitHub new-issue page, prefilled
//! match diag::export() {                        // 「导出诊断包」: zip on the desktop
//!     Ok(path) => { toast("已导出到桌面"); diag::reveal(&path); }
//!     Err(e) => toast(e),
//! }
//! diag::open_url(url)  // any link (about page): opens the browser as the user, not elevated
//! ```
//!
//! The issue body carries version, Windows version and build, screen resolution and scaling,
//! whether 点墨 runs elevated, and the voice engine. The diagnostics zip (`点墨诊断-<日期>.zip`)
//! holds `info.txt` (the same plus install / task state), `dianmo.log` (+ `.old`), `settings.ini`
//! and the installer log; never the clipboard history (`clips.txt`) or the Rime user dictionary.
//! The user profile path is replaced by `%USERPROFILE%` in every file.

// Used by app.rs (settings window); some parts only by tests and `--check-update`.
#![allow(dead_code)]

pub const ISSUES_NEW: &str = "https://github.com/yzfly/dianmo/issues/new";

/// What the issue template and `info.txt` show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemInfo {
    pub version: String,
    /// e.g. `Windows 10 Enterprise LTSC 2021 21H2（19044.5371）`
    pub windows: String,
    /// e.g. `2880×1920，缩放 200%`
    pub screen: String,
    pub elevated: bool,
    pub voice_engine: String,
}

fn voice_label(engine: &str) -> &str {
    match engine {
        "wetype" => "微信输入法",
        "doubao_ime" => "豆包输入法",
        "doubao" => "豆包语音（第三方）",
        "system" => "系统语音（Win+H）",
        other => other,
    }
}

/// The prefilled issue body (Markdown).
pub fn issue_body(info: &SystemInfo) -> String {
    format!(
        "**遇到了什么问题**\n\n（请描述问题和复现步骤，可以附截图。方便的话，在「设置 → 关于 → 导出诊断包」后把桌面上的 zip 拖到这里。）\n\n\
         **期望的结果**\n\n\n\
         ---\n\
         - 点墨版本：{}\n\
         - 系统：{}\n\
         - 屏幕：{}\n\
         - 管理员模式：{}\n\
         - 语音引擎：{}\n",
        info.version,
        info.windows,
        info.screen,
        if info.elevated { "是" } else { "否" },
        voice_label(&info.voice_engine)
    )
}

/// `https://github.com/yzfly/dianmo/issues/new?title=&body=…`
pub fn issue_url(info: &SystemInfo) -> String {
    format!("{ISSUES_NEW}?title=&body={}", percent_encode(&issue_body(info)))
}

/// URL query component encoding (UTF-8, RFC 3986 unreserved kept).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Replaces every occurrence of `needle` (ASCII case-insensitive) with `with`.
pub fn redact(text: &str, needle: &str, with: &str) -> String {
    if needle.len() < 4 {
        return text.to_owned();
    }
    let (t, n) = (text.as_bytes(), needle.as_bytes());
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if t.len() - i >= n.len() && t[i..i + n.len()].eq_ignore_ascii_case(n) {
            out.extend_from_slice(with.as_bytes());
            i += n.len();
        } else {
            out.push(t[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

// ---------------------------------------------------------------------------------------------
// Zip (stored, no compression: logs are small) — pure, tested on Linux
// ---------------------------------------------------------------------------------------------

pub mod zip {
    fn crc32(data: &[u8]) -> u32 {
        static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
        let table = TABLE.get_or_init(|| {
            let mut t = [0u32; 256];
            for (i, slot) in t.iter_mut().enumerate() {
                let mut c = i as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                *slot = c;
            }
            t
        });
        !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
    }

    /// DOS date/time fields from (year, month, day, hour, minute, second).
    pub fn dos_time(y: u16, mo: u16, d: u16, h: u16, mi: u16, s: u16) -> (u16, u16) {
        let date = (y.saturating_sub(1980) << 9) | (mo << 5) | d;
        let time = (h << 11) | (mi << 5) | (s / 2);
        (date, time)
    }

    /// A zip archive of `(name, data)` entries, stored (method 0), UTF-8 names.
    pub fn write(entries: &[(String, Vec<u8>)], (date, time): (u16, u16)) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let (n, len) = (name.as_bytes(), data.len() as u32);
            // Local header.
            le32(&mut out, 0x0403_4b50);
            le16(&mut out, 20); // version needed
            le16(&mut out, 0x0800); // UTF-8 names
            le16(&mut out, 0); // stored
            le16(&mut out, time);
            le16(&mut out, date);
            le32(&mut out, crc);
            le32(&mut out, len);
            le32(&mut out, len);
            le16(&mut out, n.len() as u16);
            le16(&mut out, 0);
            out.extend_from_slice(n);
            out.extend_from_slice(data);
            // Central directory entry.
            le32(&mut central, 0x0201_4b50);
            le16(&mut central, 20); // made by
            le16(&mut central, 20);
            le16(&mut central, 0x0800);
            le16(&mut central, 0);
            le16(&mut central, time);
            le16(&mut central, date);
            le32(&mut central, crc);
            le32(&mut central, len);
            le32(&mut central, len);
            le16(&mut central, n.len() as u16);
            le16(&mut central, 0); // extra
            le16(&mut central, 0); // comment
            le16(&mut central, 0); // disk
            le16(&mut central, 0); // internal attrs
            le32(&mut central, 0); // external attrs
            le32(&mut central, offset);
            central.extend_from_slice(n);
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        le32(&mut out, 0x0605_4b50);
        le16(&mut out, 0);
        le16(&mut out, 0);
        le16(&mut out, entries.len() as u16);
        le16(&mut out, entries.len() as u16);
        le32(&mut out, central.len() as u32);
        le32(&mut out, cd_offset);
        le16(&mut out, 0);
        out
    }

    #[cfg(test)]
    pub(super) fn crc(data: &[u8]) -> u32 {
        crc32(data)
    }
}

// ---------------------------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
#[allow(unused_imports)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};

    use windows::Win32::Foundation::{POINT, RPC_E_CHANGED_MODE, WIN32_ERROR};
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint,
    };
    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch,
        IServiceProvider,
    };
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, SID_STopLevelBrowser,
        SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellExecuteW, ShellWindows,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS, SW_SHOWNORMAL};
    use windows::core::{BSTR, HSTRING, Interface, PCWSTR, w};

    use super::{SystemInfo, issue_url, redact, zip};
    use crate::settings::Settings;
    use crate::{elevate, install, log, platform};

    /// Collects what the issue template shows (cheap: registry and display queries).
    pub fn system_info() -> SystemInfo {
        let settings = Settings::load(&platform::data_dir().join("settings.ini"));
        SystemInfo {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            windows: windows_version(),
            screen: screen(),
            elevated: elevate::is_elevated(),
            voice_engine: settings.voice_engine,
        }
    }

    /// 「反馈问题」: the prefilled new-issue URL.
    pub fn report_issue_url() -> String {
        issue_url(&system_info())
    }

    fn reg_sz(name: &str) -> Option<String> {
        let mut buf = vec![0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
                &HSTRING::from(name),
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
        let s = String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').trim().to_owned();
        (!s.is_empty()).then_some(s)
    }

    fn reg_dword(name: &str) -> Option<u32> {
        let mut v = 0u32;
        let mut size = 4u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
                &HSTRING::from(name),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut v as *mut u32).cast()),
                Some(&mut size),
            )
        };
        (err == WIN32_ERROR(0)).then_some(v)
    }

    /// `Windows 10 Enterprise LTSC 2021 21H2（19044.5371）`. ProductName still says "Windows 10"
    /// on Windows 11, so the build number decides.
    pub fn windows_version() -> String {
        let build = reg_sz("CurrentBuild").or_else(|| reg_sz("CurrentBuildNumber")).unwrap_or_default();
        let mut product = reg_sz("ProductName").unwrap_or_else(|| "Windows".into());
        if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
            product = product.replace("Windows 10", "Windows 11");
        }
        let release = reg_sz("DisplayVersion").or_else(|| reg_sz("ReleaseId")).unwrap_or_default();
        let ubr = reg_dword("UBR").map(|u| format!(".{u}")).unwrap_or_default();
        let release = if release.is_empty() { String::new() } else { format!(" {release}") };
        format!("{product}{release}（{build}{ubr}）")
    }

    /// `2880×1920，缩放 200%` (primary monitor; `，共 2 个显示器` if more).
    pub fn screen() -> String {
        unsafe {
            let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
            let res = if EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
                format!("{}×{}", dm.dmPelsWidth, dm.dmPelsHeight)
            } else {
                "未知分辨率".into()
            };
            // The real DPI only reaches DPI-aware callers (the CLI `--diagnostics` isn't).
            let old = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let (mut dx, mut dy) = (0u32, 0u32);
            let scale = match GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) {
                Ok(()) if dx > 0 => format!("，缩放 {}%", (dx * 100 + 48) / 96),
                _ => String::new(),
            };
            if !old.0.is_null() {
                SetThreadDpiAwarenessContext(old);
            }
            let n = GetSystemMetrics(SM_CMONITORS);
            let more = if n > 1 { format!("，共 {n} 个显示器") } else { String::new() };
            format!("{res}{scale}{more}")
        }
    }

    fn profile() -> Option<String> {
        std::env::var("USERPROFILE").ok().filter(|p| p.len() > 3)
    }

    fn scrub(text: &str) -> String {
        match profile() {
            Some(p) => redact(text, &p, "%USERPROFILE%"),
            None => text.to_owned(),
        }
    }

    fn info_txt(info: &SystemInfo) -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        let task = platform::task_name();
        let task_state = match elevate::query(&task) {
            Ok(Some(t)) => format!("{}（{}，登录时启动 {}）", task, t.exe.display(), t.logon_trigger),
            Ok(None) => format!("{task}：未注册"),
            Err(e) => format!("{task}：读取失败 {e}"),
        };
        let data = platform::data_dir();
        let mut files: Vec<String> = std::fs::read_dir(&data)
            .map(|d| {
                d.flatten()
                    .map(|e| {
                        let len = e
                            .metadata()
                            .map(|m| if m.is_dir() { "目录".to_owned() } else { format!("{} 字节", m.len()) });
                        format!("  {}  {}", e.file_name().to_string_lossy(), len.unwrap_or_default())
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let t = unsafe { GetLocalTime() };
        format!(
            "点墨诊断信息\n导出时间：{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n\n\
             点墨版本：{}\n系统：{}\n屏幕：{}\n管理员模式：{}\n语音引擎：{}\n\n\
             程序：{}\n实例：{}\n计划任务：{}\n数据目录：{}\n{}\n",
            t.wYear,
            t.wMonth,
            t.wDay,
            t.wHour,
            t.wMinute,
            t.wSecond,
            info.version,
            info.windows,
            info.screen,
            if info.elevated { "是" } else { "否" },
            info.voice_engine,
            exe.display(),
            if platform::instance_name().is_empty() { "（默认）" } else { platform::instance_name() },
            task_state,
            data.display(),
            files.join("\n")
        )
    }

    /// 「导出诊断包」: writes `点墨诊断-<日期>-<时间>.zip` to the desktop and returns its path.
    pub fn export() -> Result<PathBuf, String> {
        let info = system_info();
        let data = platform::data_dir();
        let mut entries: Vec<(String, Vec<u8>)> = vec![("info.txt".into(), scrub(&info_txt(&info)).into_bytes())];
        let text_files = [
            ("dianmo.log", data.join("dianmo.log")),
            ("dianmo.log.old", data.join("dianmo.log.old")),
            ("settings.ini", data.join("settings.ini")),
            ("setup.log", std::env::temp_dir().join("DianmoSetup.log")),
        ];
        for (name, path) in text_files {
            if let Ok(bytes) = std::fs::read(&path) {
                entries.push((name.into(), scrub(&String::from_utf8_lossy(&bytes)).into_bytes()));
            }
        }
        let t = unsafe { GetLocalTime() };
        let zip = zip::write(&entries, zip::dos_time(t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond));
        let desktop = install::known_folder(&FOLDERID_Desktop)
            .or_else(|| profile().map(|p| PathBuf::from(p).join("Desktop")))
            .ok_or("找不到桌面文件夹")?;
        let stem = format!("点墨诊断-{:04}{:02}{:02}-{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute);
        let path = (1..100)
            .map(|n| desktop.join(if n == 1 { format!("{stem}.zip") } else { format!("{stem}-{n}.zip") }))
            .find(|p| !p.exists())
            .ok_or("桌面上同名文件太多")?;
        let tmp = path.with_extension("zip.tmp");
        std::fs::write(&tmp, zip).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("写入诊断包失败：{e}")
        })?;
        log!("diagnostics exported to {} ({} files)", path.display(), entries.len());
        Ok(path)
    }

    /// `dianmo.exe --diagnostics`: prints the issue URL and exports the zip (support, tests).
    pub fn cmd_diagnostics() -> i32 {
        platform::attach_parent_console();
        println!("{}", report_issue_url());
        match export() {
            Ok(p) => {
                println!("{}", p.display());
                0
            }
            Err(e) => {
                println!("export failed: {e}");
                1
            }
        }
    }

    /// Opens `url` in the default browser. When 点墨 is elevated the browser is started through
    /// Explorer (as the user, not as administrator). Returns at once (a short thread does it).
    pub fn open_url(url: &str) {
        shell_open_async(url.to_owned(), String::new());
    }

    /// Opens an Explorer window with `path` selected.
    pub fn reveal(path: &Path) {
        shell_open_async("explorer.exe".into(), format!("/select,\"{}\"", path.display()));
    }

    fn shell_open_async(file: String, args: String) {
        let r = std::thread::Builder::new().name("dianmo-shell-open".into()).spawn(move || {
            if let Err(e) = shell_open(&file, &args) {
                log!("opening {file} {args}: {e}");
            }
        });
        if let Err(e) = r {
            log!("shell thread: {e}");
        }
    }

    /// `ShellExecute(open)`; when elevated, through the desktop's Explorer so the started program
    /// runs unelevated (falls back to a plain ShellExecute).
    pub fn shell_open(file: &str, args: &str) -> windows::core::Result<()> {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let balance = hr.is_ok() && hr != RPC_E_CHANGED_MODE;
        let result = (|| {
            if elevate::is_elevated() {
                match shell_execute_as_user(file, args) {
                    Ok(()) => return Ok(()),
                    Err(e) => log!("unelevated ShellExecute failed ({e}); opening directly"),
                }
            }
            let args = HSTRING::from(args);
            let r = unsafe {
                ShellExecuteW(
                    None,
                    w!("open"),
                    &HSTRING::from(file),
                    if args.is_empty() { PCWSTR::null() } else { PCWSTR(args.as_ptr()) },
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                )
            };
            if r.0 as isize > 32 { Ok(()) } else { Err(windows::core::Error::from_thread()) }
        })();
        if balance {
            unsafe { CoUninitialize() };
        }
        result
    }

    /// Raymond Chen's "launch unelevated from elevated": ask the desktop window's shell view for
    /// its `IShellDispatch2` and call its ShellExecute (it runs inside Explorer).
    fn shell_execute_as_user(file: &str, args: &str) -> windows::core::Result<()> {
        unsafe {
            // Each step says where it failed (the chain crosses into Explorer's process).
            let step = |what: &'static str| move |e: windows::core::Error| windows::core::Error::new(e.code(), format!("{what}: {}", e.message()));
            let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER).map_err(step("ShellWindows"))?;
            let empty = VARIANT::default();
            let mut hwnd = 0i32;
            let disp: IDispatch =
                windows.FindWindowSW(&empty, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH).map_err(step("FindWindowSW"))?;
            let sp: IServiceProvider = disp.cast().map_err(step("IServiceProvider"))?;
            let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser).map_err(step("QueryService"))?;
            let view = browser.QueryActiveShellView().map_err(step("QueryActiveShellView"))?;
            // As IDispatch first, then the dual interface (asking the view for
            // IShellFolderViewDual directly fails with E_NOINTERFACE across processes).
            let view_disp: IDispatch = view.GetItemObject(SVGIO_BACKGROUND).map_err(step("GetItemObject"))?;
            let folder_view: IShellFolderViewDual = view_disp.cast().map_err(step("IShellFolderViewDual"))?;
            let shell: IShellDispatch2 =
                folder_view.Application().map_err(step("Application"))?.cast().map_err(step("IShellDispatch2"))?;
            shell.ShellExecute(
                &BSTR::from(file),
                &VARIANT::from(args),
                &VARIANT::from(""),
                &VARIANT::from("open"),
                &VARIANT::from(1i32), // SW_SHOWNORMAL
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_url_is_encoded() {
        let info = SystemInfo {
            version: "0.1.0".into(),
            windows: "Windows 10 Enterprise LTSC 2021 21H2（19044.5371）".into(),
            screen: "2880×1920，缩放 200%".into(),
            elevated: true,
            voice_engine: "wetype".into(),
        };
        let body = issue_body(&info);
        assert!(body.contains("- 点墨版本：0.1.0\n"));
        assert!(body.contains("- 管理员模式：是\n"));
        assert!(body.contains("- 语音引擎：微信输入法\n"));
        let url = issue_url(&info);
        assert!(url.starts_with("https://github.com/yzfly/dianmo/issues/new?title=&body=%2A%2A"));
        assert!(url.contains("2880%C3%971920"));
        let body = &url[url.find("body=").unwrap() + 5..];
        assert!(!body.contains([' ', '\n', '&', '#', '=']));
        assert_eq!(percent_encode("a b&c=中"), "a%20b%26c%3D%E4%B8%AD");
    }

    #[test]
    fn redaction() {
        let t = r"open C:\Users\wecode\AppData\x; c:\users\WECODE\y; C:\Users\wecodex";
        assert_eq!(
            redact(t, r"C:\Users\wecode", "%USERPROFILE%"),
            r"open %USERPROFILE%\AppData\x; %USERPROFILE%\y; %USERPROFILE%x"
        );
        assert_eq!(redact("中文 C:\\Users\\张三\\a", "C:\\Users\\张三", "%USERPROFILE%"), "中文 %USERPROFILE%\\a");
        assert_eq!(redact("abc", "", "x"), "abc");
    }

    #[test]
    fn zip_layout() {
        assert_eq!(zip::crc(b"123456789"), 0xCBF4_3926);
        let entries = vec![("info.txt".to_owned(), b"hello".to_vec()), ("dianmo.log".to_owned(), vec![])];
        let z = zip::write(&entries, zip::dos_time(2026, 10, 6, 22, 15, 30));
        assert_eq!(&z[..4], b"PK\x03\x04");
        // End of central directory: 2 entries, central dir right after the data.
        let eocd = z.len() - 22;
        assert_eq!(&z[eocd..eocd + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([z[eocd + 10], z[eocd + 11]]), 2);
        let cd_off = u32::from_le_bytes(z[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(&z[cd_off..cd_off + 4], b"PK\x01\x02");
        assert_eq!(&z[30..38], b"info.txt");
        assert_eq!(&z[38..43], b"hello");
        let (date, time) = zip::dos_time(2026, 10, 6, 22, 15, 30);
        assert_eq!(date >> 9, 46);
        assert_eq!(time >> 11, 22);
    }
}
