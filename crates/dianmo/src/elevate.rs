//! Running elevated through a scheduled task (TODO #23).
//!
//! A normal-integrity process neither receives UI Automation focus events from elevated windows
//! (e.g. an administrator PowerShell) nor may it `SendInput` into them (UIPI). So 点墨 is installed
//! with a scheduled task (`Dianmo`, current user, interactive, "run with highest privileges") whose
//! action is `dianmo.exe --task`. Starting that task gives an elevated 点墨 without a UAC prompt.
//! `dianmo.exe` started normally (shortcut, double click) hands over to the task when it is not
//! elevated itself (see `main.rs`); autostart is the task's logon trigger.
//!
//! Registering or changing the task needs an elevated caller; reading and running it does not
//! (the task's user gets read/execute access).

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, HANDLE, RPC_E_CHANGED_MODE, VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, IRegisteredTask, ITaskDefinition, ITaskFolder, ITaskService, TASK_ACTION_EXEC,
    TASK_COMPATIBILITY_V2_1, TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_RUNLEVEL_HIGHEST, TASK_TRIGGER_LOGON, TASK_TRIGGER_TYPE2, TASK_UPDATE, TaskScheduler,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VT_I4, VariantClear};
use windows::core::{BSTR, HSTRING, Interface, Result};

/// The argument the task passes: "started by the task" (start hidden, never hand over again).
pub const TASK_ARG: &str = "--task";

/// True if this process runs with an elevated (full administrator) token.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// What we need to know about a registered task.
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// The first exec action's program, environment variables expanded.
    pub exe: PathBuf,
    /// Has an enabled logon trigger (= autostart).
    pub logon_trigger: bool,
}

impl TaskInfo {
    /// True if the task starts `exe` (case-insensitive; both canonicalized when possible).
    pub fn runs(&self, exe: &Path) -> bool {
        let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned()).to_string_lossy().to_lowercase();
        norm(&self.exe) == norm(exe)
    }
}

/// The task `name` in the root folder, `Ok(None)` if there is none.
pub fn query(name: &str) -> Result<Option<TaskInfo>> {
    let _com = Com::init();
    let Some(task) = get_task(&root_folder()?, name)? else { return Ok(None) };
    let def = unsafe { task.Definition()? };
    let exe = exec_path(&def)?.map(|p| PathBuf::from(expand_env(&p))).unwrap_or_default();
    Ok(Some(TaskInfo { exe, logon_trigger: logon_trigger_count(&def, true)? > 0 }))
}

/// Starts the task now (it starts nothing if an instance is already running).
pub fn run(name: &str) -> Result<()> {
    let _com = Com::init();
    let task = get_task(&root_folder()?, name)?.ok_or_else(not_found)?;
    unsafe { task.Run(&VARIANT::default())? };
    Ok(())
}

/// Creates or replaces the task `name` for the current user: run `exe args` elevated, no time
/// limit, on battery too, normal priority, on demand, one instance. Keeps an existing logon
/// trigger unless `autostart` says otherwise. Needs an elevated caller.
pub fn register(name: &str, exe: &Path, args: &str, autostart: Option<bool>) -> Result<()> {
    let _com = Com::init();
    let service = service()?;
    let folder = unsafe { service.GetFolder(&BSTR::from("\\"))? };
    let had_trigger = match get_task(&folder, name)? {
        Some(t) => logon_trigger_count(&unsafe { t.Definition()? }, true)? > 0,
        None => false,
    };
    let user = current_user();
    unsafe {
        let def = service.NewTask(0)?;
        let info = def.RegistrationInfo()?;
        info.SetDescription(&BSTR::from("点墨 Dianmo 触屏输入法：以最高权限运行，才能给管理员窗口输入"))?;
        info.SetAuthor(&BSTR::from(user.as_str()))?;

        let principal = def.Principal()?;
        principal.SetUserId(&BSTR::from(user.as_str()))?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
        principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST)?;

        let settings = def.Settings()?;
        settings.SetCompatibility(TASK_COMPATIBILITY_V2_1)?;
        settings.SetAllowDemandStart(VARIANT_TRUE)?;
        settings.SetDisallowStartIfOnBatteries(VARIANT_FALSE)?;
        settings.SetStopIfGoingOnBatteries(VARIANT_FALSE)?;
        settings.SetExecutionTimeLimit(&BSTR::from("PT0S"))?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)?;
        // Task processes default to priority 7 (below normal CPU, low I/O and memory priority).
        settings.SetPriority(4)?;
        settings.SetStartWhenAvailable(VARIANT_FALSE)?;
        settings.SetEnabled(VARIANT_TRUE)?;

        let action: IExecAction = def.Actions()?.Create(TASK_ACTION_EXEC)?.cast()?;
        action.SetPath(&BSTR::from(exe.to_string_lossy().as_ref()))?;
        action.SetArguments(&BSTR::from(args))?;
        if let Some(dir) = exe.parent() {
            action.SetWorkingDirectory(&BSTR::from(dir.to_string_lossy().as_ref()))?;
        }

        if autostart.unwrap_or(had_trigger) {
            add_logon_trigger(&def, &user)?;
        }
        save(&folder, name, &def, &user, TASK_CREATE_OR_UPDATE.0)
    }
}

/// Deletes the task; Ok if there was none.
pub fn unregister(name: &str) -> Result<()> {
    let _com = Com::init();
    let folder = root_folder()?;
    match unsafe { folder.DeleteTask(&BSTR::from(name), 0) } {
        Err(e) if is_not_found(&e) => Ok(()),
        r => r,
    }
}

