//! App-window check: a small keyboard strip plus an ordinary window (like the settings window)
//! with a long scrollable list, buttons and images.
//!
//!   window_demo.exe [--open] [--dark]
//!
//! - Keyboard strip buttons: 打开窗口 (open, or bring the window to the front), 关闭窗口, 隐藏键盘
//!   (the host releases the keyboard's device after 5 s), 退出.
//! - Window: header with the "app-icon" image (exe RCDATA) and a 深色 / 关闭 button; a list of
//!   60 rows (tap toggles a row) with a "demo-strip" image card (loaded from `image_dir`, not
//!   embedded). Drag with a finger to scroll (with momentum), mouse wheel, arrow keys / PgUp /
//!   PgDn / Home / End; Esc closes; the mouse highlights the row under it.
//! - `--open`: open the window at start. `--dark`: start with the dark theme (title bar too).
//!
//! With `DIANMO_DEMO_LOG=<file>` events (pointer, wheel, key, hover, taps, open/close, scroll
//! offset) are appended to that file for tests.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    demo::main()
}

#[cfg(windows)]
mod demo {
    use std::fs::File;
    use std::io::Write;

    use dianmo_ui::{
        Align, Canvas, Color, Font, InputState, PointerEvent, PointerPhase, Rect, Response, TextStyle, ThemeKind,
        UiAction, View,
    };
    use dianmo_win::{App, HostControl, HostOptions, WindowId, WindowOptions, now_ms};

    fn log_line(line: &str) {
        if let Some(path) = std::env::var_os("DIANMO_DEMO_LOG")
            && let Ok(mut f) = File::options().create(true).append(true).open(path)
        {
            let _ = writeln!(f, "{} {}", now_ms(), line);
        }
    }

    fn style(size: f32, color: Color, align: Align, bold: bool) -> TextStyle {
        TextStyle { size, color, align, bold, font: Font::Ui }
    }

    const ACCENT: Color = Color::rgb(0x3B, 0x6C, 0xF6);

    // ---- keyboard strip ------------------------------------------------------------------

    const STRIP_BUTTONS: [&str; 4] = ["打开窗口", "关闭窗口", "隐藏键盘", "退出"];

    struct Strip {
        width: f32,
        pressed: Option<usize>,
    }

    impl Strip {
        fn button(&self, i: usize) -> Rect {
            let w = 160.0;
            Rect::new(12.0 + i as f32 * (w + 12.0), 8.0, w, 48.0)
        }
    }

    impl View for Strip {
        fn resize(&mut self, width: f32, _height: f32) {
            self.width = width;
        }
        fn preferred_height(&self, _width: f32) -> f32 {
            64.0
        }
        fn paint(&mut self, c: &mut dyn Canvas) {
            c.clear(Color::rgb(0xD5, 0xD8, 0xDE));
            for (i, label) in STRIP_BUTTONS.iter().enumerate() {
                let r = self.button(i);
                let bg = if self.pressed == Some(i) { Color::rgb(0x9C, 0xA3, 0xAF) } else { Color::rgb(0xFF, 0xFF, 0xFF) };
                c.fill_rect(r, 8.0, bg);
                c.text(label, r, style(17.0, Color::rgb(0x11, 0x18, 0x27), Align::Center, false));
            }
            let note = Rect::new(12.0 + 4.0 * 172.0, 8.0, (self.width - 712.0).max(0.0), 48.0);
            c.text("window_demo · 键盘窗口（不抢焦点）", note, style(14.0, Color::rgb(0x6B, 0x72, 0x80), Align::Start, false));
        }
        fn pointer(&mut self, e: PointerEvent) -> Response {
            let hit = (0..STRIP_BUTTONS.len()).find(|&i| self.button(i).contains(e.x, e.y));
            match e.phase {
                PointerPhase::Down => {
                    self.pressed = hit;
                    Response::repaint()
                }
                PointerPhase::Up => {
                    let was = self.pressed.take();
                    let mut r = Response::repaint();
                    if was.is_some() && was == hit {
                        r.actions.push(UiAction::Paste(STRIP_BUTTONS[was.unwrap()].to_owned()));
                    }
                    r
                }
                PointerPhase::Cancel => {
                    self.pressed = None;
                    Response::repaint()
                }
                PointerPhase::Move => Response::none(),
            }
        }
        fn timer(&mut self, _now_ms: u64) -> Response {
            Response::none()
        }
        fn set_input_state(&mut self, _state: InputState) -> Response {
            Response::none()
        }
        fn set_more_candidates(&mut self, _start: usize, _c: Vec<dianmo_core::Candidate>) -> Response {
            Response::none()
        }
        fn set_t9_spellings(&mut self, _s: Vec<String>) -> Response {
            Response::none()
        }
    }

