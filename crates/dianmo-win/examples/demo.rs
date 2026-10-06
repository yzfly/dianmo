//! End-to-end check of the platform layer: a few big keys and a candidate-like strip that type
//! into the focused app through `SendInputSink`, without ever taking focus.
//!
//!   demo.exe [--no-appbar] [--hidden] [--no-tray] [--no-handle] [--focus] [--auto] [--tray-menu]
//!            [--voice-ball] [--ball-right]
//!
//! - `--focus`: start the UI Automation focus watcher and log every `FocusEvent`.
//! - `--auto`: with `--focus`, show the keyboard on `Editable { by_touch: true }` and hide it on
//!   `NotEditable { by_touch: true }` (the auto show/hide rule of DESIGN.md §2).
//! - `--tray-menu`: app items in the tray menu (a 布局 submenu with radio-like checks, a 深色主题
//!   switch); choices are logged.
//! - `--voice-ball`: the floating ball acts as the voice ball: a tap toggles the listening halo,
//!   a long press shows the keyboard. Ball events are logged either way. `--ball-right` starts
//!   the ball on the right edge.
//!
//! With `DIANMO_DEMO_LOG=<file>` every pointer event, focus event, tray command and visibility
//! change is appended to that file (for tests).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    demo::main()
}

#[cfg(windows)]
mod demo {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;

    use dianmo_core::{Action, Candidate, EditKey, TextSink};
    use dianmo_ui::{
        Align, Canvas, Color, Font, InputState, PointerEvent, PointerPhase, Rect, Response, TextStyle, UiAction, View,
    };
    use dianmo_win::{
        App, BallEdge, BallEvent, BallPos, BallState, FocusEvent, FocusWatcher, HostControl, HostOptions, SendInputSink, TrayItem, now_ms, start_focus_watcher,
        start_voice_typing,
    };

    /// Appends a line to `DIANMO_DEMO_LOG` (if set).
    fn log_line(line: &str) {
        if let Some(path) = std::env::var_os("DIANMO_DEMO_LOG")
            && let Ok(mut f) = File::options().create(true).append(true).open(path)
        {
            let _ = writeln!(f, "{} {}", now_ms(), line);
        }
    }

    const BG: Color = Color::rgb(0xD5, 0xD8, 0xDE);
    const KEY: Color = Color::rgb(0xFF, 0xFF, 0xFF);
    const KEY_FN: Color = Color::rgb(0xAB, 0xB1, 0xBB);
    const KEY_DOWN: Color = Color::rgb(0x9C, 0xA3, 0xAF);
    const INK: Color = Color::rgb(0x11, 0x18, 0x27);
    const MUTED: Color = Color::rgb(0x6B, 0x72, 0x80);
    const ACCENT: Color = Color::rgb(0x25, 0x5F, 0xB0);
    const STRIP_H: f32 = 52.0;

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Text(&'static str),
        Backspace,
        Enter,
        Voice,
        Hide,
    }

    struct Key {
        label: &'static str,
        icon: bool,
        kind: Kind,
        rect: Rect,
        func: bool,
    }

    const CANDIDATES: [&str; 5] = ["你好", "你", "好", "世界", "点墨"];

    struct DemoView {
        width: f32,
        keys: Vec<Key>,
        chips: Vec<(Rect, &'static str)>,
        /// Pointer id → index into `keys` (or `keys.len() + chip`).
        pressed: HashMap<u32, usize>,
        max_simultaneous: usize,
        touches: u32,
        repeating: Option<u32>,
        log: Option<File>,
    }

    impl DemoView {
        fn new() -> Self {
            let log = std::env::var_os("DIANMO_DEMO_LOG")
                .and_then(|p| File::options().create(true).append(true).open(p).ok());
            Self {
                width: 0.0,
                keys: Vec::new(),
                chips: Vec::new(),
                pressed: HashMap::new(),
                max_simultaneous: 0,
                touches: 0,
                repeating: None,
                log,
            }
        }

        fn row_height(width: f32) -> f32 {
            (width * 0.075).clamp(64.0, 104.0)
        }

