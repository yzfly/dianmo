//! The settings that need librime (TODO #38): 双拼方案, 模糊音 and the user dictionary
//! (export / import / clear / word count).
//!
//! Anything slow runs off the UI thread:
//! - 模糊音: `dianmo_rime::set_fuzzy` writes the user customization (fast). If it needs a new
//!   user build, `dianmo.exe --deploy-user` runs as a child process (below-normal priority,
//!   about a second); while it runs the settings page shows 「正在应用模糊音…」. Chips tapped meanwhile
//!   are saved and applied by one more deploy afterwards. Then the engine is reloaded.
//! - Engine jobs (reload, user dictionary): the `RimeEngine` is taken out of the controller
//!   (the stand-in engine types letters meanwhile; only when nothing is being composed), moved
//!   to a short-lived thread, and handed back through `pending_rime` (`swap_in_rime`). One job
//!   at a time; others queue.
//! - The learned-word count (输入 page) is cached in `user_words`: counted once librime is up,
//!   again whenever the settings window opens, and as part of 导出 / 导入 / 清空. A plain count
//!   (`RimeJob::Count`) waits until the keyboard is hidden or has had no input for
//!   [`COUNT_IDLE_MS`], so it never takes the engine away while someone types; the settings page
//!   shows the cached number meanwhile.
//! - The file dialogs run on their own threads, owned by the settings window.

// Without librime most of this compiles to stubs.
#![cfg_attr(not(feature = "rime"), allow(unused_imports, unused_variables))]

use std::any::Any;
use std::path::PathBuf;

use dianmo_ui::settings::{Level, ShuangpinScheme, Status};
use dianmo_ui::{Response, View};
use dianmo_win::HostControl;

use super::DianmoApp;
use crate::log;

/// A word count runs once the keyboard has had no input for this long (or is hidden).
#[cfg(feature = "rime")]
const COUNT_IDLE_MS: u64 = 3000;

/// Look again whether a waiting word count can run now.
#[cfg(feature = "rime")]
struct RimeIdleCheck;

/// Which way the user dictionary file goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DictFile {
    Export,
    Import,
}

/// The file dialog for the user dictionary closed (`None` = cancelled).
struct DictPicked(DictFile, Option<PathBuf>);

const DICT_FILTERS: [(&str, &str); 2] = [("文本文件 (*.txt)", "*.txt"), ("所有文件 (*.*)", "*.*")];
const NO_ENGINE: &str = "词库没有加载，暂时不能操作用户词库";

#[cfg(feature = "rime")]
pub(super) fn rime_scheme(s: ShuangpinScheme) -> dianmo_rime::ShuangpinScheme {
    match s {
        ShuangpinScheme::Xiaohe => dianmo_rime::ShuangpinScheme::Flypy,
        ShuangpinScheme::Ziranma => dianmo_rime::ShuangpinScheme::Ziranma,
        ShuangpinScheme::Microsoft => dianmo_rime::ShuangpinScheme::Mspy,
        ShuangpinScheme::Sogou => dianmo_rime::ShuangpinScheme::Sogou,
    }
}

/// Work on the `RimeEngine` away from the UI thread.
#[cfg(feature = "rime")]
#[derive(Clone, Debug, PartialEq)]
pub(super) enum RimeJob {
    /// Re-initialize librime (a new 模糊音 user build).
    Reload,
    Export(PathBuf),
    Import(PathBuf),
    Clear,
    Count,
}

#[cfg(feature = "rime")]
struct RimeJobDone {
    engine: dianmo_rime::RimeEngine,
    job: RimeJob,
    /// Words exported / imported / counted, or why it failed.
    result: Result<usize, String>,
    /// The user dictionary's word count afterwards, if known.
    words: Option<usize>,
}

/// `dianmo.exe --deploy-user` finished.
#[cfg(feature = "rime")]
struct FuzzyDeployed(Result<(), String>);

