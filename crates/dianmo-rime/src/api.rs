//! Safe-ish wrapper over the `RimeApi` function table: loading `rime.dll`, the process-wide
//! setup/initialize state, and session calls that copy everything librime returns into owned
//! Rust values (freeing librime's structs with its own `free_*`).

use std::ffi::{CStr, CString, c_char, c_int};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Storage::FileSystem::GetShortPathNameW;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW};
use windows::core::{PCSTR, PCWSTR};

use crate::ffi::{self, RimeApi, RimeSessionId, rime_struct};

pub(crate) type Api = &'static RimeApi;

/// `rime.dll` stays loaded for the life of the process (librime keeps global state).
static API: OnceLock<Result<Api, String>> = OnceLock::new();

/// Process-wide librime state. glog must be initialised exactly once (`setup`), and
/// `initialize` starts the service that sessions live in.
pub(crate) struct State {
    pub setup: bool,
    pub initialized: bool,
    /// `shutdown()` ran; librime can't be brought back in this process.
    pub finalized: bool,
}

pub(crate) static STATE: Mutex<State> = Mutex::new(State { setup: false, initialized: false, finalized: false });

/// Loads `dll` (first call wins; later calls reuse the loaded library) and returns the API table.
pub(crate) fn load(dll: &Path) -> Result<Api, String> {
    API.get_or_init(|| unsafe { load_inner(dll) }).clone()
}

/// The already loaded API, if any.
pub(crate) fn loaded() -> Result<Api, String> {
    API.get().cloned().unwrap_or_else(|| Err("rime.dll 未加载".into()))
}

unsafe fn load_inner(dll: &Path) -> Result<Api, String> {
    if !dll.is_file() {
        return Err(format!("{} 不存在", dll.display()));
    }
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    // ALTERED_SEARCH_PATH: dependencies resolve next to rime.dll, not next to the exe.
    let module: HMODULE = unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map_err(|e| format!("{}: {e}", dll.display()))?;
    let Some(proc) = (unsafe { GetProcAddress(module, PCSTR(c"rime_get_api".as_ptr().cast())) }) else {
        return Err("rime.dll 没有导出 rime_get_api".into());
    };
    let get_api: unsafe extern "C" fn() -> *mut RimeApi = unsafe { std::mem::transmute(proc) };
    let api = unsafe { get_api() };
    if api.is_null() {
        return Err("rime_get_api 返回空指针".into());
    }
    let api: Api = unsafe { &*api };
    let want = (std::mem::size_of::<RimeApi>() - std::mem::size_of::<c_int>()) as c_int;
    if api.data_size < want {
        return Err(format!("RimeApi 太旧（data_size {} < {want}），需要 librime 1.17", api.data_size));
    }
    // Every slot we call must be present.
    let required = [
        api.setup.is_some(),
        api.initialize.is_some(),
        api.finalize.is_some(),
        api.start_maintenance.is_some(),
        api.join_maintenance_thread.is_some(),
        api.deployer_initialize.is_some(),
        api.deploy.is_some(),
        api.create_session.is_some(),
        api.destroy_session.is_some(),
        api.process_key.is_some(),
        api.clear_composition.is_some(),
        api.get_commit.is_some(),
        api.free_commit.is_some(),
        api.get_context.is_some(),
        api.free_context.is_some(),
        api.select_schema.is_some(),
        api.get_input.is_some(),
        api.select_candidate.is_some(),
        api.candidate_list_from_index.is_some(),
        api.candidate_list_next.is_some(),
        api.candidate_list_end.is_some(),
        api.set_input.is_some(),
        api.get_version.is_some(),
        api.highlight_candidate.is_some(),
    ];
    if required.contains(&false) {
        return Err("RimeApi 缺少函数".into());
    }
    Ok(api)
}

pub(crate) fn version(api: Api) -> String {
    unsafe { opt_str((api.get_version.unwrap())()) }
}

/// A path librime can open. librime 1.17 decodes `char*` paths as UTF-8 on Windows (verified
/// with a non-ASCII user directory on the Surface); non-ASCII paths still prefer their 8.3 short
/// form when the volume has one, which works with any decoding.
pub(crate) fn path_arg(p: &Path) -> Result<CString, String> {
    let s = p.to_string_lossy();
    let s = if s.is_ascii() { s.into_owned() } else { short_path(p).unwrap_or_else(|| s.into_owned()) };
    CString::new(s).map_err(|_| format!("路径含 NUL：{}", p.display()))
}

fn short_path(p: &Path) -> Option<String> {
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let s = String::from_utf16(&buf[..n]).ok()?;
    s.is_ascii().then_some(s)
}

/// Owned strings backing a `RimeTraits` (librime copies them during the call).
pub(crate) struct Traits {
    shared: CString,
    user: CString,
    log_dir: CString,
    staging: CString,
    prebuilt: CString,
    min_log_level: c_int,
}

pub(crate) struct TraitsSpec<'a> {
    pub shared: &'a Path,
    pub user: &'a Path,
    pub staging: &'a Path,
    pub prebuilt: &'a Path,
    pub log_dir: Option<&'a Path>,
    pub min_log_level: i32,
}

impl Traits {
    pub fn new(spec: &TraitsSpec) -> Result<Traits, String> {
        Ok(Traits {
            shared: path_arg(spec.shared)?,
            user: path_arg(spec.user)?,
            staging: path_arg(spec.staging)?,
            prebuilt: path_arg(spec.prebuilt)?,
            // "" = stderr only (no log files).
            log_dir: match spec.log_dir {
                Some(d) => path_arg(d)?,
                None => CString::default(),
            },
            min_log_level: spec.min_log_level.clamp(0, 3),
        })
    }

