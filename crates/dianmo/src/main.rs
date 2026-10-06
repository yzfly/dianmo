//! 点墨 Dianmo: the keyboard program (`dianmo.exe`).
//!
//!   dianmo.exe                    start (or show the keyboard of the running instance)
//!   dianmo.exe --hidden           start with the keyboard hidden (tray / edge handle / auto-popup)
//!   dianmo.exe --autostart        same as --hidden (old HKCU Run entries)
//!   dianmo.exe --task             started by the scheduled task `Dianmo`: hidden, never hands over
//!   dianmo.exe --no-elevate       don't hand over to the scheduled task
//!   dianmo.exe --register-task    (elevated) create/update the task `Dianmo` for this exe
//!   dianmo.exe --unregister-task  (elevated) delete the task
//!   dianmo.exe --deploy <dir>     precompile Rime data in <dir> into <dir>\build (packaging)
//!   dianmo.exe --version
//!   --instance <name>             (tests) separate instance: own mutex, data dir, task name
//!
//! Started without elevation (shortcut, double click) while the task `Dianmo` exists and runs this
//! exe, `dianmo.exe` starts the task (elevated, no UAC prompt) and exits, so 点墨 can type into
//! administrator windows (TODO #23, see `elevate.rs`).
//!
//! Errors never show dialogs; see `%APPDATA%\Dianmo\dianmo.log`.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod basic;
#[cfg(windows)]
mod elevate;
mod engine;
mod log;
mod settings;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod platform;

#[cfg(not(windows))]
fn main() {
    eprintln!("点墨 Dianmo 只能在 Windows 上运行");
}

#[cfg(windows)]
fn main() {
    std::process::exit(win::main());
}

#[cfg(windows)]
mod win {
    use std::time::{Duration, Instant};

    use dianmo_ui::{KeyboardConfig, KeyboardView};
    use dianmo_win::HostOptions;

    use crate::app::{DianmoApp, RimeSetup, restore_system_keyboard, take_over_system_keyboard};
    use crate::basic::BasicEngine;
    use crate::engine::AnyEngine;
    use crate::settings::Settings;
    use crate::{elevate, log, platform};

    pub const VERSION: &str = env!("CARGO_PKG_VERSION");