impl DianmoApp {
    /// 双拼方案 → the running engine (no-op while librime is starting or busy: the scheme is
    /// applied when it comes back, see `swap_in_rime`).
    pub(super) fn set_engine_shuangpin(&mut self, scheme: ShuangpinScheme, _view: &mut dyn View) -> Response {
        #[cfg(feature = "rime")]
        {
            if let Some(o) = &mut self.rime_options {
                o.shuangpin = rime_scheme(scheme);
            }
            if matches!(self.ctl.engine_mut(), crate::engine::AnyEngine::Rime(_)) {
                // Switching drops the composition: drop ours too, so both agree.
                if self.ctl.is_composing() {
                    self.ctl.handle(dianmo_core::Action::ClearComposition);
                }
                if let crate::engine::AnyEngine::Rime(e) = self.ctl.engine_mut() {
                    e.set_shuangpin(rime_scheme(scheme));
                }
                log!("shuangpin scheme -> {}", rime_scheme(scheme).schema_id());
                return _view.set_input_state(self.input_state());
            }
        }
        let _ = scheme;
        Response::none()
    }

    /// 模糊音 changed (or at start, or after 恢复默认): write the customization and deploy /
    /// reload as needed.
    pub(super) fn apply_fuzzy(&mut self, host: &mut HostControl) {
        #[cfg(feature = "rime")]
        {
            let Some(opts) = self.rime_options.clone() else { return };
            if !matches!(self.status, super::EngineStatus::Rime) {
                // librime isn't running (yet): `on_rime_ready` calls us again.
                return;
            }
            if self.fuzzy_deploying {
                self.fuzzy_again = true;
                return;
            }
            match dianmo_rime::set_fuzzy(&opts, self.settings.fuzzy) {
                Ok(dianmo_rime::FuzzyChange::Unchanged) => {}
                Ok(dianmo_rime::FuzzyChange::Reload) => {
                    self.fuzzy_status = Status::new(Level::Unknown, "正在应用模糊音…");
                    self.rime_job(RimeJob::Reload, host);
                }
                Ok(dianmo_rime::FuzzyChange::Deploy) => self.deploy_fuzzy(host),
                Err(e) => {
                    log!("fuzzy: writing the customization failed: {e}");
                    self.fuzzy_status = Status::error(format!("没有应用成功：{e}"));
                }
            }
        }
        #[cfg(not(feature = "rime"))]
        let _ = host;
    }

    #[cfg(feature = "rime")]
    fn deploy_fuzzy(&mut self, host: &mut HostControl) {
        self.fuzzy_deploying = true;
        self.fuzzy_again = false;
        self.fuzzy_status = Status::new(Level::Unknown, "正在应用模糊音…");
        let proxy = host.proxy();
        let exe = std::env::current_exe().unwrap_or_default();
        let instance = crate::platform::instance_name();
        log!("fuzzy: deploying the user build");
        let _ = std::thread::Builder::new().name("dianmo-deploy-user".into()).spawn(move || {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let t = std::time::Instant::now();
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--deploy-user").creation_flags(CREATE_NO_WINDOW);
            if !instance.is_empty() {
                cmd.args(["--instance", instance]);
            }
            let result = match cmd.status() {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(format!("部署失败（{}），详情见日志", s.code().unwrap_or(-1))),
                Err(e) => Err(format!("无法启动部署：{e}")),
            };
            log!("fuzzy: deploy finished in {} ms: {result:?}", t.elapsed().as_millis());
            proxy.post(FuzzyDeployed(result));
        });
    }

    /// Queues a job on the engine; starts it now if possible.
    #[cfg(feature = "rime")]
    pub(super) fn rime_job(&mut self, job: RimeJob, host: &mut HostControl) {
        if !self.rime_jobs.contains(&job) {
            self.rime_jobs.push_back(job);
        }
        self.next_rime_job(host);
    }

