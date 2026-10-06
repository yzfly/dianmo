//! Checking for updates and upgrading (docs/PRODUCT.md P6).
//!
//! Source: GitHub Releases of `yzfly/dianmo` (`/releases/latest`; the asset `DianmoSetup-*.exe`).
//! Everything runs on short-lived background threads and reports back with
//! `HostProxy::post(UpdateEvent)`; nothing stays resident:
//!
//! ```ignore
//! update::start_auto_check(proxy, settings.last_update_check);  // once, in on_start
//! update::set_auto_check(settings.auto_update);                  // the 「自动检查更新」 switch
//! update::check_async(proxy);                                    // 「检查更新」
//! update::download_and_install(release, proxy);                  // 「立即更新」
//! // App::on_event: event.downcast::<UpdateEvent>() → see `UpdateEvent`
//! ```
//!
//! Automatic checks: 1 minute after start, then at most once a day (`last_update_check`, unix
//! seconds, persisted by the app from `UpdateEvent::Checked::at`). The checking thread sleeps in
//! one-hour steps (one wake-up per hour, no timer on the UI thread) and does nothing while
//! automatic checks are off.
//!
//! Upgrading: the installer is downloaded to `%TEMP%\DianmoSetup-<version>.exe` (size and, when
//! GitHub provides it, SHA-256 verified), then started with `/S`. It closes this 点墨 itself
//! (`dianmo.exe --quit`), replaces the files and starts the new version hidden. The running 点墨 is
//! elevated (scheduled task), so the installer inherits that and never shows a UAC prompt.
//!
//! JSON is read with a small built-in parser (below) instead of serde_json (saves ~100 KB).
//! For tests, `DIANMO_UPDATE_REPO=owner/name` points the check at another repository, and
//! `dianmo.exe --check-update` prints the result.

// Used by app.rs (settings window); some parts only by tests and `--check-update`.
#![allow(dead_code)]

pub const REPO: &str = "yzfly/dianmo";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A newer release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    /// Without the leading `v`, e.g. `0.2.0`.
    pub version: String,
    pub tag: String,
    /// Release notes as plain text (Markdown markers removed, at most ~1200 chars).
    pub notes: String,
    /// `browser_download_url` of `DianmoSetup-*.exe`; empty if the release has no installer
    /// (then open `page` instead).
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the installer, if GitHub lists a digest.
    pub sha256: Option<String>,
    /// The release page (html_url).
    pub page: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CheckOutcome {
    /// `latest` = the newest release's version (≤ ours).
    UpToDate {
        latest: String,
    },
    /// The repository has no (published) release yet, or doesn't exist (yet).
    NoRelease,
    Available(Release),
}

/// Posted to the app (`App::on_event`).
#[derive(Clone, Debug)]
pub enum UpdateEvent {
    /// A check finished. `auto`: from the daily background check (show the tray dot / settings
    /// row, no toast). `at`: unix seconds; store it as `last_update_check`. `Err` is a short
    /// Chinese reason for the settings row.
    Checked { auto: bool, at: u64, outcome: Result<CheckOutcome, String> },
    /// Download progress 0.0..=1.0 (→ `UpdateState::Downloading`).
    Progress(f32),
    /// The installer is running (`/S`); it will close this 点墨 in a moment. Hide the settings
    /// window or show 「正在更新…」; no need to quit (but quitting is fine).
    Installing,
    /// Download or starting the installer failed.
    Failed(String),
}

// ---------------------------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------------------------

