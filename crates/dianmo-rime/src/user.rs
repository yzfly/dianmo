//! Per-user Rime data: the fuzzy pinyin user build and the user dictionary.
//!
//! User directory layout (`Options::user_data_dir`, e.g. `%APPDATA%\Dianmo\rime`):
//! ```text
//! rime_ice.userdb\              learned words (LevelDB), shared by every schema
//! <schema>.custom.yaml          fuzzy pinyin patch (set_fuzzy), one per schema in FUZZY_SCHEMAS
//! build\                        user build in use: customized <schema>.schema.yaml + .prism.bin
//!     dianmo-build.txt          stamp of what it was built from (custom::stamp)
//! build.new\                    finished deploy_user output, swapped in by start/reload
//! build.lock                    held by deploy_user while it runs
//! build.dev\                    developer fallback when the shared build is incomplete
//! ```

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, SetPriorityClass};

use crate::api::{self, Levers, Traits, TraitsSpec};
use crate::custom::{self, FUZZY_SCHEMAS, Fuzzy};
use crate::engine::{DeployReport, Error, Options, check_deployer_allowed, io_err, rime_err, run_deployer};

pub(crate) const BUILD: &str = "build";
pub(crate) const BUILD_NEW: &str = "build.new";
pub(crate) const BUILD_DEV: &str = "build.dev";
const LOCK: &str = "build.lock";
const MARKER: &str = "dianmo-build.txt";
/// The user dictionary every schema shares (`translator/dictionary: rime_ice`).
const USER_DICT: &std::ffi::CStr = c"rime_ice";
const USERDB_DIR: &str = "rime_ice.userdb";

fn custom_path(user: &Path, schema: &str) -> PathBuf {
    user.join(format!("{schema}.custom.yaml"))
}

/// The customization files that exist, sorted by name, with their contents.
fn custom_files(user: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = FUZZY_SCHEMAS
        .iter()
        .filter_map(|s| {
            let name = format!("{s}.custom.yaml");
            std::fs::read_to_string(user.join(&name)).ok().map(|t| (name, t))
        })
        .collect();
    v.sort();
    v
}

/// Names and sizes of `<shared>\build`, sorted.
fn shared_signature(shared: &Path) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(shared.join(BUILD))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    m.is_file().then(|| (e.file_name().to_string_lossy().into_owned(), m.len()))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The stamp a user build must have to match the current customization; `None` when there is
/// no customization (the shared build is all that's needed).
fn wanted_stamp(opts: &Options) -> Option<String> {
    let custom = custom_files(&opts.user_data_dir);
    if custom.is_empty() {
        return None;
    }
    Some(custom::stamp(&shared_signature(&opts.shared_data_dir), &custom))
}

fn marker(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(MARKER)).ok().map(|s| s.trim().to_string())
}

/// Takes `<user>\build.lock` exclusively; `None` if another process holds it.
fn try_lock(user: &Path) -> Option<File> {
    OpenOptions::new().create(true).truncate(false).write(true).share_mode(0).open(user.join(LOCK)).ok()
}

/// Called before `initialize` (nothing of the user build is mapped then): promotes a finished
/// `build.new`, drops builds that no longer match, and returns the user build to use, if any.
pub(crate) fn prepare(opts: &Options) -> Option<PathBuf> {
    let user = &opts.user_data_dir;
    let build = user.join(BUILD);
    let new = user.join(BUILD_NEW);
    let want = wanted_stamp(opts);
    // Promote or drop build.new, unless a deploy is writing it right now (holds the lock).
    if new.exists()
        && let Some(_lock) = try_lock(user)
    {
        if want.is_some() && marker(&new) == want {
            let _ = std::fs::remove_dir_all(&build);
            if std::fs::rename(&new, &build).is_err() {
                // build\ couldn't be replaced; use the new one where it is.
                return Some(new);
            }
        } else {
            let _ = std::fs::remove_dir_all(&new);
        }
    }
    match want {
        Some(w) if marker(&build).as_deref() == Some(w.as_str()) => Some(build),
        Some(_) => None, // stale: shared build until deploy_user has run
        None => {
            if build.exists() {
                let _ = std::fs::remove_dir_all(&build);
            }
            None
        }
    }
}

