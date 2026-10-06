//! Safe wrapper over `rime.dll` and the [`dianmo_core::Engine`] implementation.
//!
//! Runtime layout: librime reads the precompiled `<shared>\build` directly (it is both the
//! staging and the prebuilt dir), so startup never compiles anything and never writes to the
//! install directory. Per-user state (the learning user dictionaries) goes to `user_data_dir`.

use std::ffi::CStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dianmo_core::{Candidate, Engine, Schema, Snapshot};

use crate::api::{self, Session, Traits, TraitsSpec};
use crate::ffi::keysym;
use crate::{comment, t9};

/// Where librime and its data live.
#[derive(Clone, Debug)]
pub struct Options {
    /// `rime.dll`.
    pub dll: PathBuf,
    /// Shared data: schemas, `build\` (precompiled), `lua\`, `opencc\`, ...
    pub shared_data_dir: PathBuf,
    /// Per-user data (user dictionaries, sync). Created if missing.
    pub user_data_dir: PathBuf,
    /// glog output; `None` disables file logging.
    pub log_dir: Option<PathBuf>,
    /// 0 info, 1 warning, 2 error, 3 fatal.
    pub min_log_level: i32,
}

impl Options {
    /// Installed-app layout: `<exe dir>\rime.dll`, `<exe dir>\data\rime`, `%APPDATA%\Dianmo\rime`,
    /// no log files, warnings and up.
    pub fn for_app() -> Options {
        let exe = api::exe_dir();
        let user = std::env::var_os("APPDATA")
            .map(|d| PathBuf::from(d).join("Dianmo").join("rime"))
            .unwrap_or_else(|| exe.join("user").join("rime"));
        Options {
            dll: exe.join("rime.dll"),
            shared_data_dir: exe.join("data").join("rime"),
            user_data_dir: user,
            log_dir: None,
            min_log_level: 1,
        }
    }

    /// Same layout rooted at `dir` instead of the exe directory (user data stays in `%APPDATA%`).
    pub fn in_dir(dir: &Path) -> Options {
        Options { dll: dir.join("rime.dll"), shared_data_dir: dir.join("data").join("rime"), ..Options::for_app() }
    }
}