/// `v1.2.3-beta.1` → comparable key. Missing parts are 0; a pre-release sorts before the release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
            None => (s, Vec::new()),
        };
        let mut parts = core.split('.');
        let mut n = [0u64; 3];
        for (i, slot) in n.iter_mut().enumerate() {
            match parts.next() {
                Some(p) => *slot = p.parse().ok()?,
                None if i > 0 => {}
                None => return None,
            }
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Version { core: n, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Equal,
            (true, false) => Greater,
            (false, true) => Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Less,
                        (Err(_), Ok(_)) => Greater,
                        _ => a.cmp(b),
                    };
                    if o != Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

/// True if `candidate` is newer than `current` (unparsable → false).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing the GitHub response
// ---------------------------------------------------------------------------------------------

/// Parses a `/releases/latest` response.
pub fn parse_release(body: &str, current: &str) -> Result<CheckOutcome, String> {
    let v = json::parse(body).map_err(|e| format!("GitHub 返回的数据无法解析（{e}）"))?;
    let tag = v.get("tag_name").and_then(json::Value::as_str).ok_or("GitHub 返回的数据里没有版本号")?;
    if v.get("draft").and_then(json::Value::as_bool) == Some(true) {
        return Ok(CheckOutcome::NoRelease);
    }
    let version = tag.trim().trim_start_matches(['v', 'V']).to_owned();
    if !is_newer(&version, current) {
        return Ok(CheckOutcome::UpToDate { latest: version });
    }
    let mut url = String::new();
    let mut size = 0;
    let mut sha256 = None;
    for a in v.get("assets").and_then(json::Value::as_array).unwrap_or(&[]) {
        let name = a.get("name").and_then(json::Value::as_str).unwrap_or("").to_ascii_lowercase();
        if name.starts_with("dianmosetup") && name.ends_with(".exe") {
            url = a.get("browser_download_url").and_then(json::Value::as_str).unwrap_or("").to_owned();
            size = a.get("size").and_then(json::Value::as_f64).unwrap_or(0.0) as u64;
            sha256 = a
                .get("digest")
                .and_then(json::Value::as_str)
                .and_then(|d| d.strip_prefix("sha256:"))
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase);
            break;
        }
    }
    Ok(CheckOutcome::Available(Release {
        version,
        tag: tag.to_owned(),
        notes: plain_notes(v.get("body").and_then(json::Value::as_str).unwrap_or("")),
        url,
        size,
        sha256,
        page: v.get("html_url").and_then(json::Value::as_str).unwrap_or("").to_owned(),
    }))
}

/// Release notes for display: Markdown headings / emphasis / links reduced to text, blank-line
/// runs collapsed, at most ~1200 characters.
pub fn plain_notes(md: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in md.lines() {
        let mut l = strip_tags(line.trim_end().trim_start_matches('#').trim()).trim().to_owned();
        if line.trim_start().starts_with("<!--") || l.starts_with("```") {
            continue;
        }
        for m in ["**", "__", "`"] {
            l = l.replace(m, "");
        }
        // [text](url) → text
        while let (Some(a), Some(b)) = (l.find("]("), l.find('[')) {
            let Some(c) = l[a..].find(')').map(|c| a + c) else { break };
            if b >= a {
                break;
            }
            l = format!("{}{}{}", &l[..b], &l[b + 1..a], &l[c + 1..]);
        }
        if let Some(rest) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            l = format!("· {rest}");
        }
        if l.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&l);
        if out.chars().count() > 1200 {
            let cut: String = out.chars().take(1200).collect();
            return format!("{cut}…");
        }
    }
    out
}