/// Whether the fuzzy customization has no matching user build yet (the app should run
/// `dianmo.exe --deploy-user`, then [`crate::RimeEngine::reload`]). Cheap (a few file reads);
/// check it at startup too: a new app version invalidates the user build.
pub fn needs_user_deploy(opts: &Options) -> bool {
    let Some(want) = wanted_stamp(opts) else { return false };
    let user = &opts.user_data_dir;
    marker(&user.join(BUILD)).as_deref() != Some(want.as_str())
        && marker(&user.join(BUILD_NEW)).as_deref() != Some(want.as_str())
}

/// What the app has to do after [`set_fuzzy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzyChange {
    /// Nothing changed.
    Unchanged,
    /// Call [`crate::RimeEngine::reload`] (e.g. everything turned off, or a matching build exists).
    Reload,
    /// Run `dianmo.exe --deploy-user` (seconds, below-normal priority), then reload on exit code 0.
    Deploy,
}

/// The fuzzy pairs currently written to the user directory.
pub fn fuzzy(opts: &Options) -> Fuzzy {
    std::fs::read_to_string(custom_path(&opts.user_data_dir, FUZZY_SCHEMAS[0]))
        .map(|t| custom::parse_custom_yaml(&t))
        .unwrap_or_default()
}

/// Writes (or removes, when all off) `<user>\<schema>.custom.yaml` for every keyboard schema.
/// Doesn't touch librime; the returned [`FuzzyChange`] says what to do next.
pub fn set_fuzzy(opts: &Options, fuzzy: Fuzzy) -> Result<FuzzyChange, Error> {
    let user = &opts.user_data_dir;
    std::fs::create_dir_all(user).map_err(|e| io_err("创建", user, e))?;
    let text = custom::custom_yaml(&fuzzy);
    let mut changed = false;
    for schema in FUZZY_SCHEMAS {
        let p = custom_path(user, schema);
        let old = std::fs::read_to_string(&p).ok();
        match &text {
            Some(t) if old.as_deref() != Some(t.as_str()) => {
                std::fs::write(&p, t).map_err(|e| io_err("写入", &p, e))?;
                changed = true;
            }
            None if old.is_some() => {
                std::fs::remove_file(&p).map_err(|e| io_err("删除", &p, e))?;
                changed = true;
            }
            _ => {}
        }
    }
    Ok(if needs_user_deploy(opts) {
        FuzzyChange::Deploy
    } else if changed {
        FuzzyChange::Reload
    } else {
        FuzzyChange::Unchanged
    })
}

