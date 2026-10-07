//! First-run onboarding (PRODUCT.md P3): ① welcome ② default layout ③ voice engine ④ gestures.
//! Cards slide sideways with the finger (snap with a short ease), dots at the bottom, 跳过 /
//! 上一步 / 下一步 / 开始使用. Choices apply immediately (`SetLayout`, `SetVoiceEngine`);
//! finishing or skipping sends `FinishOnboarding`.

use crate::canvas::{Align, Canvas, Rect, TextStyle};
use crate::scroll::{VelocityTracker, FRAME_MS};
use crate::theme::{SettingsTheme, ThemeKind};
use crate::view::{InputState, PointerEvent, PointerPhase, Response, View};

use super::about::app_icon;
use super::art::{art_ball, art_swipe, art_trackpad, mini_keyboard};
use super::model::{LayoutChoice, Level, SettingsAction, SettingsModel, Status, VoiceEngineChoice};
use super::widgets::{bold, button, centered, icon, mix, radio, shadow, status_line, style, text_w, BtnState, ButtonKind};

pub const ONBOARDING_PAGES: usize = 4;

const BAR_H: f32 = 96.0;
const SLIDE_MS: u64 = 260;
const SLOP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Skip,
    Back,
    Next,
    Finish,
    Dot(usize),
    Layout(LayoutChoice),
    Engine(VoiceEngineChoice),
}

#[derive(Clone, Copy, Debug)]
struct Press {
    id: u32,
    x0: f32,
    y0: f32,
    target: Option<Target>,
    dragging: bool,
}

#[derive(Clone, Copy, Debug)]
struct Slide {
    from: f32,
    start: u64,
}

pub struct OnboardingView {
    model: SettingsModel,
    theme: SettingsTheme,
    page: usize,
    w: f32,
    h: f32,
    /// Horizontal offset of the current page (finger drag or the snap animation).
    offset: f32,
    slide: Option<Slide>,
    press: Option<Press>,
    vel: VelocityTracker,
    actions: Vec<SettingsAction>,
}

const LAYOUTS: [(LayoutChoice, &str, &str); 3] = [
    (LayoutChoice::Pinyin, "全拼", "和手机一样的 26 键，打完整拼音"),
    (LayoutChoice::Shuangpin, "双拼", "每个字按两下，默认小鹤方案"),
    (LayoutChoice::T9, "九宫格", "按键大，适合单手和竖屏"),
];

impl OnboardingView {
    pub fn new(model: SettingsModel, theme: ThemeKind) -> Self {
        Self {
            model,
            theme: SettingsTheme::of(theme),
            page: 0,
            w: 760.0,
            h: 560.0,
            offset: 0.0,
            slide: None,
            press: None,
            vel: VelocityTracker::default(),
            actions: Vec::new(),
        }
    }

    pub fn set_model(&mut self, model: SettingsModel) -> bool {
        let changed = model != self.model;
        self.model = model;
        changed
    }