    // ---- the window's view -----------------------------------------------------------------

    const HEADER_H: f32 = 76.0;
    const ROW_H: f32 = 56.0;
    const ROWS: usize = 60;
    const CARD_H: f32 = 180.0;
    const PAD: f32 = 24.0;
    const FRAME_MS: u64 = 16;
    /// Movement before a press becomes a drag (DIPs).
    const SLOP: f32 = 8.0;

    struct Palette {
        bg: Color,
        card: Color,
        ink: Color,
        muted: Color,
        line: Color,
        hover: Color,
        pressed: Color,
    }

    fn palette(dark: bool) -> Palette {
        if dark {
            Palette {
                bg: Color::rgb(0x1E, 0x1F, 0x22),
                card: Color::rgb(0x2B, 0x2D, 0x31),
                ink: Color::rgb(0xF2, 0xF3, 0xF5),
                muted: Color::rgb(0x9A, 0xA0, 0xA6),
                line: Color::rgb(0x3A, 0x3C, 0x41),
                hover: Color::rgb(0x34, 0x37, 0x3C),
                pressed: Color::rgb(0x40, 0x44, 0x4A),
            }
        } else {
            Palette {
                bg: Color::rgb(0xF5, 0xF6, 0xF8),
                card: Color::rgb(0xFF, 0xFF, 0xFF),
                ink: Color::rgb(0x1F, 0x23, 0x29),
                muted: Color::rgb(0x86, 0x90, 0x9C),
                line: Color::rgb(0xE5, 0xE6, 0xEB),
                hover: Color::rgb(0xF0, 0xF2, 0xF5),
                pressed: Color::rgb(0xE3, 0xE6, 0xEB),
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Hit {
        Dark,
        Close,
        Row(usize),
    }

    struct Press {
        id: u32,
        x0: f32,
        y0: f32,
        offset0: f32,
        dragging: bool,
        hit: Option<Hit>,
        /// Recent (time, y) samples for the fling velocity.
        samples: Vec<(u64, f32)>,
    }

    struct DemoWindowView {
        w: f32,
        h: f32,
        dark: bool,
        offset: f32,
        vel: f32,
        last_frame: u64,
        press: Option<Press>,
        hover: Option<Hit>,
        on: Vec<bool>,
        /// The view logs its own drop (memory release check).
        _alive: Vec<u8>,
    }

    impl Drop for DemoWindowView {
        fn drop(&mut self) {
            log_line("view dropped");
        }
    }

    impl DemoWindowView {
        fn new(dark: bool) -> Self {
            Self {
                w: 0.0,
                h: 0.0,
                dark,
                offset: 0.0,
                vel: 0.0,
                last_frame: 0,
                press: None,
                hover: None,
                on: vec![false; ROWS],
                // 4 MB that must be freed when the window closes.
                _alive: vec![1u8; 4 << 20],
            }
        }

        fn content_h(&self) -> f32 {
            PAD + CARD_H + PAD + ROWS as f32 * ROW_H + PAD
        }

        fn max_offset(&self) -> f32 {
            (self.content_h() - (self.h - HEADER_H)).max(0.0)
        }

        fn set_offset(&mut self, o: f32) -> bool {
            let o = o.clamp(0.0, self.max_offset());
            let changed = o != self.offset;
            self.offset = o;
            changed
        }

        fn dark_button(&self) -> Rect {
            Rect::new(self.w - PAD - 2.0 * 96.0 - 12.0, 16.0, 96.0, 44.0)
        }

        fn close_button(&self) -> Rect {
            Rect::new(self.w - PAD - 96.0, 16.0, 96.0, 44.0)
        }

        fn list_x(&self) -> (f32, f32) {
            let w = (self.w - 2.0 * PAD).min(720.0);
            ((self.w - w) / 2.0, w)
        }

        fn row_rect(&self, i: usize) -> Rect {
            let (x, w) = self.list_x();
            Rect::new(x, HEADER_H + PAD + CARD_H + PAD + i as f32 * ROW_H - self.offset, w, ROW_H)
        }

        fn hit(&self, x: f32, y: f32) -> Option<Hit> {
            if y < HEADER_H {
                if self.dark_button().contains(x, y) {
                    return Some(Hit::Dark);
                }
                if self.close_button().contains(x, y) {
                    return Some(Hit::Close);
                }
                return None;
            }
            let first = self.row_rect(0);
            if !(first.x..first.x + first.w).contains(&x) || y < first.y {
                return None;
            }
            let i = ((y - first.y) / ROW_H) as usize;
            (i < ROWS).then_some(Hit::Row(i))
        }

        fn activate(&mut self, hit: Hit) -> Response {
            log_line(&format!("tap {hit:?}"));
            let mut r = Response::repaint();
            match hit {
                Hit::Dark => {
                    self.dark = !self.dark;
                    r.actions.push(UiAction::ThemeChanged(if self.dark { ThemeKind::Dark } else { ThemeKind::Light }));
                }
                Hit::Close => r.actions.push(UiAction::Hide),
                Hit::Row(i) => self.on[i] = !self.on[i],
            }
            r
        }

        fn fling_frame(&mut self, now: u64) -> Response {
            let dt = now.saturating_sub(self.last_frame).clamp(1, 50) as f32;
            self.last_frame = now;
            let moved = self.set_offset(self.offset + self.vel * dt);
            self.vel *= 0.95f32.powf(dt / FRAME_MS as f32);
            if !moved || self.vel.abs() < 0.02 {
                self.vel = 0.0;
                log_line(&format!("fling end offset={:.0}", self.offset));
                return Response::repaint();
            }
            Response { repaint: true, timer_ms: Some(FRAME_MS), ..Response::default() }
        }
    }

    impl View for DemoWindowView {
        fn resize(&mut self, width: f32, height: f32) {
            log_line(&format!("resize {width:.0}x{height:.0}"));
            self.w = width;
            self.h = height;
            self.set_offset(self.offset);
        }

        fn preferred_height(&self, _width: f32) -> f32 {
            self.h
        }

        fn paint(&mut self, c: &mut dyn Canvas) {
            let p = palette(self.dark);
            c.clear(p.bg);

            // Scrolling content (clipped below the header).
            c.push_clip(Rect::new(0.0, HEADER_H, self.w, (self.h - HEADER_H).max(0.0)));
            let (lx, lw) = self.list_x();
            let card = Rect::new(lx, HEADER_H + PAD - self.offset, lw, CARD_H);
            c.fill_rect(card, 12.0, p.card);
            c.image("demo-strip", card.inset(16.0));
            let first = ((self.offset - PAD - CARD_H - PAD) / ROW_H).floor().max(0.0) as usize;
            for i in first..ROWS {
                let r = self.row_rect(i);
                if r.y > self.h {
                    break;
                }
                let pressed = self.press.as_ref().is_some_and(|pr| !pr.dragging && pr.hit == Some(Hit::Row(i)));
                let bg = if pressed {
                    p.pressed
                } else if self.hover == Some(Hit::Row(i)) {
                    p.hover
                } else {
                    p.card
                };
                c.fill_rect(r, 0.0, bg);
                c.fill_rect(Rect::new(r.x + 16.0, r.y + r.h - 1.0, r.w - 32.0, 1.0), 0.0, p.line);
                c.text(&format!("第 {} 项", i + 1), Rect::new(r.x + 16.0, r.y, 200.0, r.h), style(15.0, p.ink, Align::Start, false));
                c.text(
                    "点一下切换；手指拖动滚动",
                    Rect::new(r.x + 120.0, r.y, r.w - 220.0, r.h),
                    style(12.0, p.muted, Align::Start, false),
                );
                // Switch.
                let sw = Rect::new(r.x + r.w - 16.0 - 48.0, r.y + (r.h - 28.0) / 2.0, 48.0, 28.0);
                c.fill_rect(sw, 14.0, if self.on[i] { ACCENT } else { p.line });
                let kx = if self.on[i] { sw.x + 22.0 } else { sw.x + 2.0 };
                c.fill_rect(Rect::new(kx, sw.y + 2.0, 24.0, 24.0), 12.0, Color::rgb(0xFF, 0xFF, 0xFF));
            }
            c.pop_clip();

            // Header.
            c.fill_rect(Rect::new(0.0, 0.0, self.w, HEADER_H), 0.0, p.bg);
            c.image("app-icon", Rect::new(PAD, 14.0, 48.0, 48.0));
            c.text("点墨 · 窗口演示", Rect::new(PAD + 60.0, 10.0, 300.0, 32.0), style(20.0, p.ink, Align::Start, true));
            c.text(
                &format!("滚动 {:.0} / {:.0}", self.offset, self.max_offset()),
                Rect::new(PAD + 60.0, 42.0, 300.0, 22.0),
                style(12.0, p.muted, Align::Start, false),
            );
            for (hit, rect, label) in [(Hit::Dark, self.dark_button(), if self.dark { "浅色" } else { "深色" }), (Hit::Close, self.close_button(), "关闭")] {
                let pressed = self.press.as_ref().is_some_and(|pr| pr.hit == Some(hit));
                let bg = if hit == Hit::Close { ACCENT } else if pressed || self.hover == Some(hit) { p.pressed } else { p.card };
                c.fill_rect(rect, 8.0, bg);
                let ink = if hit == Hit::Close { Color::rgb(0xFF, 0xFF, 0xFF) } else { p.ink };
                c.text(label, rect, style(15.0, ink, Align::Center, false));
            }
            c.fill_rect(Rect::new(0.0, HEADER_H - 1.0, self.w, 1.0), 0.0, p.line);
        }

        fn pointer(&mut self, e: PointerEvent) -> Response {
            log_line(&format!("pointer {} {:?} {:.0},{:.0}", e.id, e.phase, e.x, e.y));
            match e.phase {
                PointerPhase::Down => {
                    if self.press.is_some() {
                        return Response::none(); // one finger at a time
                    }
                    self.vel = 0.0;
                    self.press = Some(Press {
                        id: e.id,
                        x0: e.x,
                        y0: e.y,
                        offset0: self.offset,
                        dragging: false,
                        hit: self.hit(e.x, e.y),
                        samples: vec![(e.time_ms, e.y)],
                    });
                    Response::repaint()
                }
                PointerPhase::Move => {
                    let Some(pr) = self.press.as_mut().filter(|p| p.id == e.id) else { return Response::none() };
                    pr.samples.push((e.time_ms, e.y));
                    pr.samples.retain(|s| e.time_ms.saturating_sub(s.0) <= 100);
                    if !pr.dragging && ((e.y - pr.y0).abs() > SLOP || (e.x - pr.x0).abs() > SLOP) {
                        // Only list presses scroll; header buttons just stop being pressed.
                        pr.dragging = true;
                        pr.offset0 = self.offset;
                        pr.y0 = e.y;
                    }
                    if pr.dragging && e.y >= 0.0 {
                        let target = pr.offset0 - (e.y - pr.y0);
                        let in_list = !matches!(pr.hit, Some(Hit::Dark | Hit::Close));
                        if in_list {
                            self.set_offset(target);
                        }
                    }
                    Response::repaint()
                }
                PointerPhase::Up => {
                    let Some(pr) = self.press.take().filter(|p| p.id == e.id) else { return Response::none() };
                    if pr.dragging {
                        let (t0, y0) = pr.samples.first().copied().unwrap_or((e.time_ms, e.y));
                        let dt = e.time_ms.saturating_sub(t0) as f32;
                        let v = if dt > 0.0 { (e.y - y0) / dt } else { 0.0 };
                        log_line(&format!("drag end offset={:.0} v={v:.2}", self.offset));
                        if v.abs() > 0.15 && !matches!(pr.hit, Some(Hit::Dark | Hit::Close)) {
                            self.vel = (-v).clamp(-6.0, 6.0);
                            self.last_frame = e.time_ms;
                            return Response { repaint: true, timer_ms: Some(FRAME_MS), ..Response::default() };
                        }
                        return Response::repaint();
                    }
                    match pr.hit {
                        Some(hit) if self.hit(e.x, e.y) == Some(hit) => self.activate(hit),
                        _ => Response::repaint(),
                    }
                }
                PointerPhase::Cancel => {
                    self.press = None;
                    Response::repaint()
                }
            }
        }

        fn timer(&mut self, now_ms: u64) -> Response {
            if self.vel != 0.0 { self.fling_frame(now_ms) } else { Response::none() }
        }

        fn wheel(&mut self, x: f32, y: f32, delta_y: f32) -> Response {
            self.vel = 0.0;
            let changed = self.set_offset(self.offset + delta_y);
            log_line(&format!("wheel {x:.0},{y:.0} d={delta_y:.1} offset={:.0}", self.offset));
            if changed { Response::repaint() } else { Response::none() }
        }

        fn key(&mut self, vk: u32, down: bool) -> Response {
            log_line(&format!("key {vk:#x} {}", if down { "down" } else { "up" }));
            if !down {
                return Response::none();
            }
            let page = (self.h - HEADER_H) * 0.9;
            let target = match vk {
                0x1B => return Response { actions: vec![UiAction::Hide], ..Response::default() },
                0x26 => self.offset - 40.0,
                0x28 => self.offset + 40.0,
                0x21 => self.offset - page,
                0x22 => self.offset + page,
                0x24 => 0.0,
                0x23 => self.max_offset(),
                _ => return Response::none(),
            };
            self.vel = 0.0;
            if self.set_offset(target) { Response::repaint() } else { Response::none() }
        }

        fn hover(&mut self, x: f32, y: f32) -> Response {
            let hit = if x < 0.0 { None } else { self.hit(x, y) };
            if hit == self.hover {
                return Response::none();
            }
            log_line(&format!("hover {hit:?}"));
            self.hover = hit;
            Response::repaint()
        }

        fn set_input_state(&mut self, _state: InputState) -> Response {
            Response::none()
        }
        fn set_more_candidates(&mut self, _start: usize, _c: Vec<dianmo_core::Candidate>) -> Response {
            Response::none()
        }
        fn set_t9_spellings(&mut self, _s: Vec<String>) -> Response {
            Response::none()
        }
    }

    // ---- app ----------------------------------------------------------------------------------

    struct DemoApp {
        window: Option<WindowId>,
        dark: bool,
        open_at_start: bool,
    }

    impl DemoApp {
        fn open(&mut self, host: &mut HostControl) {
            if let Some(id) = self.window.filter(|&id| host.window_open(id)) {
                log_line(&format!("focus {id:?}"));
                host.focus_window(id);
                return;
            }
            let opts = WindowOptions {
                title: "点墨 · 窗口演示".to_owned(),
                width: 760.0,
                height: 560.0,
                min_width: 420.0,
                min_height: 320.0,
                dark: Some(self.dark),
                ..WindowOptions::default()
            };
            let id = host.open_window(Box::new(DemoWindowView::new(self.dark)), opts);
            log_line(&format!("open {id:?}"));
            self.window = Some(id);
        }
    }

    impl App for DemoApp {
        fn on_start(&mut self, _view: &mut dyn View, host: &mut HostControl) -> Response {
            log_line(&format!("start system_dark={}", dianmo_win::system_dark_mode()));
            if self.open_at_start {
                self.open(host);
            }
            Response::none()
        }

        fn on_action(&mut self, action: UiAction, _view: &mut dyn View, host: &mut HostControl) -> Response {
            if let UiAction::Paste(cmd) = action {
                log_line(&format!("strip {cmd}"));
                match cmd.as_str() {
                    "打开窗口" => self.open(host),
                    "关闭窗口" => {
                        if let Some(id) = self.window {
                            host.close_window(id);
                        }
                    }
                    "隐藏键盘" => host.hide(),
                    "退出" => host.quit(),
                    _ => {}
                }
            }
            Response::none()
        }

        fn on_window_action(&mut self, id: WindowId, action: UiAction, _view: &mut dyn View, host: &mut HostControl) -> Response {
            log_line(&format!("window action {id:?} {action:?}"));
            match action {
                UiAction::Hide => host.close_window(id),
                UiAction::ThemeChanged(kind) => {
                    self.dark = kind == ThemeKind::Dark;
                    host.set_window_dark(id, Some(self.dark));
                }
                _ => {}
            }
            Response::none()
        }

        fn on_window_closed(&mut self, id: WindowId) {
            log_line(&format!("closed {id:?}"));
            if self.window == Some(id) {
                self.window = None;
            }
        }
    }

    pub fn main() -> windows::core::Result<()> {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let app = DemoApp { window: None, dark: has("--dark"), open_at_start: has("--open") };
        let opts = HostOptions {
            appbar: false,
            tray: false,
            edge_handle: false,
            image_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/res").into()),
            ..HostOptions::default()
        };
        dianmo_win::run_with(Box::new(Strip { width: 0.0, pressed: None }), Box::new(app), opts)
    }
}