/// Builds the user build for the current customization into `<user>\build.new` (the running
/// engine picks it up on [`crate::RimeEngine::reload`] or the next start).
///
/// Only the customized schemas are rebuilt (`deploy_schema` each): their compiled
/// `*.schema.yaml` and `*.prism.bin`; dictionaries (`*.table.bin`, `*.reverse.bin`) and
/// everything else are read from `<shared>\build` as prebuilt data. Needs the schema sources
/// (`<shared>\*.schema.yaml`, `default.yaml`, `symbols_*.yaml`; shipped by stage.ps1).
///
/// One-shot like [`crate::deploy`]: run it in its own process (`dianmo.exe --deploy-user`).
/// Lowers the process priority to below normal. Concurrent runs queue on `build.lock`.
pub fn deploy_user(opts: &Options) -> Result<DeployReport, Error> {
    let t0 = Instant::now();
    check_deployer_allowed()?;
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
    }
    let user = &opts.user_data_dir;
    let shared = &opts.shared_data_dir;
    std::fs::create_dir_all(user).map_err(|e| io_err("创建", user, e))?;
    let _lock = {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(l) = try_lock(user) {
                break l;
            }
            if Instant::now() > deadline {
                return Err(Error::Rime("另一个部署一直没有结束（build.lock）".into()));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let new = user.join(BUILD_NEW);
    let done = |bytes| DeployReport { millis: t0.elapsed().as_millis() as u64, build_bytes: bytes };
    let Some(want) = wanted_stamp(opts) else {
        let _ = std::fs::remove_dir_all(&new);
        return Ok(done(0));
    };
    if [user.join(BUILD), new.clone()].iter().any(|d| marker(d).as_deref() == Some(want.as_str())) {
        return Ok(done(0));
    }
    let schemas: Vec<&str> =
        FUZZY_SCHEMAS.iter().copied().filter(|s| custom_path(user, s).is_file()).collect();
    let prebuilt = shared.join(BUILD);
    for s in &schemas {
        let src = shared.join(format!("{s}.schema.yaml"));
        if !src.is_file() {
            return Err(Error::Rime(format!("缺少方案源文件 {}（数据包太旧？）", src.display())));
        }
    }
    let api = api::load(&opts.dll).map_err(Error::Load)?;
    if new.exists() {
        std::fs::remove_dir_all(&new).map_err(|e| io_err("删除", &new, e))?;
    }
    std::fs::create_dir_all(&new).map_err(|e| io_err("创建", &new, e))?;
    if let Some(d) = &opts.log_dir {
        std::fs::create_dir_all(d).map_err(|e| io_err("创建", d, e))?;
    }
    let traits = Traits::new(&TraitsSpec {
        shared,
        user,
        staging: &new,
        prebuilt: &prebuilt,
        log_dir: opts.log_dir.as_deref(),
        min_log_level: opts.min_log_level,
    })
    .map_err(rime_err)?;
    let sources: Vec<_> =
        schemas.iter().map(|s| api::path_arg(&shared.join(format!("{s}.schema.yaml")))).collect::<Result<_, _>>().map_err(rime_err)?;
    let failed: Vec<&str> = run_deployer(api, &traits, |api| {
        schemas
            .iter()
            .zip(&sources)
            .filter(|(_, src)| unsafe { (api.deploy_schema.unwrap())(src.as_ptr()) } == 0)
            .map(|(s, _)| *s)
            .collect()
    })?;
    let fail = |msg: String| {
        let _ = std::fs::remove_dir_all(&new);
        Err(Error::Rime(msg))
    };
    if !failed.is_empty() {
        return fail(format!("部署方案失败：{}（见日志）", failed.join(", ")));
    }
    // Every schema compiled with its prism (rime_ice's prism is named after the dictionary,
    // the others after the schema via translator/prism), and no dictionary was rebuilt: without
    // dictionary sources a rebuild would produce an empty table.
    for s in &schemas {
        for f in [format!("{s}.schema.yaml"), format!("{s}.prism.bin")] {
            if !new.join(&f).is_file() {
                return fail(format!("部署后缺少 {f}"));
            }
        }
    }
    let unexpected: Vec<String> = std::fs::read_dir(&new)
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<String>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|n: &String| !(n.ends_with(".schema.yaml") || n.ends_with(".prism.bin")))
        .collect();
    if !unexpected.is_empty() {
        return fail(format!("部署产生了意外的文件：{}", unexpected.join(", ")));
    }
    let m = new.join(MARKER);
    std::fs::write(&m, format!("{want}\n")).map_err(|e| io_err("写入", &m, e))?;
    Ok(done(api::dir_size(&new)))
}

// ---- user dictionary --------------------------------------------------------------------------

const EXPORT_HEADER: &str = "# Rime user dictionary export\n";

/// `export_user_dict` through the levers API (the user dictionary must not be open).
pub(crate) fn export_with(levers: Levers, user: &Path, path: &Path) -> Result<usize, Error> {
    if !user.join(USERDB_DIR).exists() {
        std::fs::write(path, EXPORT_HEADER).map_err(|e| io_err("写入", path, e))?;
        return Ok(0);
    }
    // librime opens a path that doesn't exist yet by its UTF-8 name; create it first so a
    // non-ASCII directory can use its short name.
    File::create(path).map_err(|e| io_err("创建", path, e))?;
    let p = api::path_arg(path).map_err(rime_err)?;
    let n = unsafe { (levers.export_user_dict.unwrap())(USER_DICT.as_ptr(), p.as_ptr()) };
    if n < 0 {
        return Err(Error::Rime("导出用户词库失败（词库被占用或已损坏）".into()));
    }
    Ok(n as usize)
}