        fn hit(&self, x: f32, y: f32) -> Option<usize> {
            if let Some(i) = self.keys.iter().position(|k| k.rect.contains(x, y)) {
                return Some(i);
            }
            self.chips.iter().position(|(r, _)| r.contains(x, y)).map(|i| self.keys.len() + i)
        }

        fn activate(&self, target: usize) -> Vec<UiAction> {
            if target >= self.keys.len() {
                let text = self.chips[target - self.keys.len()].1;
                return vec![UiAction::Input(Action::Text(text.to_owned()))];
            }
            match self.keys[target].kind {
                Kind::Text(t) => vec![UiAction::Input(Action::Text(t.to_owned()))],
                Kind::Backspace => vec![], // acts on press (and repeats)
                Kind::Enter => vec![UiAction::Input(Action::Enter)],
                Kind::Voice => vec![UiAction::Voice],
                Kind::Hide => vec![UiAction::Hide],
            }
        }

        fn log(&mut self, ev: &PointerEvent) {
            let active = self.pressed.len();
            if let Some(f) = &mut self.log {
                let _ = writeln!(
                    f,
                    "{} id={} {:?} x={:.0} y={:.0} active={} max={}",
                    ev.time_ms, ev.id, ev.phase, ev.x, ev.y, active, self.max_simultaneous
                );
            }
        }
    }

    impl View for DemoView {
        fn resize(&mut self, width: f32, height: f32) {
            self.width = width;
            let rh = (height - STRIP_H) / 2.0;
            let gap = 6.0;
            let row = |specs: &[(&'static str, bool, Kind, f32, bool)], y: f32, keys: &mut Vec<Key>| {
                let total: f32 = specs.iter().map(|s| s.3).sum();
                let unit = (width - gap) / total;
                let mut x = gap / 2.0;
                for &(label, icon, kind, span, func) in specs {
                    let w = unit * span;
                    keys.push(Key { label, icon, kind, rect: Rect::new(x, y, w, rh).inset(gap / 2.0), func });
                    x += w;
                }
            };
            let mut keys = Vec::new();
            row(
                &[
                    ("你", false, Kind::Text("你"), 1.0, false),
                    ("好", false, Kind::Text("好"), 1.0, false),
                    ("，", false, Kind::Text("，"), 1.0, false),
                    ("。", false, Kind::Text("。"), 1.0, false),
                    ("\u{E750}", true, Kind::Backspace, 1.5, true),
                ],
                STRIP_H,
                &mut keys,
            );
            row(
                &[
                    ("\u{E720}", true, Kind::Voice, 1.0, true),
                    ("空格", false, Kind::Text(" "), 2.5, false),
                    ("\u{E751}", true, Kind::Enter, 1.0, true),
                    ("\u{E70D}", true, Kind::Hide, 1.0, true),
                ],
                STRIP_H + rh,
                &mut keys,
            );
            self.keys = keys;
        }

        fn preferred_height(&self, width: f32) -> f32 {
            STRIP_H + 2.0 * Self::row_height(width)
        }

        fn paint(&mut self, c: &mut dyn Canvas) {
            c.clear(BG);
            let style = |size, color, align, font| TextStyle { size, color, align, bold: false, font };
            // Candidate strip: pinyin, candidates, status.
            c.text("ni hao", Rect::new(14.0, 0.0, 80.0, 22.0), style(13.0, MUTED, Align::Start, Font::Ui));
            let mut x = 14.0;
            let cand = style(22.0, INK, Align::Center, Font::Ui);
            let mut chips = Vec::new();
            for (i, text) in CANDIDATES.iter().enumerate() {
                let w = c.measure_text(text, cand) + 28.0;
                let r = Rect::new(x, 16.0, w, STRIP_H - 18.0);
                let down = self.pressed.values().any(|&k| k == self.keys.len() + i);
                if down {
                    c.fill_rect(r, 8.0, KEY_DOWN);
                }
                let color = if i == 0 { ACCENT } else { INK };
                c.text(text, r, TextStyle { color, bold: i == 0, ..cand });
                chips.push((r, *text));
                x += w;
            }
            self.chips = chips;
            let status = format!(
                "点墨 demo · 触点 {} · 同时最多 {} · 按下 {}",
                self.touches,
                self.max_simultaneous,
                self.pressed.len()
            );
            c.text(&status, Rect::new(x + 16.0, 0.0, self.width - x - 30.0, STRIP_H), style(14.0, MUTED, Align::End, Font::Ui));

            for (i, k) in self.keys.iter().enumerate() {
                let down = self.pressed.values().any(|&p| p == i);
                let fill = if down { KEY_DOWN } else if k.func { KEY_FN } else { KEY };
                c.fill_rect(Rect { y: k.rect.y + 1.5, ..k.rect }, 8.0, Color::rgb(0x89, 0x8A, 0x8D));
                c.fill_rect(k.rect, 8.0, fill);
                let (size, font) = if k.icon { (26.0, Font::Icon) } else { (30.0, Font::Ui) };
                c.text(k.label, k.rect, style(size, INK, Align::Center, font));
            }
            c.stroke_rect(Rect::new(0.0, 0.0, self.width, 1.0), 0.0, 1.0, Color::rgb(0xB0, 0xB4, 0xBA));
        }

        fn pointer(&mut self, ev: PointerEvent) -> Response {
            let mut resp = Response::repaint();
            match ev.phase {
                PointerPhase::Down => {
                    self.touches += 1;
                    if let Some(t) = self.hit(ev.x, ev.y) {
                        self.pressed.insert(ev.id, t);
                        if t < self.keys.len() && self.keys[t].kind == Kind::Backspace {
                            resp.actions.push(UiAction::Input(Action::Backspace));
                            self.repeating = Some(ev.id);
                            resp.timer_ms = Some(400);
                        }
                    }
                    self.max_simultaneous = self.max_simultaneous.max(self.pressed.len());
                }
                PointerPhase::Move => {
                    // Sliding off a key cancels it (phone behaviour).
                    if let Some(&t) = self.pressed.get(&ev.id)
                        && self.hit(ev.x, ev.y) != Some(t)
                    {
                        self.pressed.remove(&ev.id);
                    }
                }
                PointerPhase::Up => {
                    if let Some(t) = self.pressed.remove(&ev.id) {
                        resp.actions.extend(self.activate(t));
                    }
                }
                PointerPhase::Cancel => {
                    self.pressed.remove(&ev.id);
                }
            }
            if self.repeating.is_some_and(|id| !self.pressed.contains_key(&id)) {
                self.repeating = None;
            }
            self.log(&ev);
            resp
        }

        fn timer(&mut self, _now_ms: u64) -> Response {
            match self.repeating {
                Some(id) if self.pressed.contains_key(&id) => Response {
                    repaint: false,
                    actions: vec![UiAction::Input(Action::Backspace)],
                    timer_ms: Some(70),
                },
                _ => Response::none(),
            }
        }

        fn set_input_state(&mut self, _state: InputState) -> Response {
            Response::none()
        }

        fn set_more_candidates(&mut self, _start: usize, _candidates: Vec<Candidate>) -> Response {
            Response::none()
        }

        fn set_t9_spellings(&mut self, _spellings: Vec<String>) -> Response {
            Response::none()
        }
    }

