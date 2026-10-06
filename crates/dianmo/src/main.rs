//! 点墨 Dianmo: the keyboard program (`dianmo.exe`).
//!
//!   dianmo.exe                    start (or show the keyboard of the running instance)
//!   dianmo.exe --hidden           start with the keyboard hidden (tray / edge handle / auto-popup)
//!   dianmo.exe --autostart        same as --hidden; used by the HKCU Run entry
//!   dianmo.exe --deploy <dir>     precompile Rime data in <dir> into <dir>\build (packaging)
//!   dianmo.exe --version
//!
//! Errors never show dialogs; see `%APPDATA%\Dianmo\dianmo.log`.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod basic;
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
    use std::time::Instant;

    use dianmo_ui::{KeyboardConfig, KeyboardView};
    use dianmo_win::HostOptions;

    use crate::app::{DianmoApp, RimeSetup, restore_system_keyboard, take_over_system_keyboard};
    use crate::basic::BasicEngine;
    use crate::engine::AnyEngine;
    use crate::settings::Settings;
    use crate::{log, platform};

    pub const VERSION: &str = env!("CARGO_PKG_VERSION");

    pub fn main() -> i32 {
        let t0 = Instant::now();
        let args: Vec<String> = std::env::args().skip(1).collect();
        let data = platform::data_dir();
        crate::log::init(&data.join("dianmo.log"));
        install_panic_hook();

        let mut hidden = false;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--deploy" => return deploy(it.next().map(String::as_str)),
                "--version" | "-V" => {
                    platform::attach_parent_console();
                    println!("点墨 Dianmo {VERSION}");
                    return 0;
                }
                "--hidden" | "--autostart" => hidden = true,
                other => log!("ignoring unknown argument {other:?}"),
            }
        }

        let Some(_lock) = platform::acquire_single_instance() else {
            let ok = platform::signal_running_instance();
            log!("already running; asked it to show the keyboard (delivered: {ok})");
            return 0;
        };
        log!("start {VERSION} {args:?}");

        let settings_path = data.join("settings.ini");
        let mut settings = Settings::load(&settings_path);
        settings.autostart = platform::autostart_enabled();
        if platform::autostart_stale() {
            // Installed somewhere else since autostart was turned on: follow the current exe.
            if let Err(e) = platform::set_autostart(true) {
                log!("updating autostart failed: {e}");
            }
        }

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
        let opts = dianmo_rime::Options::for_app();
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
