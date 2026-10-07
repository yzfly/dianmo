//! Safe wrapper over `rime.dll` and the [`dianmo_core::Engine`] implementation.
//!
//! Runtime layout: librime reads the precompiled `<shared>\build` directly (it is both the
//! staging and the prebuilt dir), so startup never compiles anything and never writes to the
//! install directory. Per-user state (the learning user dictionaries) goes to `user_data_dir`.
//! With fuzzy pinyin on, `<user>\build` (made by [`crate::deploy_user`]) holds the customized
//! schemas and prisms; it is the staging dir and `<shared>\build` the fallback for everything else.

use std::ffi::CStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dianmo_core::{Candidate, Engine, Schema, Snapshot};

use crate::api::{self, Session, Traits, TraitsSpec};
use crate::custom::ShuangpinScheme;
use crate::ffi::keysym;
use crate::{comment, t9, user};

/// Where librime and its data live.
#[derive(Clone, Debug)]
pub struct Options {
    /// `rime.dll`.
    pub dll: PathBuf,
    /// Shared data: schemas, `build\` (precompiled), `lua\`, `opencc\`, ...
    pub shared_data_dir: PathBuf,
    /// Per-user data (user dictionaries, fuzzy pinyin build). Created if missing.
    pub user_data_dir: PathBuf,
    /// glog output; `None` disables file logging.
    pub log_dir: Option<PathBuf>,
    /// 0 info, 1 warning, 2 error, 3 fatal.
    pub min_log_level: i32,
    /// The double pinyin scheme behind `Schema::Shuangpin`.
    pub shuangpin: ShuangpinScheme,
}

impl Options {
    /// Installed-app layout: `<exe dir>\rime.dll`, `<exe dir>\data\rime`, `%APPDATA%\Dianmo\rime`,
    /// no log files, warnings and up, 小鹤双拼.
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
            shuangpin: ShuangpinScheme::Flypy,
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
    /// librime setup / deploy / session / user dictionary failure.
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

/// Result of [`deploy`] / [`crate::deploy_user`].
#[derive(Clone, Debug)]
pub struct DeployReport {
    pub millis: u64,
    /// Total size of the directory that was built, in bytes (0: nothing had to be built).
    pub build_bytes: u64,
}

/// The Rime schema behind each Dianmo schema (`Shuangpin`: the default scheme, 小鹤).
pub fn schema_id(schema: Schema) -> &'static CStr {
    rime_schema(schema, ShuangpinScheme::Flypy)
}

/// The Rime schema behind `schema` with double pinyin scheme `sp`.
pub fn rime_schema(schema: Schema, sp: ShuangpinScheme) -> &'static CStr {
    match schema {
        Schema::Pinyin => c"rime_ice",
        Schema::T9 => c"t9",
        Schema::Shuangpin => match sp {
            ShuangpinScheme::Flypy => c"double_pinyin_flypy",
            ShuangpinScheme::Ziranma => c"double_pinyin",
            ShuangpinScheme::Mspy => c"double_pinyin_mspy",
            ShuangpinScheme::Sogou => c"double_pinyin_sogou",
        },
    }
}

/// Every schema the keyboard can select (all must be in the precompiled set).
fn all_schema_ids() -> impl Iterator<Item = &'static CStr> {
    [rime_schema(Schema::Pinyin, ShuangpinScheme::Flypy), rime_schema(Schema::T9, ShuangpinScheme::Flypy)]
        .into_iter()
        .chain(ShuangpinScheme::ALL.into_iter().map(|sp| rime_schema(Schema::Shuangpin, sp)))
}

/// Candidates in a [`Snapshot`] (the candidate bar); the grid pages through [`Engine::candidates`].
const FIRST_BATCH: usize = 30;
/// Candidates whose comments feed the T9 spelling column.
const T9_SCAN: usize = 60;
/// Multi-letter spellings in the T9 column (single letters are appended after them).
const T9_SPELLINGS: usize = 12;

pub(crate) fn rime_err(e: impl fmt::Display) -> Error {
    Error::Rime(e.to_string())
}

pub(crate) fn io_err(what: &str, p: &Path, e: std::io::Error) -> Error {
    Error::Rime(format!("{what} {}: {e}", p.display()))
}

/// `build\` holds a usable precompiled set: `default.yaml` and every schema.
fn precompiled(build: &Path) -> bool {
    build.join("default.yaml").is_file()
        && all_schema_ids().all(|id| build.join(format!("{}.schema.yaml", id.to_str().unwrap())).is_file())
}