    const LAYOUTS: [&str; 3] = ["全拼", "小鹤双拼", "九宫格"];
    const CMD_LAYOUT: u32 = 10;
    const CMD_DARK: u32 = 20;
    const CMD_ABOUT: u32 = 30;

    struct DemoApp {
        sink: SendInputSink,
        focus: bool,
        auto: bool,
        watcher: Option<FocusWatcher>,
        layout: usize,
        dark: bool,
        tray_menu: bool,
        voice_ball: bool,
        listening: bool,
    }

    impl DemoApp {
        fn tray_items(&self) -> Vec<TrayItem> {
            let layouts = LAYOUTS
                .iter()
                .enumerate()
                .map(|(i, l)| TrayItem::Command { id: CMD_LAYOUT + i as u32, label: (*l).to_owned(), checked: i == self.layout })
                .collect();
            vec![
                TrayItem::Submenu { label: "布局".to_owned(), items: layouts },
                TrayItem::Command { id: CMD_DARK, label: "深色主题".to_owned(), checked: self.dark },
                TrayItem::Separator,
                TrayItem::Command { id: CMD_ABOUT, label: "关于点墨 demo".to_owned(), checked: false },
            ]
        }
    }

    impl App for DemoApp {
        fn on_start(&mut self, _view: &mut dyn View, host: &mut HostControl) -> Response {
            if self.focus {
                let t0 = now_ms();
                match start_focus_watcher(host.proxy()) {
                    Ok(w) => {
                        log_line(&format!("focus watcher started in {}ms", now_ms() - t0));
                        self.watcher = Some(w);
                    }
                    Err(e) => log_line(&format!("focus watcher failed: {e}")),
                }
            }
            Response::none()
        }