/// `import_user_dict` through the levers API. The file is normalised first (UTF-8 BOM and CRLF
/// removed) into a temp copy; non-UTF-8 files are refused.
pub(crate) fn import_with(levers: Levers, path: &Path) -> Result<usize, Error> {
    let raw = std::fs::read(path).map_err(|e| io_err("读取", path, e))?;
    let bytes: &[u8] = raw.strip_prefix(b"\xef\xbb\xbf".as_slice()).unwrap_or(&raw);
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Rime("词库文件不是 UTF-8 文本".into()))?;
    let tmp = std::env::temp_dir().join(format!("dianmo-import-{}.txt", std::process::id()));
    let mut f = File::create(&tmp).map_err(|e| io_err("创建", &tmp, e))?;
    f.write_all(text.replace("\r\n", "\n").as_bytes()).map_err(|e| io_err("写入", &tmp, e))?;
    drop(f);
    let p = api::path_arg(&tmp).map_err(rime_err);
    let n = p.map(|p| unsafe { (levers.import_user_dict.unwrap())(USER_DICT.as_ptr(), p.as_ptr()) });
    let _ = std::fs::remove_file(&tmp);
    match n? {
        n if n < 0 => Err(Error::Rime("导入用户词库失败（格式不对，或词库被占用）".into())),
        n => Ok(n as usize),
    }
}

/// Entry count via an export to a temp file.
pub(crate) fn count_with(levers: Levers, user: &Path) -> Result<usize, Error> {
    let tmp = std::env::temp_dir().join(format!("dianmo-count-{}.txt", std::process::id()));
    let r = export_with(levers, user, &tmp);
    let _ = std::fs::remove_file(&tmp);
    r
}

/// Deletes the user dictionary files (librime must not have them open).
pub(crate) fn remove_userdb(user: &Path) -> Result<(), Error> {
    let db = user.join(USERDB_DIR);
    if db.exists() {
        std::fs::remove_dir_all(&db).map_err(|e| io_err("删除", &db, e))?;
    }
    let _ = std::fs::remove_file(user.join("rime_ice.userdb.txt"));
    Ok(())
}

/// Runs `f` with librime initialized for `opts` but no engine in this process.
fn without_engine<R>(opts: &Options, f: impl FnOnce(Levers) -> Result<R, Error>) -> Result<R, Error> {
    let api = api::load(&opts.dll).map_err(Error::Load)?;
    let mut state = api::state();
    if state.engines > 0 {
        return Err(Error::Rime("引擎正在运行：请用 RimeEngine 的同名方法".into()));
    }
    if state.deployed {
        return Err(Error::Rime("这个进程已经运行过部署".into()));
    }
    if !state.initialized {
        crate::engine::initialize(api, &mut state, opts)?;
    }
    let levers = api::levers(api).map_err(Error::Rime)?;
    f(levers)
}

/// [`crate::RimeEngine::export_user_dict`] for a process without a running engine.
pub fn export_user_dict(opts: &Options, path: &Path) -> Result<usize, Error> {
    without_engine(opts, |l| export_with(l, &opts.user_data_dir, path))
}

/// [`crate::RimeEngine::import_user_dict`] for a process without a running engine.
pub fn import_user_dict(opts: &Options, path: &Path) -> Result<usize, Error> {
    without_engine(opts, |l| import_with(l, path))
}

/// [`crate::RimeEngine::user_word_count`] for a process without a running engine.
pub fn user_word_count(opts: &Options) -> Option<usize> {
    if !opts.user_data_dir.join(USERDB_DIR).exists() {
        return Some(0);
    }
    without_engine(opts, |l| count_with(l, &opts.user_data_dir)).ok()
}

/// [`crate::RimeEngine::clear_user_dict`] for a process without a running engine.
pub fn clear_user_dict(opts: &Options) -> Result<(), Error> {
    if api::state().engines > 0 {
        return Err(Error::Rime("引擎正在运行：请用 RimeEngine::clear_user_dict".into()));
    }
    remove_userdb(&opts.user_data_dir)
}