/// Precompiles schemas and dictionaries into `<shared>\build` (packaging step, not at runtime).
///
/// Any existing `build\` is removed first (full rebuild). Uses a throwaway user directory, so
/// nothing outside `<shared>` is touched. One-shot: run it in its own process (e.g. the app's
/// `--deploy` mode); afterwards neither `deploy` nor [`RimeEngine::start`] works in that process.
pub fn deploy(opts: &Options) -> Result<DeployReport, Error> {
    let t0 = Instant::now();
    check_deployer_allowed()?;
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
    let ok = run_deployer(api, &traits, |api| unsafe { (api.deploy.unwrap())() != 0 })?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !ok {
        return Err(Error::Rime("deploy() 失败（见日志）".into()));
    }
    if !precompiled(&build) {
        return Err(Error::Rime(format!("部署后 {} 不完整（缺 default.yaml 或方案）", build.display())));
    }
    Ok(DeployReport { millis: t0.elapsed().as_millis() as u64, build_bytes: api::dir_size(&build) })
}

const ONE_SHOT: &str = "部署要在单独的进程里运行（每个进程一次，且不能启动过 RimeEngine）";

pub(crate) fn check_deployer_allowed() -> Result<(), Error> {
    let state = api::state();
    if state.initialized || state.deployed || state.engines > 0 {
        return Err(Error::Rime(ONE_SHOT.into()));
    }
    Ok(())
}

/// `deployer_initialize` with `traits`, `work`, `finalize`. Marks the process as deployed.
pub(crate) fn run_deployer<R>(api: api::Api, traits: &Traits, work: impl FnOnce(api::Api) -> R) -> Result<R, Error> {
    let mut state = api::state();
    if state.initialized || state.deployed || state.engines > 0 {
        return Err(Error::Rime(ONE_SHOT.into()));
    }
    api::setup_once(api, &mut state, traits);
    let mut raw = traits.raw();
    unsafe { (api.deployer_initialize.unwrap())(&mut raw) };
    let r = work(api);
    unsafe { (api.finalize.unwrap())() };
    state.deployed = true;
    Ok(r)
}

/// Shuts librime down (flushes and closes user dictionaries). Optional, for a clean exit:
/// call after every [`RimeEngine`] is dropped. A later [`RimeEngine::start`] initializes again.
pub fn shutdown() {
    let mut state = api::state();
    if state.initialized {
        if let Ok(api) = api::loaded() {
            unsafe { (api.finalize.unwrap())() };
        }
        state.initialized = false;
    }
}

/// One librime session. One per process, used from one thread at a time.
pub struct RimeEngine {
    schema: Schema,
    shuangpin: ShuangpinScheme,
    session: Session,
    opts: Options,
}

impl RimeEngine {
    /// Loads librime (once per process), initializes it with the precompiled `build\` data (and
    /// the user build, if it is up to date) and opens a session on `schema`.
    pub fn start(opts: &Options, schema: Schema) -> Result<RimeEngine, Error> {
        let api = api::load(&opts.dll).map_err(Error::Load)?;
        {
            let mut state = api::state();
            if state.deployed {
                return Err(Error::Rime("这个进程已经运行过部署，不能再启动引擎".into()));
            }
            if !state.initialized {
                initialize(api, &mut state, opts)?;
            }
        }
        let session = Session::create(api).ok_or_else(|| Error::Rime("create_session 失败".into()))?;
        let mut engine = RimeEngine { schema, shuangpin: opts.shuangpin, session, opts: opts.clone() };
        api::state().engines += 1;
        engine.select_current()?;
        Ok(engine)
    }

    /// Selects `self.schema` (with `self.shuangpin`) on the session and warms it up.
    fn select_current(&mut self) -> Result<(), Error> {
        let id = rime_schema(self.schema, self.shuangpin);
        if !self.session.select_schema(id) {
            return Err(Error::Rime(format!("无法选择方案 {id:?}")));
        }
        self.warm_up();
        Ok(())
    }

    /// Restarts librime in place: closes the session, `finalize`, `initialize` with `opts`
    /// (switching to a newly built user build, or back to the shared one), and reopens the
    /// session on the current schema. Use after `dianmo.exe --deploy-user` finished or after
    /// [`crate::set_fuzzy`] returned [`crate::FuzzyChange::Reload`]. Takes about as long as
    /// [`RimeEngine::start`] (≈ 150–300 ms); the composition is cleared.
    ///
    /// On error the engine is left without a session (inputs do nothing); drop it and fall back.
    pub fn reload(&mut self, opts: &Options) -> Result<(), Error> {
        let api = api::loaded().map_err(Error::Load)?;
        self.session.close();
        {
            let mut state = api::state();
            if state.initialized {
                unsafe { (api.finalize.unwrap())() };
                state.initialized = false;
            }
            initialize(api, &mut state, opts)?;
        }
        self.opts = opts.clone();
        self.shuangpin = opts.shuangpin;
        if !self.session.reopen() {
            return Err(Error::Rime("create_session 失败".into()));
        }
        self.select_current()
    }