    /// The C struct; valid while `self` lives.
    pub fn raw(&self) -> ffi::RimeTraits {
        let mut t: ffi::RimeTraits = unsafe { rime_struct() };
        t.shared_data_dir = self.shared.as_ptr();
        t.user_data_dir = self.user.as_ptr();
        t.distribution_name = c"点墨".as_ptr();
        t.distribution_code_name = c"dianmo".as_ptr();
        t.distribution_version = DIST_VERSION.as_ptr();
        t.app_name = c"rime.dianmo".as_ptr();
        t.modules = std::ptr::null();
        t.min_log_level = self.min_log_level;
        t.log_dir = self.log_dir.as_ptr();
        t.prebuilt_data_dir = self.prebuilt.as_ptr();
        t.staging_dir = self.staging.as_ptr();
        t
    }
}

const DIST_VERSION: &CStr = match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
    Ok(s) => s,
    Err(_) => c"0",
};

/// `setup` once per process (glog can't be initialised twice).
pub(crate) fn setup_once(api: Api, state: &mut State, traits: &Traits) {
    if !state.setup {
        let mut raw = traits.raw();
        unsafe { (api.setup.unwrap())(&mut raw) };
        state.setup = true;
    }
}

unsafe fn opt_str(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

/// What the candidate bar needs from `RimeContext`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Context {
    pub preedit: String,
    /// Byte offsets into `preedit` of the active segment.
    pub sel_start: usize,
    pub sel_end: usize,
    /// Absolute index of librime's highlighted candidate.
    pub highlighted: usize,
}

/// One session. Not `Clone`; destroyed on drop.
pub(crate) struct Session {
    api: Api,
    id: RimeSessionId,
}

impl Session {
    pub fn create(api: Api) -> Option<Session> {
        let id = unsafe { (api.create_session.unwrap())() };
        (id != 0).then_some(Session { api, id })
    }

    pub fn version(&self) -> String {
        version(self.api)
    }

    pub fn select_schema(&self, id: &CStr) -> bool {
        unsafe { (self.api.select_schema.unwrap())(self.id, id.as_ptr()) != 0 }
    }

    pub fn process_key(&self, keycode: i32) -> bool {
        unsafe { (self.api.process_key.unwrap())(self.id, keycode, 0) != 0 }
    }

    pub fn select_candidate(&self, index: usize) -> bool {
        unsafe { (self.api.select_candidate.unwrap())(self.id, index) != 0 }
    }

    /// Moves librime's highlight (what its preedit describes) without selecting.
    pub fn highlight(&self, index: usize) -> bool {
        unsafe { (self.api.highlight_candidate.unwrap())(self.id, index) != 0 }
    }

    pub fn clear(&self) {
        unsafe { (self.api.clear_composition.unwrap())(self.id) }
    }

    pub fn set_input(&self, input: &str) -> bool {
        let Ok(c) = CString::new(input) else { return false };
        unsafe { (self.api.set_input.unwrap())(self.id, c.as_ptr()) != 0 }
    }

    /// The raw input (e.g. `"64426"`, `"ni'426"`).
    pub fn input(&self) -> String {
        unsafe { opt_str((self.api.get_input.unwrap())(self.id)) }
    }

    /// Pending commit text, consumed.
    pub fn take_commit(&self) -> Option<String> {
        unsafe {
            let mut c: ffi::RimeCommit = rime_struct();
            if (self.api.get_commit.unwrap())(self.id, &mut c) == 0 {
                return None;
            }
            let text = opt_str(c.text);
            (self.api.free_commit.unwrap())(&mut c);
            Some(text)
        }
    }

    pub fn context(&self) -> Context {
        unsafe {
            let mut c: ffi::RimeContext = rime_struct();
            if (self.api.get_context.unwrap())(self.id, &mut c) == 0 {
                return Context::default();
            }
            let preedit = opt_str(c.composition.preedit);
            let clamp = |v: c_int| (v.max(0) as usize).min(preedit.len());
            let ctx = Context {
                sel_start: clamp(c.composition.sel_start),
                sel_end: clamp(c.composition.sel_end),
                highlighted: (c.menu.page_no.max(0) * c.menu.page_size.max(0) + c.menu.highlighted_candidate_index.max(0))
                    as usize,
                preedit,
            };
            (self.api.free_context.unwrap())(&mut c);
            ctx
        }
    }

    /// Candidates `start..start + count` of the active segment as `(text, raw comment)`.
    pub fn candidates(&self, start: usize, count: usize) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(count.min(64));
        if count == 0 {
            return out;
        }
        unsafe {
            let mut it: ffi::RimeCandidateListIterator = std::mem::zeroed();
            if (self.api.candidate_list_from_index.unwrap())(self.id, &mut it, start as c_int) == 0 {
                return out;
            }
            while out.len() < count && (self.api.candidate_list_next.unwrap())(&mut it) != 0 {
                out.push((opt_str(it.candidate.text), opt_str(it.candidate.comment)));
            }
            (self.api.candidate_list_end.unwrap())(&mut it);
        }
        out
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.api.destroy_session.unwrap())(self.id) };
    }
}

pub(crate) fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

pub(crate) fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}