    pub fn main() -> i32 {
        let t0 = Instant::now();
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(i) = args.iter().position(|a| a == "--instance") {
            platform::set_instance(args.get(i + 1).map(String::as_str).unwrap_or(""));
        }
        let data = platform::data_dir();
        crate::log::init(&data.join("dianmo.log"));
        install_panic_hook();

        let mut hidden = false;
        let mut from_task = false;
        let mut no_elevate = false;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--deploy" => return deploy(it.next().map(String::as_str)),
                "--version" | "-V" => {
                    platform::attach_parent_console();
                    println!("点墨 Dianmo {VERSION}");
                    return 0;
                }
                "--register-task" => return register_task(),
                "--unregister-task" => return unregister_task(),
                "--hidden" | "--autostart" => hidden = true,
                elevate::TASK_ARG => {
                    from_task = true;
                    hidden = true;
                }
                "--no-elevate" => no_elevate = true,
                "--instance" => {
                    it.next();
                }
                other => log!("ignoring unknown argument {other:?}"),
            }
        }

        let Some(lock) = platform::acquire_single_instance() else {
            let ok = platform::signal_running_instance();
            log!("already running; asked it to show the keyboard (delivered: {ok})");
            return 0;
        };
        let elevated = elevate::is_elevated();
        let lock = if elevated || from_task || no_elevate {
            lock
        } else {
            match hand_over_to_task(lock, hidden, t0) {
                Ok(()) => return 0,
                Err(Some(lock)) => lock,
                Err(None) => return 0,
            }
        };
        let _lock = lock;
        log!("start {VERSION} {args:?} elevated={elevated}");

        let settings_path = data.join("settings.ini");
        let mut settings = Settings::load(&settings_path);
        settings.autostart = platform::sync_autostart();

        let rime = rime_setup();
        let engine = match &rime {
            RimeSetup::Unavailable(_) => BasicEngine::new(settings.schema),
            #[allow(unreachable_patterns)]
            _ => BasicEngine::loading(settings.schema),
        };
        take_over_system_keyboard(&mut settings, &settings_path);

        let mut view = KeyboardView::new(KeyboardConfig { theme: settings.theme, schema: settings.schema, chinese: settings.chinese });
        view.set_height_scale(settings.height);
        let tray_tip = match &rime {
            RimeSetup::Unavailable(p) => format!("点墨 · 词库未加载（{p}）"),
            #[allow(unreachable_patterns)]
            _ => "点墨 · 触屏输入法".to_owned(),
        };
        let opts = HostOptions { start_visible: !hidden, appbar: settings.appbar, tray_tip, ..HostOptions::default() };
        let _instance_window = platform::create_instance_window().map_err(|e| log!("instance window: {e}"));
        let app = DianmoApp::new(AnyEngine::Basic(engine), rime, settings, settings_path);
        log!("window starting {} ms after launch", t0.elapsed().as_millis());

        let result = dianmo_win::run_with(Box::new(view), Box::new(app), opts);
        // The app (dropped with the host) restores the system keyboard; make sure anyway.
        restore_system_keyboard();
        match result {
            Ok(()) => 0,
            Err(e) => {
                log!("keyboard window failed: {e}");
                1
            }
        }
    }

    /// Not elevated: if the scheduled task runs this exe, start it (elevated 点墨, hidden) and ask
    /// it to show the keyboard unless `hidden`. `Ok` = the elevated instance is up, exit;
    /// `Err(Some(lock))` = run here without elevation; `Err(None)` = another instance appeared.
    fn hand_over_to_task(
        lock: platform::InstanceLock,
        hidden: bool,
        t0: Instant,
    ) -> Result<(), Option<platform::InstanceLock>> {
        let task = platform::task_name();
        if platform::own_task().is_none() {
            log!("not elevated and no task {task} for this exe; running without elevation");
            return Err(Some(lock));
        }
        // The task's instance must be able to take the single-instance mutex.
        drop(lock);
        if let Err(e) = elevate::run(&task) {
            log!("starting task {task} failed: {e}; running without elevation");
            return reacquire();
        }
        if platform::wait_for_instance(Duration::from_secs(10), !hidden) {
            log!("handed over to task {task} ({} ms)", t0.elapsed().as_millis());
            return Ok(());
        }
        log!("task {task} started but no instance appeared within 10 s; running without elevation");
        reacquire()
    }

    fn reacquire() -> Result<(), Option<platform::InstanceLock>> {
        match platform::acquire_single_instance() {
            Some(lock) => Err(Some(lock)),
            None => {
                platform::signal_running_instance();
                Err(None)
            }
        }
    }

    /// `--register-task`: (re)creates the scheduled task for this exe. Must run elevated.
    fn register_task() -> i32 {
        platform::attach_parent_console();
        let task = platform::task_name();
        let exe = std::env::current_exe().unwrap_or_default();
        let mut args = elevate::TASK_ARG.to_owned();
        let argv: Vec<String> = std::env::args().collect();
        if let Some(i) = argv.iter().position(|a| a == "--instance")
            && let Some(name) = argv.get(i + 1)
        {
            args.push_str(&format!(" --instance {name}"));
        }
        // An old HKCU Run entry becomes the task's logon trigger.
        let run_entry = platform::autostart_command().is_some();
        let result = elevate::register(&task, &exe, &args, run_entry.then_some(true));
        let msg = match &result {
            Ok(()) => {
                if run_entry {
                    let _ = platform::set_autostart(true);
                }
                let auto = platform::own_task().is_some_and(|t| t.logon_trigger);
                format!("registered task {task}: \"{}\" {args} (autostart {auto})", exe.display())
            }
            Err(e) => format!("registering task {task} failed: {e}"),
        };
        log!("{msg}");
        println!("{msg}");
        if result.is_ok() { 0 } else { 1 }
    }

    /// `--unregister-task`: deletes the scheduled task. Must run elevated.
    fn unregister_task() -> i32 {
        platform::attach_parent_console();
        let task = platform::task_name();
        let msg = match elevate::unregister(&task) {
            Ok(()) => format!("removed task {task}"),
            Err(e) => format!("removing task {task} failed: {e}"),
        };
        log!("{msg}");
        println!("{msg}");
        if msg.starts_with("removed") { 0 } else { 1 }
    }

    fn install_panic_hook() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log!("panic: {info}");
            restore_system_keyboard();
            prev(info);
        }));
    }

    /// The window always starts with the stand-in engine; librime (if usable) is started in the
    /// background by the app.
    #[cfg(feature = "rime")]
    fn rime_setup() -> RimeSetup {
        if std::env::var("DIANMO_ENGINE").is_ok_and(|v| v == "basic") {
            return RimeSetup::Unavailable("DIANMO_ENGINE=basic".into());
        }
        let mut opts = dianmo_rime::Options::for_app();
        // `--instance`: keep the Rime user data (user dictionary) apart too.
        opts.user_data_dir = platform::data_dir().join("rime");
        if !opts.dll.exists() {
            log!("{} not found; using the built-in engine", opts.dll.display());
            return RimeSetup::Unavailable("缺少 rime.dll".into());
        }
        RimeSetup::Start(opts)
    }

    #[cfg(not(feature = "rime"))]
    fn rime_setup() -> RimeSetup {
        RimeSetup::Unavailable("未编译 Rime".into())
    }

    /// `--deploy <shared dir>`: precompile the Rime data for packaging. Exit code 0 on success.
    #[cfg(feature = "rime")]
    fn deploy(shared: Option<&str>) -> i32 {
        platform::attach_parent_console();
        let mut opts = dianmo_rime::Options::for_app();
        if let Some(dir) = shared {
            opts.shared_data_dir = dir.into();
        }
        log!("deploy {}", opts.shared_data_dir.display());
        match dianmo_rime::deploy(&opts) {
            Ok(r) => {
                let msg = format!("deploy ok: {} ms, build {} bytes", r.millis, r.build_bytes);
                log!("{msg}");
                println!("{msg}");
                0
            }
            Err(e) => {
                log!("deploy failed: {e}");
                println!("deploy failed: {e}");
                1
            }
        }
    }

    #[cfg(not(feature = "rime"))]
    fn deploy(_: Option<&str>) -> i32 {
        platform::attach_parent_console();
        println!("deploy failed: built without the `rime` feature");
        1
    }
}