    /// Whether librime reads the user build (fuzzy pinyin) right now.
    pub fn uses_user_build(&self) -> bool {
        api::state().user_build
    }

    /// The double pinyin scheme behind `Schema::Shuangpin`.
    pub fn shuangpin(&self) -> ShuangpinScheme {
        self.shuangpin
    }

    /// Switches the double pinyin scheme (re-selects the schema if double pinyin is active;
    /// the composition is cleared). 60–170 ms like any schema switch.
    pub fn set_shuangpin(&mut self, scheme: ShuangpinScheme) {
        if scheme == self.shuangpin {
            return;
        }
        self.shuangpin = scheme;
        self.opts.shuangpin = scheme;
        if self.schema == Schema::Shuangpin {
            self.session.clear();
            let _ = self.session.take_commit();
            let _ = self.select_current();
        }
    }

    /// Runs `f` with the session closed (librime then has the user dictionary closed), then
    /// reopens the session on the current schema.
    fn with_session_closed<R>(&mut self, f: impl FnOnce(api::Levers) -> Result<R, Error>) -> Result<R, Error> {
        let api = api::loaded().map_err(Error::Load)?;
        let levers = api::levers(api).map_err(Error::Rime)?;
        self.session.close();
        let r = f(levers);
        if !self.session.reopen() {
            return Err(Error::Rime("create_session 失败".into()));
        }
        self.select_current()?;
        r
    }

    /// Exports the learned words (`rime_ice` user dictionary) as text (`词\t拼音\t次数`, UTF-8,
    /// librime's own format) to `path`. Returns the number of entries.
    pub fn export_user_dict(&mut self, path: &Path) -> Result<usize, Error> {
        let user = self.opts.user_data_dir.clone();
        self.with_session_closed(|levers| user::export_with(levers, &user, path))
    }

    /// Merges a text file (the [`RimeEngine::export_user_dict`] format) into the user
    /// dictionary. Returns the number of entries read.
    pub fn import_user_dict(&mut self, path: &Path) -> Result<usize, Error> {
        self.with_session_closed(|levers| user::import_with(levers, path))
    }

    /// Number of entries in the user dictionary (`None`: couldn't be read). Costs an export to
    /// a temp file plus a session restart (tens of ms to ~200 ms).
    pub fn user_word_count(&mut self) -> Option<usize> {
        let user = self.opts.user_data_dir.clone();
        self.with_session_closed(|levers| user::count_with(levers, &user)).ok()
    }

    /// Deletes the user dictionary (all learned words). librime is restarted around it so no
    /// file is open; takes about as long as [`RimeEngine::reload`].
    pub fn clear_user_dict(&mut self) -> Result<(), Error> {
        let api = api::loaded().map_err(Error::Load)?;
        self.session.close();
        let r = {
            let mut state = api::state();
            if state.initialized {
                unsafe { (api.finalize.unwrap())() };
                state.initialized = false;
            }
            let r = user::remove_userdb(&self.opts.user_data_dir);
            initialize(api, &mut state, &self.opts)?;
            r
        };
        if !self.session.reopen() {
            return Err(Error::Rime("create_session 失败".into()));
        }
        self.select_current()?;
        r
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

impl Drop for RimeEngine {
    fn drop(&mut self) {
        self.session.close();
        let mut state = api::state();
        state.engines = state.engines.saturating_sub(1);
    }
}

/// `initialize` against `<shared>\build` (plus `<user>\build` when it is up to date). Without a
/// complete `build\` (developer setups) it falls back to a maintenance run that compiles into
/// `<user>\build.dev` (slow, once).
pub(crate) fn initialize(api: api::Api, state: &mut api::State, opts: &Options) -> Result<(), Error> {
    let shared = &opts.shared_data_dir;
    let user = &opts.user_data_dir;
    std::fs::create_dir_all(user).map_err(|e| io_err("创建", user, e))?;
    if let Some(d) = &opts.log_dir {
        std::fs::create_dir_all(d).map_err(|e| io_err("创建", d, e))?;
    }
    let build = shared.join("build");
    let ready = precompiled(&build);
    let user_build = if ready { user::prepare(opts) } else { None };
    let staging: PathBuf = match (&user_build, ready) {
        (Some(dir), _) => dir.clone(),
        (None, true) => build.clone(),
        (None, false) => user.join(user::BUILD_DEV),
    };
    let traits = Traits::new(&TraitsSpec {
        shared,
        user,
        staging: &staging,
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
    state.user_build = user_build.is_some();
    Ok(())
}

impl Engine for RimeEngine {
    fn schema(&self) -> Schema {
        self.schema
    }

    fn set_schema(&mut self, schema: Schema) {
        self.session.clear();
        let _ = self.session.take_commit();
        if schema != self.schema && self.session.select_schema(rime_schema(schema, self.shuangpin)) {
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