/// Removes HTML tags (`<a id="x"></a>`, `<br>`); keeps text and `<` not starting a tag.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match after.find('>') {
            Some(j) if tag => rest = &after[j + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------------------------
// A minimal JSON reader (enough for the GitHub API)
// ---------------------------------------------------------------------------------------------

pub mod json {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Value>),
        Obj(Vec<(String, Value)>),
    }

    impl Value {
        pub fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
        pub fn as_str(&self) -> Option<&str> {
            if let Value::Str(s) = self { Some(s) } else { None }
        }
        pub fn as_bool(&self) -> Option<bool> {
            if let Value::Bool(b) = self { Some(*b) } else { None }
        }
        pub fn as_f64(&self) -> Option<f64> {
            if let Value::Num(n) = self { Some(*n) } else { None }
        }
        pub fn as_array(&self) -> Option<&[Value]> {
            if let Value::Arr(a) = self { Some(a) } else { None }
        }
    }

    pub fn parse(s: &str) -> Result<Value, String> {
        let mut p = Parser { b: s.as_bytes(), i: 0, depth: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("trailing data at {}", p.i));
        }
        Ok(v)
    }

    struct Parser<'a> {
        b: &'a [u8],
        i: usize,
        depth: u32,
    }

    impl Parser<'_> {
        fn ws(&mut self) {
            while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
                self.i += 1;
            }
        }
        fn err<T>(&self, what: &str) -> Result<T, String> {
            Err(format!("{what} at {}", self.i))
        }
        fn eat(&mut self, lit: &str) -> bool {
            if self.b[self.i..].starts_with(lit.as_bytes()) {
                self.i += lit.len();
                true
            } else {
                false
            }
        }
        fn value(&mut self) -> Result<Value, String> {
            self.ws();
            let Some(&c) = self.b.get(self.i) else { return self.err("unexpected end") };
            match c {
                b'{' | b'[' => {
                    self.depth += 1;
                    if self.depth > 64 {
                        return self.err("too deep");
                    }
                    let v = if c == b'{' { self.object() } else { self.array() };
                    self.depth -= 1;
                    v
                }
                b'"' => self.string().map(Value::Str),
                b't' if self.eat("true") => Ok(Value::Bool(true)),
                b'f' if self.eat("false") => Ok(Value::Bool(false)),
                b'n' if self.eat("null") => Ok(Value::Null),
                b'-' | b'0'..=b'9' => self.number(),
                _ => self.err("unexpected character"),
            }
        }
        fn object(&mut self) -> Result<Value, String> {
            self.i += 1;
            let mut m = Vec::new();
            self.ws();
            if self.eat("}") {
                return Ok(Value::Obj(m));
            }
            loop {
                self.ws();
                if self.b.get(self.i) != Some(&b'"') {
                    return self.err("expected key");
                }
                let k = self.string()?;
                self.ws();
                if !self.eat(":") {
                    return self.err("expected ':'");
                }
                let v = self.value()?;
                m.push((k, v));
                self.ws();
                if self.eat(",") {
                    continue;
                }
                if self.eat("}") {
                    return Ok(Value::Obj(m));
                }
                return self.err("expected ',' or '}'");
            }
        }
        fn array(&mut self) -> Result<Value, String> {
            self.i += 1;
            let mut a = Vec::new();
            self.ws();
            if self.eat("]") {
                return Ok(Value::Arr(a));
            }
            loop {
                a.push(self.value()?);
                self.ws();
                if self.eat(",") {
                    continue;
                }
                if self.eat("]") {
                    return Ok(Value::Arr(a));
                }
                return self.err("expected ',' or ']'");
            }
        }
        fn number(&mut self) -> Result<Value, String> {
            let start = self.i;
            while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                self.i += 1;
            }
            let s = std::str::from_utf8(&self.b[start..self.i]).unwrap_or("");
            s.parse().map(Value::Num).or_else(|_| self.err("bad number"))
        }
        fn hex4(&mut self) -> Result<u32, String> {
            let h = self.b.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok());
            let v = h.and_then(|h| u32::from_str_radix(h, 16).ok());
            match v {
                Some(v) => {
                    self.i += 4;
                    Ok(v)
                }
                None => self.err("bad \\u escape"),
            }
        }
        fn string(&mut self) -> Result<String, String> {
            self.i += 1;
            let mut out = Vec::<u8>::new();
            loop {
                let Some(&c) = self.b.get(self.i) else { return self.err("unterminated string") };
                self.i += 1;
                match c {
                    b'"' => return String::from_utf8(out).or_else(|_| self.err("invalid UTF-8")),
                    b'\\' => {
                        let Some(&e) = self.b.get(self.i) else { return self.err("bad escape") };
                        self.i += 1;
                        let ch = match e {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let hi = self.hex4()?;
                                let code = if (0xD800..0xDC00).contains(&hi) && self.eat("\\u") {
                                    let lo = self.hex4()?;
                                    0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF)
                                } else {
                                    hi
                                };
                                char::from_u32(code).unwrap_or('\u{FFFD}')
                            }
                            _ => return self.err("bad escape"),
                        };
                        let mut buf = [0; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    _ => out.push(c),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windows: HTTP (WinHTTP), checking, downloading
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
#[allow(unused_imports)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use dianmo_win::HostProxy;
    use windows::Win32::Networking::WinHttp::*;
    use windows::Win32::Security::Cryptography::{
        BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
        BCryptHashData,
    };
    use windows::core::{HSTRING, PCWSTR, w};

    use super::{CURRENT, CheckOutcome, REPO, Release, UpdateEvent, parse_release};
    use crate::log;

    static AUTO: AtomicBool = AtomicBool::new(true);
    static CHECKING: AtomicBool = AtomicBool::new(false);
    static DOWNLOADING: AtomicBool = AtomicBool::new(false);
    static AUTO_STARTED: AtomicBool = AtomicBool::new(false);

    const DAY: u64 = 24 * 3600;

    pub fn now_unix() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    fn repo() -> String {
        std::env::var("DIANMO_UPDATE_REPO").ok().filter(|r| r.contains('/')).unwrap_or_else(|| REPO.to_owned())
    }

    /// Turns the daily automatic check on/off (the settings switch). On by default.
    pub fn set_auto_check(on: bool) {
        AUTO.store(on, Ordering::Relaxed);
    }

    /// Starts the automatic check (once per process): first after 1 minute, then whenever a day
    /// has passed since the last check. `last_check` = unix seconds (0 = never).
    pub fn start_auto_check(proxy: HostProxy, last_check: u64) {
        if AUTO_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let spawned =
            std::thread::Builder::new().name("dianmo-update".into()).stack_size(256 * 1024).spawn(move || {
                std::thread::sleep(Duration::from_secs(60));
                let mut last = last_check;
                loop {
                    let now = now_unix();
                    // A clock set back makes `last` look like the future: check then too.
                    if AUTO.load(Ordering::Relaxed)
                        && (now >= last + DAY || last > now + DAY)
                        && !CHECKING.swap(true, Ordering::SeqCst)
                    {
                        let outcome = check();
                        CHECKING.store(false, Ordering::SeqCst);
                        log!("automatic update check: {}", describe(&outcome));
                        last = now;
                        if !proxy.post(UpdateEvent::Checked { auto: true, at: now, outcome }) {
                            return; // the host is gone
                        }
                    }
                    // Hourly: wall-clock based, so sleep/hibernation doesn't stretch the day.
                    std::thread::sleep(Duration::from_secs(3600));
                }
            });
        if let Err(e) = spawned {
            log!("update thread: {e}");
        }
    }

    /// 「检查更新」: checks on a background thread, posts `UpdateEvent::Checked { auto: false, .. }`.
    /// Ignored while a check is running.
    pub fn check_async(proxy: HostProxy) {
        if CHECKING.swap(true, Ordering::SeqCst) {
            return;
        }
        let r = std::thread::Builder::new().name("dianmo-update-check".into()).spawn(move || {
            let outcome = check();
            CHECKING.store(false, Ordering::SeqCst);
            log!("update check: {}", describe(&outcome));
            proxy.post(UpdateEvent::Checked { auto: false, at: now_unix(), outcome });
        });
        if r.is_err() {
            CHECKING.store(false, Ordering::SeqCst);
        }
    }

    fn describe(o: &Result<CheckOutcome, String>) -> String {
        match o {
            Ok(CheckOutcome::Available(r)) => {
                format!("{} available ({} bytes, sha256 {})", r.version, r.size, r.sha256.is_some())
            }
            Ok(other) => format!("{other:?}"),
            Err(e) => format!("failed: {e}"),
        }
    }

    /// Blocking check against GitHub. A missing repository or no release → `NoRelease`.
    pub fn check() -> Result<CheckOutcome, String> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
        let mut body = Vec::new();
        let status =
            get(&url, &["Accept: application/vnd.github+json", "X-GitHub-Api-Version: 2022-11-28"], |chunk, _| {
                if body.len() + chunk.len() > 4 << 20 {
                    return false;
                }
                body.extend_from_slice(chunk);
                true
            })?;
        match status {
            200 => parse_release(&String::from_utf8_lossy(&body), CURRENT),
            404 => Ok(CheckOutcome::NoRelease),
            403 | 429 => Err("GitHub 暂时限制了访问次数，请过一会儿再试".into()),
            s => Err(format!("GitHub 返回 {s}")),
        }
    }

    /// 「立即更新」: downloads the installer (posting `Progress`), verifies it, starts it with `/S`
    /// and posts `Installing` (or `Failed`).
    pub fn download_and_install(release: Release, proxy: HostProxy) {
        if DOWNLOADING.swap(true, Ordering::SeqCst) {
            return;
        }
        let r = std::thread::Builder::new().name("dianmo-update-download".into()).spawn(move || {
            let result = download(&release, &|f| {
                proxy.post(UpdateEvent::Progress(f));
            })
            .and_then(|path| run_installer(&path));
            DOWNLOADING.store(false, Ordering::SeqCst);
            match result {
                Ok(()) => {
                    log!("update {}: installer started", release.version);
                    proxy.post(UpdateEvent::Installing);
                }
                Err(e) => {
                    log!("update {} failed: {e}", release.version);
                    proxy.post(UpdateEvent::Failed(e));
                }
            }
        });
        if r.is_err() {
            DOWNLOADING.store(false, Ordering::SeqCst);
        }
    }

    /// Downloads `release` to `%TEMP%\DianmoSetup-<version>.exe`.
    pub fn download(release: &Release, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
        if release.url.is_empty() {
            return Err("这个版本没有安装包，请到发布页下载".into());
        }
        let name: String = format!("DianmoSetup-{}.exe", release.version)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .collect();
        let path = std::env::temp_dir().join(name);
        let part = path.with_extension("exe.part");
        let mut file = std::fs::File::create(&part).map_err(|e| format!("无法写入临时文件：{e}"))?;
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        let mut last_step = -1i32;
        let mut write_err = None;
        let status = get(&release.url, &["Accept: application/octet-stream"], |chunk, total| {
            if let Err(e) = file.write_all(chunk) {
                write_err = Some(e);
                return false;
            }
            if let Some(h) = hasher.as_mut() {
                h.update(chunk);
            }
            got += chunk.len() as u64;
            let total = if total > 0 { total } else { release.size };
            if let Some(step) = (got * 100).checked_div(total).map(|s| s.min(100) as i32)
                && step != last_step
            {
                last_step = step;
                progress(step as f32 / 100.0);
            }
            true
        });
        drop(file);
        let fail = |msg: String| {
            let _ = std::fs::remove_file(&part);
            Err(msg)
        };
        if let Some(e) = write_err {
            return fail(format!("写入安装包失败：{e}"));
        }
        match status {
            Ok(200) => {}
            Ok(s) => return fail(format!("下载失败（HTTP {s}）")),
            Err(e) => return fail(e),
        }
        if release.size > 0 && got != release.size {
            return fail(format!("下载不完整（{got}/{} 字节）", release.size));
        }
        if let (Some(want), Some(h)) = (&release.sha256, hasher) {
            let have = h.finish_hex();
            if have.as_deref() != Some(want.as_str()) {
                return fail("安装包校验失败（SHA-256 不一致）".into());
            }
        }
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&part, &path).map_err(|e| format!("保存安装包失败：{e}"))?;
        log!("downloaded {} ({got} bytes)", path.display());
        Ok(path)
    }

    fn run_installer(path: &Path) -> Result<(), String> {
        let mut cmd = std::process::Command::new(path);
        cmd.arg("/S");
        if !crate::platform::instance_name().is_empty() {
            cmd.args(["--instance", crate::platform::instance_name()]);
        }
        cmd.spawn().map(|_| ()).map_err(|e| format!("启动安装程序失败：{e}"))
    }

    // -----------------------------------------------------------------------------------------

    /// Owns a WinHTTP handle.
    struct Handle(*mut core::ffi::c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn handle(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
        if h.is_null() {
            let e = windows::core::Error::from_thread();
            log!("WinHTTP {what}: {e}");
            Err(net_error(&e))
        } else {
            Ok(Handle(h))
        }
    }

    fn net_error(e: &windows::core::Error) -> String {
        // WinHTTP errors are 12000+ (HRESULT_FROM_WIN32).
        match (e.code().0 as u32) & 0xFFFF {
            12002 => "连接 GitHub 超时，请检查网络".into(),
            12007 => "找不到 GitHub 服务器（DNS），请检查网络".into(),
            12029 | 12030 => "无法连接 GitHub，请检查网络".into(),
            12175 | 12038 | 12044 | 12045 | 12169 => "与 GitHub 的安全连接失败".into(),
            c => format!("网络错误 {c}"),
        }
    }

    /// `https://host[:port]/path?query` → (host, port, path?query).
    fn split_url(url: &str) -> Result<(String, u16, String), String> {
        let rest = url.strip_prefix("https://").ok_or_else(|| format!("不支持的地址 {url}"))?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().map_err(|_| format!("不支持的地址 {url}"))?),
            None => (authority, 443),
        };
        Ok((host.to_owned(), port, path.to_owned()))
    }

    /// HTTPS GET following redirects; `on_data(chunk, content_length)` gets the body (return false
    /// to stop). Returns the final status code.
    fn get(url: &str, headers: &[&str], mut on_data: impl FnMut(&[u8], u64) -> bool) -> Result<u32, String> {
        let (host, port, path) = split_url(url)?;
        let agent = HSTRING::from(format!("Dianmo/{CURRENT} (+https://github.com/{REPO})"));
        unsafe {
            let session = handle(
                WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
                "open",
            )?;
            let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000);
            let connect = handle(WinHttpConnect(session.0, &HSTRING::from(host), port, 0), "connect")?;
            let request = handle(
                WinHttpOpenRequest(
                    connect.0,
                    w!("GET"),
                    &HSTRING::from(path),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    WINHTTP_FLAG_SECURE,
                ),
                "request",
            )?;
            let hdrs: Vec<u16> = headers.join("\r\n").encode_utf16().collect();
            WinHttpSendRequest(request.0, Some(&hdrs), None, 0, 0, 0).map_err(|e| net_error(&e))?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| net_error(&e))?;
            let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
            if status != 200 {
                return Ok(status);
            }
            let total = query_number(&request, WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0) as u64;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read)
                    .map_err(|e| net_error(&e))?;
                if read == 0 {
                    break;
                }
                if !on_data(&buf[..read as usize], total) {
                    return Err("下载被中止".into());
                }
            }
            Ok(status)
        }
    }

    fn query_number(request: &Handle, what: u32) -> Option<u32> {
        let mut value = 0u32;
        let mut len = 4u32;
        unsafe {
            WinHttpQueryHeaders(
                request.0,
                what | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&mut value as *mut u32).cast()),
                &mut len,
                std::ptr::null_mut(),
            )
            .ok()?;
        }
        Some(value)
    }

    /// Streaming SHA-256 (CNG; `None` if unavailable — then only the size is checked).
    struct Sha256(BCRYPT_HASH_HANDLE);

    impl Sha256 {
        fn new() -> Option<Sha256> {
            let mut h = BCRYPT_HASH_HANDLE::default();
            let status = unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut h, None, None, 0) };
            status.is_ok().then_some(Sha256(h))
        }
        fn update(&mut self, data: &[u8]) {
            unsafe {
                let _ = BCryptHashData(self.0, data, 0);
            }
        }
        fn finish_hex(self) -> Option<String> {
            let mut out = [0u8; 32];
            let ok = unsafe { BCryptFinishHash(self.0, &mut out, 0) }.is_ok();
            ok.then(|| out.iter().map(|b| format!("{b:02x}")).collect())
        }
    }

    impl Drop for Sha256 {
        fn drop(&mut self) {
            unsafe {
                let _ = BCryptDestroyHash(self.0);
            }
        }
    }

    /// `dianmo.exe --check-update`: prints the check result (tests; `DIANMO_UPDATE_REPO`).
    /// `DIANMO_UPDATE_TEST_DOWNLOAD="<url> <size> <sha256>"` instead downloads that file the way an
    /// update would (redirects, size and hash checks), prints the outcome and deletes it.
    pub fn cmd_check() -> i32 {
        crate::platform::attach_parent_console();
        if let Ok(spec) = std::env::var("DIANMO_UPDATE_TEST_DOWNLOAD") {
            let mut p = spec.split_whitespace();
            let release = Release {
                version: "download-test".into(),
                url: p.next().unwrap_or("").into(),
                size: p.next().and_then(|s| s.parse().ok()).unwrap_or(0),
                sha256: p.next().map(str::to_ascii_lowercase),
                ..Release::default()
            };
            let steps = std::cell::Cell::new(0);
            let r = download(&release, &|_| steps.set(steps.get() + 1));
            println!("download: {r:?} ({} progress steps)", steps.get());
            if let Ok(path) = &r {
                let _ = std::fs::remove_file(path);
            }
            return if r.is_ok() { 0 } else { 1 };
        }
        let r = check();
        let line = match &r {
            Ok(CheckOutcome::Available(rel)) => format!(
                "available {} (tag {}) url={} size={} sha256={} page={}\nnotes:\n{}",
                rel.version,
                rel.tag,
                rel.url,
                rel.size,
                rel.sha256.as_deref().unwrap_or("-"),
                rel.page,
                rel.notes.chars().take(300).collect::<String>()
            ),
            Ok(o) => format!("{o:?}"),
            Err(e) => format!("error: {e}"),
        };
        log!("--check-update ({}): {}", repo(), line.lines().next().unwrap_or(""));
        println!("repo {}: {line}", repo());
        if r.is_ok() { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta.2"));
        assert!(is_newer("0.2.0-beta.10", "0.2.0-beta.2"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(is_newer("1.91.1", "0.1.0"));
        assert_eq!(Version::parse("1.2.3.4"), None);
    }

    #[test]
    fn json_parser() {
        let v = json::parse(r#" {"a": [1, -2.5e2, true, null, "x\"é😀\n"], "b": {}} "#).unwrap();
        let a = v.get("a").and_then(json::Value::as_array).unwrap();
        assert_eq!(a[1].as_f64(), Some(-250.0));
        assert_eq!(a[2].as_bool(), Some(true));
        assert_eq!(a[3], json::Value::Null);
        assert_eq!(a[4].as_str(), Some("x\"é😀\n"));
        assert!(json::parse("{\"a\": }").is_err());
        assert!(json::parse("[1,2").is_err());
        assert!(json::parse("{} x").is_err());
        assert!(json::parse(&"[".repeat(100)).is_err(), "depth limit");
        assert_eq!(json::parse("\"中文\"").unwrap().as_str(), Some("中文"));
    }

    const SAMPLE: &str = r###"{
      "html_url": "https://github.com/yzfly/dianmo/releases/tag/v0.2.0",
      "tag_name": "v0.2.0", "draft": false, "prerelease": false,
      "body": "## 新功能\r\n\r\n- **设置窗口**：见 [文档](https://x/y)\r\n- 安装包\r\n\r\n<!-- hidden -->\r\n### 修复\r\n* 语音模式",
      "assets": [
        {"name": "notes.txt", "browser_download_url": "https://x/notes.txt", "size": 10},
        {"name": "DianmoSetup-0.2.0.exe", "size": 25165824,
         "digest": "sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
         "browser_download_url": "https://github.com/yzfly/dianmo/releases/download/v0.2.0/DianmoSetup-0.2.0.exe"}
      ]
    }"###;

    #[test]
    fn release_parsing() {
        let CheckOutcome::Available(r) = parse_release(SAMPLE, "0.1.0").unwrap() else { panic!() };
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.tag, "v0.2.0");
        assert!(r.url.ends_with("/DianmoSetup-0.2.0.exe"));
        assert_eq!(r.size, 25165824);
        assert_eq!(r.sha256.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(r.notes, "新功能\n\n· 设置窗口：见 文档\n· 安装包\n\n修复\n· 语音模式");
        assert_eq!(parse_release(SAMPLE, "0.2.0").unwrap(), CheckOutcome::UpToDate { latest: "0.2.0".into() });
        assert_eq!(parse_release(SAMPLE, "0.3.0").unwrap(), CheckOutcome::UpToDate { latest: "0.2.0".into() });
        // No installer asset: still "available", url empty (the app opens the release page).
        let no_asset = SAMPLE.replace("DianmoSetup-0.2.0.exe\", \"size\"", "other.zip\", \"size\"");
        let CheckOutcome::Available(r) = parse_release(&no_asset, "0.1.0").unwrap() else { panic!() };
        assert!(r.url.is_empty());
        assert!(parse_release("{\"message\": \"Not Found\"}", "0.1.0").is_err());
        assert!(parse_release("<html>", "0.1.0").is_err());
        assert_eq!(
            plain_notes("<a id=\"1.99.0-Language\"></a>\n\nLanguage\n--------\n- a < b <br>"),
            "Language\n--------\n· a < b"
        );
    }
}