/// Adds or removes the task's logon trigger (autostart). Needs an elevated caller.
pub fn set_logon_trigger(name: &str, on: bool) -> Result<()> {
    let _com = Com::init();
    let folder = root_folder()?;
    let task = get_task(&folder, name)?.ok_or_else(not_found)?;
    unsafe {
        let def = task.Definition()?;
        let has = logon_trigger_count(&def, true)? > 0;
        if has == on && logon_trigger_count(&def, false)? == logon_trigger_count(&def, true)? {
            return Ok(());
        }
        // Remove every logon trigger (also disabled ones), then add one back if wanted.
        let triggers = def.Triggers()?;
        let mut count = 0;
        triggers.Count(&mut count)?;
        for i in (1..=count).rev() {
            if trigger_type(&def, i)? == TASK_TRIGGER_LOGON {
                let mut index = i4_variant(i);
                let r = triggers.Remove(&index);
                let _ = VariantClear(&mut index);
                r?;
            }
        }
        let user = principal_user(&def).unwrap_or_else(current_user);
        if on {
            add_logon_trigger(&def, &user)?;
        }
        save(&folder, name, &def, &user, TASK_UPDATE.0)
    }
}

// ---------------------------------------------------------------------------------------------

/// Initializes COM on this thread for the duration of a call (no-op if the thread already has it).
struct Com(bool);

impl Com {
    fn init() -> Com {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // S_OK / S_FALSE must be balanced; RPC_E_CHANGED_MODE (thread is MTA) must not.
        Com(hr.is_ok() && hr != RPC_E_CHANGED_MODE)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn service() -> Result<ITaskService> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        Ok(service)
    }
}

fn root_folder() -> Result<ITaskFolder> {
    unsafe { service()?.GetFolder(&BSTR::from("\\")) }
}

fn get_task(folder: &ITaskFolder, name: &str) -> Result<Option<IRegisteredTask>> {
    match unsafe { folder.GetTask(&BSTR::from(name)) } {
        Ok(t) => Ok(Some(t)),
        Err(e) if is_not_found(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

fn is_not_found(e: &windows::core::Error) -> bool {
    // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND)
    matches!(e.code().0 as u32, 0x8007_0002 | 0x8007_0003)
}

fn not_found() -> windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(0x8007_0002_u32 as i32), "task not found")
}

fn save(folder: &ITaskFolder, name: &str, def: &ITaskDefinition, user: &str, flags: i32) -> Result<()> {
    unsafe {
        let mut user = bstr_variant(user);
        let empty = VARIANT::default();
        let r = folder.RegisterTaskDefinition(&BSTR::from(name), def, flags, &user, &empty, TASK_LOGON_INTERACTIVE_TOKEN, &empty);
        let _ = VariantClear(&mut user);
        r.map(|_| ())
    }
}

fn exec_path(def: &ITaskDefinition) -> Result<Option<String>> {
    unsafe {
        let actions = def.Actions()?;
        let mut count = 0;
        actions.Count(&mut count)?;
        for i in 1..=count {
            if let Ok(exec) = actions.get_Item(i)?.cast::<IExecAction>() {
                let mut path = BSTR::new();
                exec.Path(&mut path)?;
                return Ok(Some(path.to_string().trim_matches('"').to_owned()));
            }
        }
        Ok(None)
    }
}

fn trigger_type(def: &ITaskDefinition, index: i32) -> Result<TASK_TRIGGER_TYPE2> {
    unsafe {
        let mut ty = TASK_TRIGGER_TYPE2::default();
        def.Triggers()?.get_Item(index)?.Type(&mut ty)?;
        Ok(ty)
    }
}

/// Number of logon triggers (only enabled ones if `enabled_only`).
fn logon_trigger_count(def: &ITaskDefinition, enabled_only: bool) -> Result<i32> {
    unsafe {
        let triggers = def.Triggers()?;
        let mut count = 0;
        triggers.Count(&mut count)?;
        let mut n = 0;
        for i in 1..=count {
            let t = triggers.get_Item(i)?;
            let mut ty = TASK_TRIGGER_TYPE2::default();
            t.Type(&mut ty)?;
            if ty != TASK_TRIGGER_LOGON {
                continue;
            }
            let mut enabled = VARIANT_BOOL::default();
            t.Enabled(&mut enabled)?;
            if !enabled_only || enabled.as_bool() {
                n += 1;
            }
        }
        Ok(n)
    }
}

fn add_logon_trigger(def: &ITaskDefinition, user: &str) -> Result<()> {
    unsafe {
        let trigger: ILogonTrigger = def.Triggers()?.Create(TASK_TRIGGER_LOGON)?.cast()?;
        trigger.SetUserId(&BSTR::from(user))?;
        trigger.SetEnabled(VARIANT_TRUE)?;
        Ok(())
    }
}

fn principal_user(def: &ITaskDefinition) -> Option<String> {
    unsafe {
        let mut user = BSTR::new();
        def.Principal().ok()?.UserId(&mut user).ok()?;
        let user = user.to_string();
        (!user.is_empty()).then_some(user)
    }
}

/// `DOMAIN\user` of this process.
fn current_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

fn expand_env(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let src = HSTRING::from(s);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { ExpandEnvironmentStringsW(&src, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_owned();
    }
    String::from_utf16_lossy(&buf[..n - 1])
}

fn bstr_variant(s: &str) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_BSTR;
        inner.Anonymous.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(s));
    }
    v
}

fn i4_variant(i: i32) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = i;
    }
    v
}