#[derive(Debug)]
pub enum Error {
    /// `rime.dll` missing or not loadable, or `rime_get_api` absent / ABI mismatch.
    Load(String),
    /// librime setup / deploy / session failure.
    Rime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Load(m) => write!(f, "无法加载 rime.dll：{m}"),
            Error::Rime(m) => write!(f, "librime 出错：{m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result of [`deploy`].
#[derive(Clone, Debug)]
pub struct DeployReport {
    pub millis: u64,
    /// Total size of `<shared>\build` in bytes.
    pub build_bytes: u64,
}

/// The Rime schema behind each Dianmo schema.
pub fn schema_id(schema: Schema) -> &'static CStr {
    match schema {
        Schema::Pinyin => c"rime_ice",
        Schema::Shuangpin => c"double_pinyin_flypy",
        Schema::T9 => c"t9",
    }
}

const ALL_SCHEMAS: [Schema; 3] = [Schema::Pinyin, Schema::Shuangpin, Schema::T9];

/// Candidates in a [`Snapshot`] (the candidate bar); the grid pages through [`Engine::candidates`].
const FIRST_BATCH: usize = 30;
/// Candidates whose comments feed the T9 spelling column.
const T9_SCAN: usize = 60;
/// Multi-letter spellings in the T9 column (single letters are appended after them).
const T9_SPELLINGS: usize = 12;

fn rime_err(e: impl fmt::Display) -> Error {
    Error::Rime(e.to_string())
}

fn io_err(what: &str, p: &Path, e: std::io::Error) -> Error {
    Error::Rime(format!("{what} {}: {e}", p.display()))
}

/// `build\` holds a usable precompiled set: `default.yaml` and every schema.
fn precompiled(build: &Path) -> bool {
    build.join("default.yaml").is_file()
        && ALL_SCHEMAS.iter().all(|&s| build.join(format!("{}.schema.yaml", schema_id(s).to_str().unwrap())).is_file())
}

/// Precompiles schemas and dictionaries into `<shared>\build` (packaging step, not at runtime).
///
/// Any existing `build\` is removed first (full rebuild). Uses a throwaway user directory, so
/// nothing outside `<shared>` is touched. One-shot: run it in its own process (e.g. the app's
/// `--deploy` mode); afterwards neither `deploy` nor [`RimeEngine::start`] works in that process.
pub fn deploy(opts: &Options) -> Result<DeployReport, Error> {
    const ONE_SHOT: &str = "deploy 要在单独的进程里运行（每个进程一次，且不能启动过 RimeEngine）";
    let t0 = Instant::now();
    {
        let state = api::STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.initialized || state.finalized {
            return Err(Error::Rime(ONE_SHOT.into()));
        }
    }
    let api = api::load(&opts.dll).map_err(Error::Load)?;
    let shared = &opts.shared_data_dir;
    if !shared.join("default.yaml").is_file() {
        return Err(Error::Rime(format!("{} 里没有 default.yaml", shared.display())));
    }
    let build = shared.join("build");
    if build.exists() {
        std::fs::remove_dir_all(&build).map_err(|e| io_err("删除", &build, e))?;
    }
    std::fs::create_dir_all(&build).map_err(|e| io_err("创建", &build, e))?;
    let tmp = std::env::temp_dir().join(format!("dianmo-deploy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| io_err("创建", &tmp, e))?;
    if let Some(d) = &opts.log_dir {
        std::fs::create_dir_all(d).map_err(|e| io_err("创建", d, e))?;
    }
    // A prebuilt dir that doesn't exist: nothing is reused, everything is compiled into build\.
    let no_prebuilt = tmp.join("no-prebuilt");
    let traits = Traits::new(&TraitsSpec {
        shared,
        user: &tmp,
        staging: &build,
        prebuilt: &no_prebuilt,
        log_dir: opts.log_dir.as_deref(),
        min_log_level: opts.min_log_level,
    })
    .map_err(rime_err)?;

    let ok = {
        let mut state = api::STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.initialized || state.finalized {
            return Err(Error::Rime(ONE_SHOT.into()));
        }
        api::setup_once(api, &mut state, &traits);
        let mut raw = traits.raw();
        let ok = unsafe {
            (api.deployer_initialize.unwrap())(&mut raw);
            let ok = (api.deploy.unwrap())() != 0;
            (api.finalize.unwrap())();
            ok
        };
        state.finalized = true;
        ok
    };
    let _ = std::fs::remove_dir_all(&tmp);
    if !ok {
        return Err(Error::Rime("deploy() 失败（见日志）".into()));
    }
    if !precompiled(&build) {
        return Err(Error::Rime(format!("部署后 {} 不完整（缺 default.yaml 或方案）", build.display())));
    }
    Ok(DeployReport { millis: t0.elapsed().as_millis() as u64, build_bytes: api::dir_size(&build) })
}

/// Shuts librime down (flushes and closes user dictionaries). Optional, for a clean exit:
/// call after every [`RimeEngine`] is dropped; no engine can be started afterwards.
pub fn shutdown() {
    let mut state = api::STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.initialized {
        if let Ok(api) = api::loaded() {
            unsafe { (api.finalize.unwrap())() };
        }
        state.initialized = false;
        state.finalized = true;
    }
}

/// One librime session. Create once per process, use from one thread.
pub struct RimeEngine {
    schema: Schema,
    session: Session,
}

impl RimeEngine {
    /// Loads librime (once per process), initializes it with the precompiled `build\` data and
    /// opens a session on `schema`.
    pub fn start(opts: &Options, schema: Schema) -> Result<RimeEngine, Error> {
        let api = api::load(&opts.dll).map_err(Error::Load)?;
        {
            let mut state = api::STATE.lock().unwrap_or_else(|e| e.into_inner());
            if state.finalized {
                return Err(Error::Rime("librime 已经 shutdown".into()));
            }
            if !state.initialized {
                initialize(api, &mut state, opts)?;
            }
        }
        let session = Session::create(api).ok_or_else(|| Error::Rime("create_session 失败".into()))?;
        if !session.select_schema(schema_id(schema)) {
            return Err(Error::Rime(format!("无法选择方案 {:?}", schema_id(schema))));
        }
        let engine = RimeEngine { schema, session };
        engine.warm_up();
        Ok(engine)
    }

    /// The first key after selecting a schema costs 30–50 ms (librime loads the schema's
    /// dictionaries, filters and Lua scripts lazily). Pay it here instead of on the user's
    /// first keystroke.
    fn warm_up(&self) {
        let key = if self.schema == Schema::T9 { '6' } else { 'n' };
        self.session.process_key(key as i32);
        let _ = self.session.candidates(0, FIRST_BATCH);
        self.session.clear();
        let _ = self.session.take_commit();
    }

    /// librime's version string.
    pub fn rime_version(&self) -> String {
        self.session.version()
    }

    /// The raw composition input (`"64426"`, `"ni'426"`), for diagnostics.
    pub fn raw_input(&self) -> String {
        self.session.input()
    }

    /// Current state, consuming any pending commit text.
    fn state(&mut self) -> Snapshot {
        let commit = self.session.take_commit();
        let mut s = self.current();
        s.commit = commit;
        s
    }

    fn current(&self) -> Snapshot {
        let mut ctx = self.session.context();
        if ctx.preedit.is_empty() {
            return Snapshot::default();
        }
        // The keyboard's default is always candidate 0. librime moves its highlight elsewhere
        // when Backspace reopens an earlier selection; move it back so the preedit describes
        // candidate 0 ("jin tian tian qi hen hao", not "jin tiantianqihenhao").
        if ctx.highlighted != 0 && self.session.highlight(0) {
            ctx = self.session.context();
        }
        let raw = self.session.candidates(0, FIRST_BATCH);
        let preedit = if self.schema == Schema::T9 {
            let first = raw.first().map(|(_, c)| c.as_str());
            t9::display_preedit(&ctx.preedit, ctx.sel_start, ctx.sel_end, first)
        } else {
            ctx.preedit
        };
        Snapshot { preedit, candidates: self.to_candidates(raw), commit: None }
    }

    fn to_candidates(&self, raw: Vec<(String, String)>) -> Vec<Candidate> {
        let t9 = self.schema == Schema::T9;
        raw.into_iter().map(|(text, c)| Candidate { text, comment: comment::display(&c, t9) }).collect()
    }

    /// T9: byte offset where the unconfirmed input starts.
    fn t9_unconfirmed(&self) -> (String, usize) {
        let input = self.session.input();
        let ctx = self.session.context();
        let start = t9::unconfirmed_start(&input, &ctx.preedit, ctx.sel_start);
        (input, start)
    }
}

/// `initialize` against `<shared>\build`. Without a complete `build\` (developer setups) it
/// falls back to a maintenance run that compiles into `<user>\build` (slow, once).
fn initialize(api: api::Api, state: &mut api::State, opts: &Options) -> Result<(), Error> {
    let shared = &opts.shared_data_dir;
    let user = &opts.user_data_dir;
    std::fs::create_dir_all(user).map_err(|e| io_err("创建", user, e))?;
    if let Some(d) = &opts.log_dir {
        std::fs::create_dir_all(d).map_err(|e| io_err("创建", d, e))?;
    }
    let build = shared.join("build");
    let ready = precompiled(&build);
    let user_build = user.join("build");
    let traits = Traits::new(&TraitsSpec {
        shared,
        user,
        staging: if ready { &build } else { &user_build },
        prebuilt: &build,
        log_dir: opts.log_dir.as_deref(),
        min_log_level: opts.min_log_level,
    })
    .map_err(rime_err)?;
    api::setup_once(api, state, &traits);
    let mut raw = traits.raw();
    unsafe {
        (api.initialize.unwrap())(&mut raw);
        if !ready {
            (api.start_maintenance.unwrap())(1);
            (api.join_maintenance_thread.unwrap())();
        }
    }
    state.initialized = true;
    Ok(())
}

impl Engine for RimeEngine {
    fn schema(&self) -> Schema {
        self.schema
    }

    fn set_schema(&mut self, schema: Schema) {
        self.session.clear();
        let _ = self.session.take_commit();
        if schema != self.schema && self.session.select_schema(schema_id(schema)) {
            self.schema = schema;
            self.warm_up();
        }
    }

    fn input(&mut self, c: char) -> Snapshot {
        if c.is_ascii_graphic() {
            self.session.process_key(c as i32);
        }
        self.state()
    }

    fn backspace(&mut self) -> Snapshot {
        self.session.process_key(keysym::BACKSPACE);
        self.state()
    }

    fn select(&mut self, index: usize) -> Snapshot {
        self.session.select_candidate(index);
        self.state()
    }

    fn commit_raw(&mut self) -> Snapshot {
        let before = self.current();
        if !before.is_composing() {
            return before;
        }
        if self.schema == Schema::T9 {
            // Digits mean nothing to the user: commit the pinyin the preedit shows.
            self.session.clear();
            let _ = self.session.take_commit();
            return Snapshot { commit: Some(t9::raw_commit(&before.preedit)), ..Snapshot::default() };
        }
        // rime-ice binds Return to commit_raw_input (confirmed text + the rest as typed).
        self.session.process_key(keysym::RETURN);
        let s = self.state();
        if s.commit.is_some() || !s.is_composing() {
            return s;
        }
        // Not bound after all: commit the preedit letters ourselves.
        self.session.clear();
        let _ = self.session.take_commit();
        Snapshot { commit: Some(t9::raw_commit(&before.preedit)), ..Snapshot::default() }
    }

    fn clear(&mut self) -> Snapshot {
        self.session.clear();
        let _ = self.session.take_commit();
        self.current()
    }

    fn candidates(&mut self, start: usize, count: usize) -> Vec<Candidate> {
        let raw = self.session.candidates(start, count);
        self.to_candidates(raw)
    }

    fn t9_spellings(&mut self) -> Vec<String> {
        if self.schema != Schema::T9 {
            return Vec::new();
        }
        let (input, start) = self.t9_unconfirmed();
        let run = t9::digit_run(&input, start);
        if run.is_empty() {
            return Vec::new();
        }
        let skip = t9::locked_syllables(&input, start, run.start);
        let raw = self.session.candidates(0, T9_SCAN);
        t9::spellings(&input[run], raw.iter().map(|(_, c)| t9::skip_syllables(c, skip)), T9_SPELLINGS)
    }

    fn pick_t9_spelling(&mut self, spelling: &str) -> Snapshot {
        if self.schema == Schema::T9 {
            let (input, start) = self.t9_unconfirmed();
            // set_input keeps the segments confirmed before the first changed byte, so earlier
            // selections survive.
            if let Some(new) = t9::pick_input(&input, start, spelling) {
                self.session.set_input(&new);
            }
        }
        self.state()
    }

    fn snapshot(&mut self) -> Snapshot {
        self.current()
    }
}