    /// Starts the next queued job if the engine is free (here and not composing). A plain word
    /// count waits until the keyboard is hidden or idle; other jobs go first.
    #[cfg(feature = "rime")]
    pub(super) fn next_rime_job(&mut self, host: &mut HostControl) {
        use crate::engine::AnyEngine;
        if self.rime_busy || self.ctl.is_composing() {
            return;
        }
        let idle_for = dianmo_win::now_ms().saturating_sub(self.last_input_ms);
        let typing = host.is_visible() && idle_for < COUNT_IDLE_MS;
        let Some(next) = self.rime_jobs.iter().position(|j| !(typing && *j == RimeJob::Count)) else {
            // Only a count is waiting: look again when the pause is long enough.
            if !self.rime_jobs.is_empty() && !self.rime_idle_check {
                self.rime_idle_check = true;
                let proxy = host.proxy();
                let wait = COUNT_IDLE_MS - idle_for.min(COUNT_IDLE_MS) + 50;
                let _ = std::thread::Builder::new().name("dianmo-idle".into()).stack_size(64 * 1024).spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(wait));
                    proxy.post(RimeIdleCheck);
                });
            }
            return;
        };
        let Some(opts) = self.rime_options.clone() else {
            self.rime_jobs.clear();
            return;
        };
        let engine = match self.pending_rime.take() {
            Some(e) => e,
            None => {
                if !matches!(self.ctl.engine_mut(), AnyEngine::Rime(_)) {
                    return;
                }
                let stand_in = AnyEngine::Basic(crate::basic::BasicEngine::loading(self.ctl.schema()));
                match std::mem::replace(self.ctl.engine_mut(), stand_in) {
                    AnyEngine::Rime(e) => e,
                    other => {
                        *self.ctl.engine_mut() = other;
                        return;
                    }
                }
            }
        };
        let Some(job) = self.rime_jobs.remove(next) else {
            self.pending_rime = Some(engine);
            return;
        };
        self.rime_busy = true;
        let proxy = host.proxy();
        log!("rime job {job:?}");
        let spawned = std::thread::Builder::new().name("dianmo-rime-job".into()).spawn(move || {
            let mut engine = engine;
            let t = std::time::Instant::now();
            let (result, words) = match &job {
                RimeJob::Reload => (engine.reload(&opts).map(|()| 0).map_err(|e| e.to_string()), None),
                RimeJob::Export(p) => {
                    // An export writes every entry: its count is the word count.
                    let r = engine.export_user_dict(p).map_err(|e| e.to_string());
                    let words = r.as_ref().ok().copied();
                    (r, words)
                }
                RimeJob::Import(p) => {
                    // Recount while the engine is away anyway (imported entries may merge).
                    let r = engine.import_user_dict(p).map_err(|e| e.to_string());
                    let words = if r.is_ok() { engine.user_word_count() } else { None };
                    (r, words)
                }
                RimeJob::Clear => {
                    let r = engine.clear_user_dict().map(|()| 0).map_err(|e| e.to_string());
                    let words = r.is_ok().then_some(0);
                    (r, words)
                }
                RimeJob::Count => {
                    let words = engine.user_word_count();
                    (words.ok_or_else(|| "无法统计".to_owned()), words)
                }
            };
            log!("rime job {job:?} done in {} ms: {result:?}", t.elapsed().as_millis());
            proxy.post(RimeJobDone { engine, job, result, words });
        });
        if let Err(e) = spawned {
            // The engine moved into the closure that failed to spawn is gone; librime keeps
            // running, but this session is lost: fall back to the stand-in.
            log!("can't spawn the rime job thread: {e}");
            self.rime_busy = false;
            self.engine_failed("无法启动".into());
        }
    }

    /// 导出 / 导入: the file dialog (on its own thread, owned by the settings window).
    pub(super) fn pick_dict_file(&mut self, which: DictFile, host: &mut HostControl) {
        if !self.rime_running() {
            self.toast(host, NO_ENGINE);
            return;
        }
        let proxy = host.proxy();
        let owner = crate::shell::own_window(super::settings_ui::SETTINGS_TITLE);
        let start = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents"));
        let _ = std::thread::Builder::new().name("dianmo-pick".into()).spawn(move || {
            let path = match which {
                DictFile::Export => {
                    let name = format!("点墨用户词库-{}.txt", crate::log::timestamp(std::time::SystemTime::now())[..10].replace('-', ""));
                    crate::shell::save_file("导出用户词库", &name, "txt", &DICT_FILTERS, start.as_deref(), Some(owner))
                }
                DictFile::Import => crate::shell::open_file("导入用户词库", &DICT_FILTERS, start.as_deref(), Some(owner)),
            };
            proxy.post(DictPicked(which, path));
        });
    }

    /// 清空用户词库 (already confirmed in the settings window).
    pub(super) fn clear_user_dict(&mut self, host: &mut HostControl) {
        if !self.rime_running() {
            self.toast(host, NO_ENGINE);
        } else {
            #[cfg(feature = "rime")]
            {
                self.user_dict_status = Status::new(Level::Unknown, "正在清空…");
                self.rime_job(RimeJob::Clear, host);
            }
        }
    }

    /// Recounts the learned words (librime came up, or the settings window opened) once the
    /// keyboard is hidden or idle; `user_words` keeps the last count meanwhile.
    pub(super) fn refresh_user_words(&mut self, host: &mut HostControl) {
        #[cfg(feature = "rime")]
        if self.rime_running() {
            self.rime_job(RimeJob::Count, host);
        }
        #[cfg(not(feature = "rime"))]
        let _ = host;
    }

    fn rime_running(&self) -> bool {
        matches!(self.status, super::EngineStatus::Rime)
    }

    /// Events of this module; anything else is handed back.
    pub(super) fn rime_event(
        &mut self,
        event: Box<dyn Any + Send>,
        view: &mut dyn View,
        host: &mut HostControl,
    ) -> Result<Response, Box<dyn Any + Send>> {
        let event = match event.downcast::<DictPicked>() {
            Ok(p) => {
                let DictPicked(which, path) = *p;
                #[cfg(feature = "rime")]
                if let Some(path) = path {
                    log!("user dictionary {which:?}: {}", path.display());
                    let (busy, job) = match which {
                        DictFile::Export => ("正在导出…", RimeJob::Export(path)),
                        DictFile::Import => ("正在导入…", RimeJob::Import(path)),
                    };
                    self.user_dict_status = Status::new(Level::Unknown, busy);
                    self.rime_job(job, host);
                }
                #[cfg(not(feature = "rime"))]
                let _ = (which, path);
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        #[cfg(feature = "rime")]
        let event = match event.downcast::<RimeIdleCheck>() {
            Ok(_) => {
                self.rime_idle_check = false;
                self.next_rime_job(host);
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        #[cfg(feature = "rime")]
        let event = match event.downcast::<RimeJobDone>() {
            Ok(done) => return Ok(self.on_rime_job_done(*done, view, host)),
            Err(e) => e,
        };
        #[cfg(feature = "rime")]
        let event = match event.downcast::<FuzzyDeployed>() {
            Ok(d) => {
                self.fuzzy_deploying = false;
                match d.0 {
                    Ok(()) => self.rime_job(RimeJob::Reload, host),
                    Err(e) => self.fuzzy_status = Status::error(format!("模糊音没有应用成功：{e}")),
                }
                if std::mem::take(&mut self.fuzzy_again) {
                    self.apply_fuzzy(host);
                }
                return Ok(Response::none());
            }
            Err(e) => e,
        };
        let _ = view;
        Err(event)
    }

    #[cfg(feature = "rime")]
    fn on_rime_job_done(&mut self, done: RimeJobDone, view: &mut dyn View, host: &mut HostControl) -> Response {
        let RimeJobDone { engine, job, result, words } = done;
        self.rime_busy = false;
        self.pending_rime = Some(engine);
        let r = self.swap_in_rime(view);
        if let Some(n) = words {
            self.user_words = Some(n.min(u32::MAX as usize) as u32);
            // Fresh: a count still waiting for the keyboard to hide isn't needed.
            self.rime_jobs.retain(|j| *j != RimeJob::Count);
        }
        match (job, result) {
            (RimeJob::Reload, Ok(_)) => {
                if !self.fuzzy_deploying {
                    self.fuzzy_status = Status::default();
                }
                if self.settings.fuzzy.iter().any(|&on| on) {
                    self.toast(host, "模糊音已生效");
                }
            }
            (RimeJob::Reload, Err(e)) => self.fuzzy_status = Status::error(format!("模糊音没有应用成功：{e}")),
            (RimeJob::Export(path), Ok(n)) => {
                self.user_dict_status = Status::ok(format!("已导出 {n} 个词"));
                self.toast(host, format!("已导出 {n} 个词"));
                crate::diag::reveal(&path);
            }
            (RimeJob::Import(_), Ok(n)) => {
                self.user_dict_status = Status::ok(format!("已导入 {n} 个词"));
                self.toast(host, format!("已导入 {n} 个词"));
            }
            (RimeJob::Clear, Ok(_)) => {
                self.user_dict_status = Status::ok("已清空用户词库");
                self.toast(host, "已清空用户词库");
            }
            (RimeJob::Count, _) => {}
            (job, Err(e)) => {
                let what = match job {
                    RimeJob::Export(_) => "导出",
                    RimeJob::Import(_) => "导入",
                    _ => "清空",
                };
                self.user_dict_status = Status::error(format!("{what}失败：{e}"));
                self.toast(host, format!("{what}失败：{e}"));
            }
        }
        self.next_rime_job(host);
        r
    }
}