        fn on_event(&mut self, event: Box<dyn std::any::Any + Send>, _view: &mut dyn View, host: &mut HostControl) -> Response {
            if let Ok(ev) = event.downcast::<FocusEvent>() {
                log_line(&format!("focus {:?} fullscreen={}", *ev, host.fullscreen_app()));
                if self.auto {
                    match *ev {
                        FocusEvent::Editable { by_touch: true, .. } => host.show(),
                        FocusEvent::NotEditable { by_touch: true } => host.hide(),
                        _ => {}
                    }
                }
            }
            Response::none()
        }

        fn on_ball(&mut self, event: BallEvent, _view: &mut dyn View, host: &mut HostControl) -> Response {
            log_line(&format!("ball {event:?}"));
            match event {
                BallEvent::Tap if self.voice_ball => {
                    self.listening = !self.listening;
                    host.set_ball_state(if self.listening { BallState::Listening } else { BallState::Idle });
                }
                BallEvent::Tap | BallEvent::LongPress => {
                    if self.listening {
                        self.listening = false;
                        host.set_ball_state(BallState::Idle);
                    }
                    host.show();
                }
                BallEvent::Moved(_) => {}
            }
            Response::none()
        }

        fn on_visibility_changed(&mut self, visible: bool, _view: &mut dyn View, _host: &mut HostControl) -> Response {
            log_line(&format!("visible {visible}"));
            Response::none()
        }

        fn on_tray_command(&mut self, id: u32, _view: &mut dyn View, host: &mut HostControl) -> Response {
            log_line(&format!("tray command {id}"));
            match id {
                CMD_DARK => self.dark = !self.dark,
                id if (CMD_LAYOUT..CMD_LAYOUT + LAYOUTS.len() as u32).contains(&id) => {
                    self.layout = (id - CMD_LAYOUT) as usize
                }
                _ => {}
            }
            if self.tray_menu {
                host.set_tray_menu(self.tray_items());
            }
            Response::none()
        }

        fn on_action(&mut self, action: UiAction, _view: &mut dyn View, host: &mut HostControl) -> Response {
            match action {
                UiAction::Input(Action::Text(t)) => self.sink.commit_text(&t),
                UiAction::Input(Action::Char(c)) => self.sink.commit_text(c.encode_utf8(&mut [0; 4])),
                UiAction::Input(Action::Backspace) => self.sink.send_key(EditKey::Backspace),
                UiAction::Input(Action::Enter) => self.sink.send_key(EditKey::Enter),
                UiAction::Input(Action::Space) => self.sink.commit_text(" "),
                UiAction::Input(Action::Edit(k)) => self.sink.send_key(k),
                UiAction::Voice => {
                    start_voice_typing();
                }
                UiAction::Hide => host.hide(),
                _ => {}
            }
            Response::none()
        }
    }

    pub fn main() -> windows::core::Result<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let has = |f: &str| args.iter().any(|a| a == f);
        let app = DemoApp {
            sink: SendInputSink::new(),
            focus: has("--focus") || has("--auto"),
            auto: has("--auto"),
            watcher: None,
            layout: 0,
            dark: false,
            tray_menu: has("--tray-menu"),
            voice_ball: has("--voice-ball"),
            listening: false,
        };
        let opts = HostOptions {
            appbar: !has("--no-appbar"),
            start_visible: !has("--hidden"),
            tray: !has("--no-tray"),
            edge_handle: !has("--no-handle"),
            tray_menu: if app.tray_menu { app.tray_items() } else { Vec::new() },
            ball_pos: has("--ball-right").then_some(BallPos { edge: BallEdge::Right, y_frac: 0.85 }),
            ..HostOptions::default()
        };
        dianmo_win::run_with(Box::new(DemoView::new()), Box::new(app), opts)
    }
}