    pub fn set_theme(&mut self, kind: ThemeKind) -> bool {
        let changed = self.theme.kind != kind;
        self.theme = SettingsTheme::of(kind);
        changed
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn set_page(&mut self, page: usize) -> bool {
        let page = page.min(ONBOARDING_PAGES - 1);
        let changed = page != self.page;
        self.page = page;
        self.offset = 0.0;
        self.slide = None;
        changed
    }

    pub fn model(&self) -> &SettingsModel {
        &self.model
    }

    /// Centre of a named element for tests / automation: "跳过", "上一步", "下一步", "开始使用",
    /// a layout name ("九宫格") or an engine name ("系统语音").
    pub fn element_center(&self, name: &str) -> Option<(f32, f32)> {
        self.hits().into_iter().find(|(_, t)| self.target_name(*t) == name).map(|(r, _)| (r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    fn target_name(&self, t: Target) -> String {
        match t {
            Target::Skip => "跳过".into(),
            Target::Back => "上一步".into(),
            Target::Next => "下一步".into(),
            Target::Finish => "开始使用".into(),
            Target::Dot(i) => format!("dot{i}"),
            Target::Layout(l) => LAYOUTS.iter().find(|x| x.0 == l).map_or("", |x| x.1).into(),
            Target::Engine(e) => e.name().into(),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Geometry (current page, no slide offset)
    // -----------------------------------------------------------------------------------------

    fn content_h(&self) -> f32 {
        (self.h - BAR_H).max(0.0)
    }

    fn column(&self) -> (f32, f32) {
        let w = (self.w - 64.0).clamp(240.0, 720.0);
        ((self.w - w) / 2.0, w)
    }

    /// Illustration area and the title baseline area of a page.
    fn art_rect(&self) -> Rect {
        let (x, w) = self.column();
        let h = (self.content_h() * 0.36).clamp(110.0, 220.0);
        Rect::new(x, 56.0, w, h)
    }

    fn title_y(&self) -> f32 {
        let a = self.art_rect();
        a.y + a.h + 22.0
    }

    fn body_y(&self) -> f32 {
        self.title_y() + 74.0
    }

    fn layout_cards(&self) -> Vec<(Rect, LayoutChoice)> {
        let (x, w) = self.column();
        let gap = 16.0;
        let cw = (w - 2.0 * gap) / 3.0;
        let top = 40.0;
        let h = (self.content_h() - top - 24.0).clamp(160.0, 300.0);
        LAYOUTS.iter().enumerate().map(|(i, l)| (Rect::new(x + i as f32 * (cw + gap), top + 92.0, cw, h - 92.0 + 40.0), l.0)).collect()
    }

    fn engine_rows(&self) -> Vec<(Rect, VoiceEngineChoice)> {
        let (x, w) = self.column();
        let rh = 62.0;
        let y0 = 128.0;
        VoiceEngineChoice::ALL.iter().enumerate().map(|(i, &e)| (Rect::new(x, y0 + i as f32 * (rh + 10.0), w, rh), e)).collect()
    }

    fn bar_buttons(&self) -> Vec<(Rect, Target)> {
        let y = self.h - BAR_H + (BAR_H - 44.0) / 2.0;
        let (x, w) = self.column();
        let mut out = Vec::new();
        let last = self.page + 1 == ONBOARDING_PAGES;
        let main_w = 132.0;
        let main = Rect::new(x + w - main_w, y, main_w, 44.0);
        out.push((main, if last { Target::Finish } else { Target::Next }));
        if self.page > 0 {
            out.push((Rect::new(main.x - 12.0 - 104.0, y, 104.0, 44.0), Target::Back));
        }
        if !last {
            out.push((Rect::new(x - 8.0, y, 72.0, 44.0), Target::Skip));
        }
        // Dots (24 DIPs apart, each a 44 DIP target).
        let n = ONBOARDING_PAGES as f32;
        let dx = 22.0;
        let x0 = self.w / 2.0 - (n - 1.0) * dx / 2.0;
        for i in 0..ONBOARDING_PAGES {
            out.push((Rect::new(x0 + i as f32 * dx - 11.0, y, 22.0, 44.0), Target::Dot(i)));
        }
        out
    }

    fn hits(&self) -> Vec<(Rect, Target)> {
        let mut out = self.bar_buttons();
        match self.page {
            1 => out.extend(self.layout_cards().into_iter().map(|(r, l)| (r, Target::Layout(l)))),
            2 => out.extend(self.engine_rows().into_iter().map(|(r, e)| (r, Target::Engine(e)))),
            _ => {}
        }
        out
    }

    // -----------------------------------------------------------------------------------------
    // Behaviour
    // -----------------------------------------------------------------------------------------

    fn go(&mut self, page: usize, now: u64) -> Response {
        let page = page.min(ONBOARDING_PAGES - 1);
        if page == self.page {
            return self.settle(now);
        }
        // Keep the picture continuous: the new page starts where it currently is on screen.
        let shift = (page as f32 - self.page as f32) * self.w;
        self.offset += shift;
        self.page = page;
        self.settle(now)
    }

    fn settle(&mut self, now: u64) -> Response {
        if self.offset.abs() < 0.5 {
            self.offset = 0.0;
            self.slide = None;
            return Response::repaint();
        }
        self.slide = Some(Slide { from: self.offset, start: now });
        Response { repaint: true, actions: Vec::new(), timer_ms: Some(FRAME_MS) }
    }

    fn activate(&mut self, t: Target, now: u64) -> Response {
        match t {
            Target::Skip | Target::Finish => {
                self.actions.push(SettingsAction::FinishOnboarding);
                Response::repaint()
            }
            Target::Back => self.go(self.page.saturating_sub(1), now),
            Target::Next => self.go(self.page + 1, now),
            Target::Dot(i) => self.go(i, now),
            Target::Layout(l) => {
                let a = SettingsAction::SetLayout(l);
                self.model.apply(&a);
                self.actions.push(a);
                Response::repaint()
            }
            Target::Engine(e) => {
                let a = SettingsAction::SetVoiceEngine(e);
                self.model.apply(&a);
                self.actions.push(a);
                Response::repaint()
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Painting
    // -----------------------------------------------------------------------------------------

    fn pressed(&self, t: Target) -> bool {
        self.press.is_some_and(|p| !p.dragging && p.target == Some(t))
    }

    fn paint_page(&self, c: &mut dyn Canvas, i: usize, dx: f32) {
        let t = &self.theme;
        let (x, w) = self.column();
        let (x, ty) = (x + dx, self.title_y());
        let titles = [
            ("欢迎使用点墨", "指尖一点，落字成墨。为 Surface 触屏打造的中文输入法"),
            ("选一个顺手的布局", "以后在键盘上随时可以切换"),
            ("选择语音引擎", "说话就能打字。点墨调用你电脑上已有的语音输入"),
            ("几个常用手势", "记住这三个，打字快一倍"),
        ];
        let (title, sub) = titles[i];
        let compact = i != 0;
        let ty = if compact { 40.0 } else { ty };
        c.text(title, Rect::new(x, ty, w, 36.0), TextStyle { align: Align::Center, ..bold(24.0, t.text) });
        c.text(sub, Rect::new(x, ty + 40.0, w, 22.0), centered(14.0, t.text_secondary));
        match i {
            0 => self.paint_welcome(c, dx),
            1 => self.paint_layouts(c, dx),
            2 => self.paint_engines(c, dx),
            _ => self.paint_gestures(c, dx),
        }
    }

    fn paint_welcome(&self, c: &mut dyn Canvas, dx: f32) {
        let t = &self.theme;
        let a = self.art_rect();
        let (cx, cy) = (a.x + dx + a.w / 2.0, a.y + a.h / 2.0);
        for (k, al) in [(2.1, 0.35), (1.65, 0.6), (1.3, 1.0)] {
            let d = a.h * 0.55 * k;
            c.fill_rect(Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), d / 2.0, t.accent_soft.with_alpha(t.accent_soft.a * al * 0.7));
        }
        let s = a.h * 0.55;
        app_icon(c, Rect::new(cx - s / 2.0, cy - s / 2.0, s, s));
        // Feature pills.
        let feats = [("\u{E765}", "像手机一样打字"), ("\u{E720}", "说话就能输入"), ("\u{E77F}", "复制粘贴随手可得")];
        let pw: Vec<f32> = feats.iter().map(|f| text_w(f.1, 14.0) + 56.0).collect();
        let total = pw.iter().sum::<f32>() + 12.0 * 2.0;
        let mut x = self.w / 2.0 + dx - total / 2.0;
        let y = self.body_y() + 8.0;
        for ((ic, label), w) in feats.iter().zip(pw) {
            let r = Rect::new(x, y, w, 44.0);
            c.fill_rect(r, 22.0, t.card);
            c.stroke_rect(r, 22.0, 1.0, t.card_border);
            c.text(ic, Rect::new(r.x + 14.0, r.y, 22.0, r.h), icon(16.0, t.accent));
            c.text(label, Rect::new(r.x + 42.0, r.y, w - 48.0, r.h), style(14.0, t.text));
            x += w + 12.0;
        }
    }

    fn paint_layouts(&self, c: &mut dyn Canvas, dx: f32) {
        let t = &self.theme;
        for (r, l) in self.layout_cards() {
            let r = Rect::new(r.x + dx, r.y, r.w, r.h);
            let sel = self.model.layout == l;
            let (_, name, blurb) = LAYOUTS.iter().find(|x| x.0 == l).copied().unwrap();
            if t.kind == ThemeKind::Light {
                shadow(c, r, 14.0, t.shadow.with_alpha(0.06), 8.0);
            }
            c.fill_rect(r, 14.0, if self.pressed(Target::Layout(l)) { mix(t.card, t.text, 0.05) } else { t.card });
            if sel {
                c.stroke_rect(r.inset(1.0), 13.0, 2.0, t.accent);
            } else {
                c.stroke_rect(r, 14.0, 1.0, t.card_border);
            }
            let art = Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, (r.h - 104.0).max(60.0));
            mini_keyboard(c, t, art, l, sel);
            let ty = art.y + art.h + 14.0;
            c.text(name, Rect::new(r.x, ty, r.w, 24.0), TextStyle { align: Align::Center, ..bold(16.0, if sel { t.accent } else { t.text }) });
            c.text(blurb, Rect::new(r.x + 8.0, ty + 28.0, r.w - 16.0, 18.0), centered(12.0, t.text_faint));
            if sel {
                let b = Rect::new(r.x + r.w - 17.0, r.y - 7.0, 24.0, 24.0);
                c.fill_rect(b.inset(-2.0), 14.0, t.background);
                c.fill_rect(b, 12.0, t.accent);
                c.text("\u{E73E}", b, icon(12.0, t.on_accent));
            }
        }
    }

    fn paint_engines(&self, c: &mut dyn Canvas, dx: f32) {
        let t = &self.theme;
        let rec = self.model.engines.recommended();
        for (r, e) in self.engine_rows() {
            let r = Rect::new(r.x + dx, r.y, r.w, r.h);
            let sel = self.model.voice_engine == e;
            let st = self.model.engines.get(e);
            c.fill_rect(r, 12.0, if self.pressed(Target::Engine(e)) { mix(t.card, t.text, 0.05) } else { t.card });
            if sel {
                c.stroke_rect(r.inset(1.0), 11.0, 2.0, t.accent);
            } else {
                c.stroke_rect(r, 12.0, 1.0, t.card_border);
            }
            let glyph = match e {
                VoiceEngineChoice::WeType => "\u{E8BD}",
                VoiceEngineChoice::DoubaoIme => "\u{E765}",
                VoiceEngineChoice::Doubao => "\u{E720}",
                VoiceEngineChoice::System => "\u{E7F8}",
            };
            let ib = Rect::new(r.x + 18.0, r.y + (r.h - 40.0) / 2.0, 40.0, 40.0);
            c.fill_rect(ib, 10.0, if sel { t.accent_soft } else { t.fill });
            c.text(glyph, ib, icon(18.0, if sel { t.accent } else { t.text_secondary }));
            let tx = ib.x + 54.0;
            let ty = r.y + (r.h - 44.0) / 2.0;
            c.text(e.name(), Rect::new(tx, ty, 200.0, 22.0), bold(15.0, t.text));
            if e == rec && st.available {
                let x = tx + text_w(e.name(), 15.0) + 8.0;
                let tr = Rect::new(x, ty + 2.0, 34.0, 18.0);
                c.fill_rect(tr, 4.0, t.accent_soft);
                c.text("推荐", tr, centered(11.0, t.accent));
            }
            let status = if st.detail.is_empty() {
                Status::new(Level::Unknown, e.blurb())
            } else if st.available {
                Status::ok(st.detail.clone())
            } else {
                Status::warn(st.detail.clone())
            };
            status_line(c, t, &status, Rect::new(tx, ty + 26.0, r.w - (tx - r.x) - 60.0, 18.0));
            radio(c, t, Rect::new(r.x + r.w - 42.0, r.y + (r.h - 22.0) / 2.0, 22.0, 22.0), sel);
        }
        let (x, w) = self.column();
        let y = self.engine_rows().last().map_or(0.0, |(r, _)| r.y + r.h) + 14.0;
        c.text("以后可以在「设置 › 语音」里更改", Rect::new(x + dx, y, w, 18.0), centered(12.0, t.text_faint));
    }

    fn gesture_tiles(&self) -> Vec<Rect> {
        let (x, w) = self.column();
        let gap = 16.0;
        let tw = (w - 2.0 * gap) / 3.0;
        let top = 132.0;
        let h = (self.content_h() - top - 28.0).clamp(200.0, 320.0);
        (0..3).map(|i| Rect::new(x + i as f32 * (tw + gap), top, tw, h)).collect()
    }

    fn paint_gestures(&self, c: &mut dyn Canvas, dx: f32) {
        let t = &self.theme;
        let items = [
            ("长按空格", "变成触控板，滑动移动光标"),
            ("按键上滑", "输入键上角的数字和符号"),
            ("悬浮语音球", "点一下说话，长按展开键盘"),
        ];
        for (i, (r, (title, desc))) in self.gesture_tiles().into_iter().zip(items).enumerate() {
            let r = Rect::new(r.x + dx, r.y, r.w, r.h);
            if t.kind == ThemeKind::Light {
                shadow(c, r, 14.0, t.shadow.with_alpha(0.06), 8.0);
            }
            c.fill_rect(r, 14.0, t.card);
            c.stroke_rect(r, 14.0, 1.0, t.card_border);
            let art = Rect::new(r.x + 14.0, r.y + 14.0, r.w - 28.0, r.h - 92.0);
            match i {
                0 => art_trackpad(c, t, art),
                1 => art_swipe(c, t, art),
                _ => art_ball(c, t, art),
            }
            let by = art.y + art.h + 16.0;
            c.text(title, Rect::new(r.x, by, r.w, 24.0), TextStyle { align: Align::Center, ..bold(15.0, t.text) });
            c.text(desc, Rect::new(r.x + 6.0, by + 28.0, r.w - 12.0, 18.0), centered(12.0, t.text_secondary));
        }
    }

    fn paint_bar(&self, c: &mut dyn Canvas) {
        let t = &self.theme;
        for (r, target) in self.bar_buttons() {
            let st = if self.pressed(target) { BtnState::Pressed } else { BtnState::Normal };
            match target {
                Target::Next => button(c, t, r, "下一步", ButtonKind::Primary, true, st),
                Target::Finish => button(c, t, r, "开始使用", ButtonKind::Primary, true, st),
                Target::Back => button(c, t, r, "上一步", ButtonKind::Secondary, true, st),
                Target::Skip => {
                    if st == BtnState::Pressed {
                        c.fill_rect(r, 8.0, t.pressed);
                    }
                    c.text("跳过", r, centered(14.0, t.text_faint));
                }
                Target::Dot(i) => {
                    // The active dot stretches into a pill; follows the finger while sliding.
                    let pos = self.page as f32 - self.offset / self.w.max(1.0);
                    let near = (1.0 - (pos - i as f32).abs()).clamp(0.0, 1.0);
                    let dw = 8.0 + 14.0 * near;
                    let d = Rect::new(r.x + r.w / 2.0 - dw / 2.0, r.y + r.h / 2.0 - 4.0, dw, 8.0);
                    c.fill_rect(d, 4.0, mix(t.fill_strong, t.accent, near));
                }
                _ => {}
            }
        }
    }
}

impl OnboardingView {
    /// Hands the queued [`SettingsAction`]s to the host with the response.
    fn flush(&mut self, mut r: Response) -> Response {
        r.actions.extend(self.actions.drain(..).map(crate::view::UiAction::Settings));
        r
    }

    fn handle_pointer(&mut self, e: PointerEvent) -> Response {
        match e.phase {
            PointerPhase::Down => {
                if self.press.is_some() {
                    return Response::none();
                }
                self.slide = None;
                let target = if self.offset.abs() < 1.0 {
                    self.hits().into_iter().find(|(r, _)| r.contains(e.x, e.y)).map(|(_, t)| t)
                } else {
                    None
                };
                let base = self.offset;
                self.press = Some(Press { id: e.id, x0: e.x - base, y0: e.y, target, dragging: base != 0.0 });
                self.vel = VelocityTracker::default();
                self.vel.push(e.x, e.time_ms);
                Response::repaint()
            }
            PointerPhase::Move => {
                let Some(mut p) = self.press.filter(|p| p.id == e.id) else { return Response::none() };
                self.vel.push(e.x, e.time_ms);
                let dx = e.x - p.x0;
                if !p.dragging && dx.abs() > SLOP && dx.abs() > (e.y - p.y0).abs() {
                    p.dragging = true;
                    p.x0 += dx.signum() * SLOP;
                }
                self.press = Some(p);
                if p.dragging {
                    let dx = e.x - p.x0;
                    // Rubber band past the first / last card.
                    let edge = (self.page == 0 && dx > 0.0) || (self.page + 1 == ONBOARDING_PAGES && dx < 0.0);
                    self.offset = if edge { dx * 0.3 } else { dx };
                    return Response::repaint();
                }
                Response::none()
            }
            PointerPhase::Up => {
                let Some(p) = self.press.take().filter(|p| p.id == e.id) else { return Response::none() };
                if p.dragging {
                    let v = self.vel.velocity(e.time_ms);
                    let page = if (self.offset < -self.w * 0.2 || v < -0.5) && self.offset < 0.0 {
                        self.page + 1
                    } else if (self.offset > self.w * 0.2 || v > 0.5) && self.offset > 0.0 {
                        self.page.saturating_sub(1)
                    } else {
                        self.page
                    };
                    return self.go(page, e.time_ms);
                }
                match p.target {
                    Some(t) if self.hits().iter().any(|(r, tt)| *tt == t && r.contains(e.x, e.y)) => self.activate(t, e.time_ms),
                    _ => Response::repaint(),
                }
            }
            PointerPhase::Cancel => {
                if self.press.take().is_some() {
                    return self.settle(e.time_ms);
                }
                Response::none()
            }
        }
    }

    fn handle_key(&mut self, vk: u32, down: bool) -> Response {
        if !down {
            return Response::none();
        }
        // Monotonic time isn't passed with keys: jump without animation.
        match vk {
            0x27 | 0x0D if self.page + 1 < ONBOARDING_PAGES => {
                self.set_page(self.page + 1);
                Response::repaint()
            }
            0x0D => self.activate(Target::Finish, 0),
            0x25 if self.page > 0 => {
                self.set_page(self.page - 1);
                Response::repaint()
            }
            0x1B => self.activate(Target::Skip, 0),
            _ => Response::none(),
        }
    }
}

impl View for OnboardingView {
    fn resize(&mut self, width: f32, height: f32) {
        self.w = width;
        self.h = height;
    }

    fn preferred_height(&self, _width: f32) -> f32 {
        560.0
    }

    fn paint(&mut self, c: &mut dyn Canvas) {
        let t = self.theme;
        c.clear(t.background);
        c.push_clip(Rect::new(0.0, 0.0, self.w, self.content_h()));
        for i in 0..ONBOARDING_PAGES {
            let dx = (i as f32 - self.page as f32) * self.w + self.offset;
            if dx.abs() < self.w {
                self.paint_page(c, i, dx);
            }
        }
        c.pop_clip();
        self.paint_bar(c);
    }

    fn pointer(&mut self, e: PointerEvent) -> Response {
        let r = self.handle_pointer(e);
        self.flush(r)
    }

    fn timer(&mut self, now_ms: u64) -> Response {
        let Some(s) = self.slide else { return Response::none() };
        let f = (now_ms.saturating_sub(s.start) as f32 / SLIDE_MS as f32).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - f).powi(3);
        self.offset = s.from * (1.0 - e);
        if f >= 1.0 {
            self.offset = 0.0;
            self.slide = None;
            return Response::repaint();
        }
        Response { repaint: true, actions: Vec::new(), timer_ms: Some(FRAME_MS) }
    }

    fn key(&mut self, vk: u32, down: bool) -> Response {
        let r = self.handle_key(vk, down);
        self.flush(r)
    }

    fn set_input_state(&mut self, _state: InputState) -> Response {
        Response::none()
    }

    fn set_more_candidates(&mut self, _start: usize, _candidates: Vec<dianmo_core::Candidate>) -> Response {
        Response::none()
    }

    fn set_t9_spellings(&mut self, _spellings: Vec<String>) -> Response {
        Response::none()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

