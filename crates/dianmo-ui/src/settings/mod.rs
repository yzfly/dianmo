//! Settings window, about page and first-run onboarding (PRODUCT.md P3 / P4 / P5, §3).
//!
//! [`SettingsView`] implements [`View`] for the app-window host in `dianmo-win`: left
//! navigation (top tabs when the window is narrower than [`NARROW_WIDTH`]), scrollable content
//! (touch drag with momentum, mouse wheel, arrow / page keys). Every change applies at once: the
//! view updates its own copy of the model and hands a [`SettingsAction`] to the host as
//! `UiAction::Settings` in the returned `Response`. The host saves / applies it and may push the
//! authoritative state back with [`SettingsView::set_model`].
//!
//! [`OnboardingView`] is the 3–4 card first-run guide.

mod about;
mod art;
mod model;
mod onboarding;
mod pages;
mod widgets;

#[cfg(test)]
mod tests;

pub use about::{CREDITS, DEVELOPER, NAME, SLOGAN};
pub use model::*;
pub use onboarding::{OnboardingView, ONBOARDING_PAGES};

use crate::canvas::{Align, Canvas, Rect, TextStyle};
use crate::scroll::{Scroller, VelocityTracker, FRAME_MS};
use crate::theme::{SettingsTheme, ThemeKind};
use crate::view::{InputState, PointerEvent, PointerPhase, Response, View};

use widgets::{bold, icon, layout, paint_laid, shift, style, Block, Hit, Laid, LayoutCtx, PaintCtx, Target};

/// Below this window width (DIPs) the navigation becomes a row of tabs on top.
pub const NARROW_WIDTH: f32 = 720.0;

const NAV_W: f32 = 232.0;
const NAV_ITEM_H: f32 = 46.0;
const NAV_TOP: f32 = 76.0;
const HEADER_H: f32 = 72.0;
const TAB_H: f32 = 52.0;
const MAX_COL: f32 = 720.0;
/// Finger travel before a press turns into scrolling / slider dragging.
const SLOP: f32 = 8.0;
const SWITCH_MS: u64 = 160;

#[derive(Clone, Copy, Debug, PartialEq)]
enum PressTarget {
    Nav(Page),
    Hit(usize),
    /// Empty content area (only scrolls).
    Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PressMode {
    Pending,
    Scroll,
    Slider,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    id: u32,
    x0: f32,
    y0: f32,
    target: PressTarget,
    mode: PressMode,
}

#[derive(Clone, Debug)]
struct SwitchAnim {
    key: String,
    to_on: bool,
    start: u64,
    pos: f32,
}

pub struct SettingsView {
    model: SettingsModel,
    theme: SettingsTheme,
    page: Page,
    w: f32,
    h: f32,
    blocks: Vec<Block>,
    laid: Laid,
    scroll: Scroller,
    vel: VelocityTracker,
    press: Option<Press>,
    hover: Option<PressTarget>,
    confirming: Option<&'static str>,
    actions: Vec<SettingsAction>,
    anim: Option<SwitchAnim>,
    /// Slider value while dragging (row index, value): sent on release.
    slider_drag: Option<(usize, f32)>,
    /// A short message at the bottom of the content (text, hide time in ms).
    toast: Option<(String, u64)>,
}

impl SettingsView {
    pub fn new(model: SettingsModel, theme: ThemeKind) -> Self {
        let mut v = Self {
            model,
            theme: SettingsTheme::of(theme),
            page: Page::General,
            w: 960.0,
            h: 680.0,
            blocks: Vec::new(),
            laid: Laid::default(),
            scroll: Scroller::default(),
            vel: VelocityTracker::default(),
            press: None,
            hover: None,
            confirming: None,
            actions: Vec::new(),
            anim: None,
            slider_drag: None,
            toast: None,
        };
        v.relayout();
        v
    }

    pub fn model(&self) -> &SettingsModel {
        &self.model
    }

    /// Replaces the displayed state (after the host applied an action, or a status changed).
    /// Returns whether anything visible changed.
    pub fn set_model(&mut self, model: SettingsModel) -> bool {
        if model == self.model {
            return false;
        }
        self.model = model;
        self.relayout();
        true
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn set_page(&mut self, page: Page) -> bool {
        if page == self.page {
            return false;
        }
        self.page = page;
        self.confirming = None;
        self.hover = None;
        self.scroll.stop();
        self.scroll.offset = 0.0;
        self.relayout();
        true
    }

    pub fn set_theme(&mut self, kind: ThemeKind) -> bool {
        if self.theme.kind == kind {
            return false;
        }
        self.theme = SettingsTheme::of(kind);
        true
    }

    pub fn theme(&self) -> ThemeKind {
        self.theme.kind
    }

    /// Shows a short message (e.g. 「已导出到 …」, 「下个版本提供」) at the bottom of the content
    /// for 1.5–5 s depending on its length. `now_ms` is the host's monotonic clock.
    pub fn show_toast(&mut self, text: &str, now_ms: u64) -> Response {
        let ms = (1200 + 120 * text.chars().count() as u64).clamp(1500, 5000);
        self.toast = Some((text.to_string(), now_ms + ms));
        Response { repaint: true, actions: Vec::new(), timer_ms: self.next_timer(now_ms) }
    }

    /// The toast currently shown, if any.
    pub fn toast(&self) -> Option<&str> {
        self.toast.as_ref().map(|(t, _)| t.as_str())
    }

    /// When the view needs its next `timer` call: every frame while something animates, else when
    /// the toast goes away.
    fn next_timer(&self, now_ms: u64) -> Option<u64> {
        if self.scroll.animating() || self.anim.is_some() {
            Some(FRAME_MS)
        } else {
            self.toast.as_ref().map(|(_, until)| until.saturating_sub(now_ms).max(1))
        }
    }

    pub fn scroll_offset(&self) -> f32 {
        self.scroll.offset
    }

    pub fn max_scroll(&self) -> f32 {
        self.scroll.max
    }

    pub fn is_narrow(&self) -> bool {
        self.w < NARROW_WIDTH
    }

    /// Window coordinates of the centre of a named element: a page title in the navigation
    /// ("常规" … "关于"), a row key ("autostart", "theme"…), a button label ("检查更新"), or a
    /// segmented option / chip as "row/label" ("theme/深色", "fuzzy/z = zh"). For tests and GUI
    /// automation; the element may be scrolled out of view (see [`Self::scroll_into_view`]).
    pub fn element_center(&self, name: &str) -> Option<(f32, f32)> {
        if let Some(p) = Page::ALL.iter().find(|p| p.title() == name) {
            let r = self.nav_rect(*p);
            return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
        }
        let h = self.laid.hits.iter().find(|h| h.name == name)?;
        let r = shift(h.press, self.content_dy());
        let r = if r.w > 0.0 { r } else { shift(h.rect, self.content_dy()) };
        Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    /// Scrolls so the named element is visible. Returns whether it exists.
    pub fn scroll_into_view(&mut self, name: &str) -> bool {
        let Some(h) = self.laid.hits.iter().find(|h| h.name == name) else { return false };
        let vp = self.viewport();
        let top = h.rect.y - 16.0;
        let bottom = h.rect.y + h.rect.h + 16.0 - vp.h + self.top_pad();
        if self.scroll.offset > top {
            self.scroll.offset = top.max(0.0);
        } else if self.scroll.offset < bottom {
            self.scroll.offset = bottom.min(self.scroll.max);
        }
        true
    }

    // -----------------------------------------------------------------------------------------
    // Geometry
    // -----------------------------------------------------------------------------------------

    fn viewport(&self) -> Rect {
        if self.is_narrow() {
            Rect::new(0.0, TAB_H, self.w, (self.h - TAB_H).max(0.0))
        } else {
            Rect::new(NAV_W, HEADER_H, (self.w - NAV_W).max(0.0), (self.h - HEADER_H).max(0.0))
        }
    }

    fn top_pad(&self) -> f32 {
        if self.is_narrow() {
            16.0
        } else {
            4.0
        }
    }

    fn column(&self) -> (f32, f32) {
        let vp = self.viewport();
        let pad = if self.is_narrow() { 16.0 } else { 40.0 };
        let w = (vp.w - 2.0 * pad).clamp(200.0, MAX_COL);
        (vp.x + (vp.w - w) / 2.0, w)
    }

    /// Content → window y offset.
    fn content_dy(&self) -> f32 {
        self.viewport().y + self.top_pad() - self.scroll.offset
    }

    fn nav_rect(&self, p: Page) -> Rect {
        let i = Page::ALL.iter().position(|&q| q == p).unwrap_or(0);
        if self.is_narrow() {
            let n = Page::ALL.len() as f32;
            let tw = self.w / n;
            Rect::new(i as f32 * tw, 0.0, tw, TAB_H)
        } else {
            // 关于 is separated from the settings pages by a small gap.
            let gap = if p == Page::About { 17.0 } else { 0.0 };
            Rect::new(12.0, NAV_TOP + i as f32 * (NAV_ITEM_H + 4.0) + gap, NAV_W - 24.0, NAV_ITEM_H)
        }
    }

    fn relayout(&mut self) {
        self.blocks = pages::build(self.page, &self.model);
        let (x, w) = self.column();
        self.laid = layout(&self.blocks, x, w, &LayoutCtx { confirming: self.confirming });
        let vp = self.viewport();
        self.scroll.set_max(self.laid.height + self.top_pad() - vp.h);
    }

    fn target_at(&self, x: f32, y: f32) -> Option<PressTarget> {
        for p in Page::ALL {
            if self.nav_rect(p).contains(x, y) {
                return Some(PressTarget::Nav(p));
            }
        }
        let vp = self.viewport();
        if !vp.contains(x, y) {
            return None;
        }
        let cy = y - self.content_dy();
        let hit = self.laid.hits.iter().position(|h| h.rect.contains(x, cy));
        Some(hit.map_or(PressTarget::Content, PressTarget::Hit))
    }

    // -----------------------------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------------------------

    fn emit(&mut self, a: SettingsAction) {
        self.model.apply(&a);
        match a {
            SettingsAction::SetTheme(ThemeChoice::Light) => {
                self.set_theme(ThemeKind::Light);
            }
            SettingsAction::SetTheme(ThemeChoice::Dark) => {
                self.set_theme(ThemeKind::Dark);
            }
            _ => {}
        }
        self.actions.push(a);
    }

    fn activate(&mut self, hit: Hit, x: f32, now: u64) -> Response {
        let mut r = Response::repaint();
        // Any tap other than the open confirmation's own buttons closes it.
        self.confirming = match hit.target {
            Target::ConfirmOpen(id) => Some(id),
            _ => None,
        };
        match hit.target {
            Target::Act(a) => self.emit(a),
            Target::Toggle { key, act } => {
                let mut after = self.model.clone();
                after.apply(&act);
                let to_on = self.switch_value_after(&key, &after);
                self.anim = Some(SwitchAnim { key, to_on, start: now, pos: if to_on { 0.0 } else { 1.0 } });
                self.emit(act);
                r.timer_ms = Some(FRAME_MS);
            }
            Target::Slider { row } => {
                if let Some(v) = self.laid.rows.get(row).and_then(|lr| widgets::slider_value(lr, x)) {
                    self.commit_slider(row, v);
                }
            }
            Target::ConfirmOpen(_) | Target::ConfirmCancel => {}
        }
        self.relayout();
        r
    }

    fn switch_value_after(&self, key: &str, m: &SettingsModel) -> bool {
        let blocks = pages::build(self.page, m);
        for b in &blocks {
            if let Block::Group { rows, .. } = b {
                for r in rows {
                    if r.key == key
                        && let widgets::Control::Switch { on, .. } = r.control {
                            return on;
                        }
                }
            }
        }
        false
    }

    fn commit_slider(&mut self, row: usize, v: f32) {
        let Some(lr) = self.laid.rows.get(row) else { return };
        let widgets::Control::Slider(s) = &lr.row.control else { return };
        let a = (s.act)(v);
        self.emit(a);
    }

    fn preview_slider(&mut self, row: usize, v: f32) -> bool {
        // Show the value live without sending: patch the laid row.
        let Some(lr) = self.laid.rows.get_mut(row) else { return false };
        let widgets::Control::Slider(s) = &mut lr.row.control else { return false };
        if s.value == v {
            return false;
        }
        s.value = v;
        self.slider_drag = Some((row, v));
        true
    }

    fn stop_anim_if_done(&mut self, now: u64) -> bool {
        let Some(a) = &mut self.anim else { return false };
        let f = (now.saturating_sub(a.start) as f32 / SWITCH_MS as f32).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - f).powi(3);
        a.pos = if a.to_on { e } else { 1.0 - e };
        if f >= 1.0 {
            self.anim = None;
        }
        true
    }

    // -----------------------------------------------------------------------------------------
    // Painting
    // -----------------------------------------------------------------------------------------

    fn paint_nav(&self, c: &mut dyn Canvas) {
        let t = &self.theme;
        if self.is_narrow() {
            c.fill_rect(Rect::new(0.0, 0.0, self.w, TAB_H), 0.0, t.card);
            c.fill_rect(Rect::new(0.0, TAB_H - 1.0, self.w, 1.0), 0.0, t.divider);
            for p in Page::ALL {
                let r = self.nav_rect(p);
                let sel = p == self.page;
                if self.press.is_some_and(|pr| pr.target == PressTarget::Nav(p) && pr.mode == PressMode::Pending) {
                    c.fill_rect(r.inset(6.0), 8.0, t.pressed);
                }
                let col = if sel { t.accent } else { t.text_secondary };
                let st = TextStyle { bold: sel, align: Align::Center, ..style(15.0, col) };
                c.text(p.title(), Rect::new(r.x, r.y, r.w, r.h - 2.0), st);
                if sel {
                    let lw = widgets::text_w(p.title(), 15.0).max(24.0);
                    c.fill_rect(Rect::new(r.x + (r.w - lw) / 2.0, r.y + r.h - 4.0, lw, 3.0), 1.5, t.accent);
                }
            }
            return;
        }
        c.fill_rect(Rect::new(0.0, 0.0, NAV_W, self.h), 0.0, t.sidebar);
        c.fill_rect(Rect::new(NAV_W - 1.0, 0.0, 1.0, self.h), 0.0, t.divider);
        // Brand.
        about::app_icon(c, Rect::new(24.0, 22.0, 32.0, 32.0));
        c.text("点墨", Rect::new(66.0, 20.0, 100.0, 22.0), bold(16.0, t.text));
        c.text("设置", Rect::new(66.0, 40.0, 100.0, 18.0), style(12.0, t.text_faint));
        for p in Page::ALL {
            let r = self.nav_rect(p);
            let sel = p == self.page;
            if p == Page::About {
                c.fill_rect(Rect::new(r.x + 8.0, r.y - 11.0, r.w - 16.0, 1.0), 0.0, t.divider);
            }
            let pressed = self.press.is_some_and(|pr| pr.target == PressTarget::Nav(p) && pr.mode == PressMode::Pending);
            if sel {
                c.fill_rect(r, 8.0, t.accent_soft);
            } else if pressed {
                c.fill_rect(r, 8.0, t.pressed);
            } else if self.hover == Some(PressTarget::Nav(p)) {
                c.fill_rect(r, 8.0, t.hover);
            }
            let col = if sel { t.accent } else { t.text_secondary };
            c.text(p.icon(), Rect::new(r.x + 12.0, r.y, 24.0, r.h), icon(18.0, col));
            let st = TextStyle { bold: sel, ..style(15.0, if sel { t.accent } else { t.text }) };
            c.text(p.title(), Rect::new(r.x + 46.0, r.y, r.w - 52.0, r.h), st);
            if p == Page::About && matches!(self.model.update, UpdateState::Available { .. }) {
                c.fill_rect(Rect::new(r.x + r.w - 22.0, r.y + r.h / 2.0 - 4.0, 8.0, 8.0), 4.0, t.danger);
            }
        }
        if !self.model.version.is_empty() {
            let r = Rect::new(24.0, self.h - 40.0, NAV_W - 48.0, 20.0);
            c.text(&format!("版本 {}", self.model.version), r, style(12.0, t.text_faint));
        }
    }

    fn paint_header(&self, c: &mut dyn Canvas) {
        if self.is_narrow() {
            return;
        }
        let t = &self.theme;
        let vp = self.viewport();
        c.fill_rect(Rect::new(vp.x, 0.0, vp.w, HEADER_H), 0.0, t.background);
        let (x, w) = self.column();
        c.text(self.page.title(), Rect::new(x + 4.0, 24.0, w, 34.0), bold(widgets::PAGE_TITLE, t.text));
        if self.scroll.offset > 0.5 {
            c.fill_rect(Rect::new(vp.x, HEADER_H - 1.0, vp.w, 1.0), 0.0, t.divider);
        }
    }

    fn paint_toast(&self, c: &mut dyn Canvas) {
        let Some((text, _)) = &self.toast else { return };
        let t = &self.theme;
        let vp = self.viewport();
        let max_w = (vp.w - 48.0).max(120.0);
        let lines = widgets::wrap(text, 14.0, max_w - 40.0);
        let w = lines.iter().map(|l| widgets::text_w(l, 14.0)).fold(0.0f32, f32::max) + 40.0;
        let h = lines.len() as f32 * 21.0 + 20.0;
        let r = Rect::new(vp.x + (vp.w - w) / 2.0, vp.y + vp.h - h - 28.0, w, h);
        let (bg, fg) = if t.kind == ThemeKind::Dark { (t.fill_strong, t.text) } else { (t.text.with_alpha(0.88), t.card) };
        widgets::shadow(c, r, h.min(40.0) / 2.0, t.shadow.with_alpha(0.12), 6.0);
        c.fill_rect(r, (h / 2.0).min(20.0), bg);
        for (i, l) in lines.iter().enumerate() {
            c.text(l, Rect::new(r.x, r.y + 10.0 + i as f32 * 21.0, r.w, 21.0), TextStyle { align: Align::Center, ..style(14.0, fg) });
        }
    }

    fn paint_scrollbar(&self, c: &mut dyn Canvas) {
        let scrolling = self.scroll.animating() || self.press.is_some_and(|p| p.mode == PressMode::Scroll);
        if !scrolling || self.scroll.max <= 0.0 {
            return;
        }
        let vp = self.viewport();
        let total = self.laid.height + self.top_pad();
        let bh = (vp.h * vp.h / total).max(32.0);
        let by = vp.y + (vp.h - bh) * (self.scroll.offset / self.scroll.max);
        c.fill_rect(Rect::new(vp.x + vp.w - 7.0, by, 4.0, bh), 2.0, self.theme.text_faint.with_alpha(0.5));
    }
}

impl SettingsView {
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
                let was_flinging = self.scroll.stop();
                let Some(target) = self.target_at(e.x, e.y) else { return Response::none() };
                // A tap that stops a fling only stops it.
                let target = if was_flinging && matches!(target, PressTarget::Hit(_)) { PressTarget::Content } else { target };
                self.press = Some(Press { id: e.id, x0: e.x, y0: e.y, target, mode: PressMode::Pending });
                self.vel = VelocityTracker::default();
                self.vel.push(e.y, e.time_ms);
                Response::repaint()
            }
            PointerPhase::Move => {
                let Some(mut p) = self.press.filter(|p| p.id == e.id) else { return Response::none() };
                self.vel.push(e.y, e.time_ms);
                let (dx, dy) = (e.x - p.x0, e.y - p.y0);
                let mut repaint = false;
                if p.mode == PressMode::Pending && (dx.abs() > SLOP || dy.abs() > SLOP) {
                    let slider = matches!(p.target, PressTarget::Hit(i) if matches!(self.laid.hits[i].target, Target::Slider { .. }));
                    let in_content = !matches!(p.target, PressTarget::Nav(_)) || self.is_narrow();
                    if slider && dx.abs() >= dy.abs() {
                        p.mode = PressMode::Slider;
                    } else if dy.abs() > SLOP && in_content && !matches!(p.target, PressTarget::Nav(_)) {
                        p.mode = PressMode::Scroll;
                        self.scroll.begin_drag();
                        self.hover = None;
                    } else if !slider {
                        // Sideways drift off a button: cancel the tap.
                        p.mode = PressMode::Scroll;
                        self.scroll.begin_drag();
                    }
                    repaint = true;
                }
                match p.mode {
                    PressMode::Scroll => repaint |= self.scroll.drag_to(dy),
                    PressMode::Slider => {
                        if let PressTarget::Hit(i) = p.target
                            && let Target::Slider { row } = self.laid.hits[i].target
                                && let Some(v) = self.laid.rows.get(row).and_then(|lr| widgets::slider_value(lr, e.x)) {
                                    repaint |= self.preview_slider(row, v);
                                }
                    }
                    PressMode::Pending => {}
                }
                self.press = Some(p);
                if repaint { Response::repaint() } else { Response::none() }
            }
            PointerPhase::Up => {
                let Some(p) = self.press.filter(|p| p.id == e.id) else { return Response::none() };
                self.press = None;
                match p.mode {
                    PressMode::Scroll => {
                        self.scroll.fling(self.vel.velocity(e.time_ms), e.time_ms);
                        let mut r = Response::repaint();
                        if self.scroll.animating() {
                            r.timer_ms = Some(FRAME_MS);
                        }
                        r
                    }
                    PressMode::Slider => {
                        if let Some((row, v)) = self.slider_drag.take() {
                            self.commit_slider(row, v);
                            self.relayout();
                        }
                        Response::repaint()
                    }
                    PressMode::Pending => match p.target {
                        PressTarget::Nav(page) => {
                            self.set_page(page);
                            Response::repaint()
                        }
                        PressTarget::Hit(i) => {
                            let hit = self.laid.hits[i].clone();
                            self.activate(hit, e.x, e.time_ms)
                        }
                        PressTarget::Content => {
                            if self.confirming.take().is_some() {
                                self.relayout();
                            }
                            Response::repaint()
                        }
                    },
                }
            }
            PointerPhase::Cancel => {
                if self.press.is_some_and(|p| p.id == e.id) {
                    self.press = None;
                    if let Some((_, _)) = self.slider_drag.take() {
                        self.relayout();
                    }
                    return Response::repaint();
                }
                Response::none()
            }
        }
    }

    fn handle_key(&mut self, vk: u32, down: bool) -> Response {
        if !down {
            return Response::none();
        }
        let page_h = (self.viewport().h - 60.0).max(40.0);
        let delta = match vk {
            0x1B => {
                if self.confirming.take().is_some() {
                    self.relayout();
                } else {
                    self.actions.push(SettingsAction::Close);
                }
                return Response::repaint();
            }
            0x26 => -48.0,
            0x28 => 48.0,
            0x21 => -page_h,
            0x22 | 0x20 => page_h,
            0x24 => -1e9,
            0x23 => 1e9,
            // Tab: next page.
            0x09 => {
                let i = Page::ALL.iter().position(|&p| p == self.page).unwrap_or(0);
                self.set_page(Page::ALL[(i + 1) % Page::ALL.len()]);
                return Response::repaint();
            }
            _ => return Response::none(),
        };
        self.scroll.stop();
        let old = self.scroll.offset;
        self.scroll.offset = (old + delta).clamp(0.0, self.scroll.max);
        if self.scroll.offset != old { Response::repaint() } else { Response::none() }
    }
}

impl View for SettingsView {
    fn resize(&mut self, width: f32, height: f32) {
        if (width, height) != (self.w, self.h) {
            self.w = width;
            self.h = height;
            self.relayout();
        }
    }

    fn preferred_height(&self, _width: f32) -> f32 {
        680.0
    }

    fn paint(&mut self, c: &mut dyn Canvas) {
        let t = self.theme;
        c.clear(t.background);
        let vp = self.viewport();
        c.push_clip(vp);
        let pressed = match self.press {
            Some(Press { target: PressTarget::Hit(i), mode: PressMode::Pending, .. }) => self.laid.hits.get(i),
            _ => None,
        };
        let hovered = match self.hover {
            Some(PressTarget::Hit(i)) if self.press.is_none() => self.laid.hits.get(i),
            _ => None,
        };
        let anim = self.anim.as_ref().map(|a| (a.key.as_str(), a.pos));
        let ctx = PaintCtx { t: &t, dy: self.content_dy(), pressed, hovered, switch_anim: anim };
        paint_laid(c, &self.laid, &ctx, &|c, h, r| about::paint_hero(c, &t, h, r));
        c.pop_clip();
        self.paint_header(c);
        self.paint_nav(c);
        self.paint_scrollbar(c);
        self.paint_toast(c);
    }

    fn pointer(&mut self, e: PointerEvent) -> Response {
        let r = self.handle_pointer(e);
        self.flush(r)
    }

    fn timer(&mut self, now_ms: u64) -> Response {
        let mut r = Response::none();
        if self.scroll.step(now_ms) {
            r.repaint = true;
        }
        if self.stop_anim_if_done(now_ms) {
            r.repaint = true;
        }
        if self.toast.as_ref().is_some_and(|(_, until)| now_ms >= *until) {
            self.toast = None;
            r.repaint = true;
        }
        r.timer_ms = self.next_timer(now_ms);
        r
    }

    fn wheel(&mut self, x: f32, y: f32, delta_y: f32) -> Response {
        if !self.viewport().contains(x, y) {
            return Response::none();
        }
        self.scroll.stop();
        let old = self.scroll.offset;
        self.scroll.offset = (old + delta_y).clamp(0.0, self.scroll.max);
        if self.scroll.offset != old { Response::repaint() } else { Response::none() }
    }

    fn key(&mut self, vk: u32, down: bool) -> Response {
        let r = self.handle_key(vk, down);
        self.flush(r)
    }

    fn hover(&mut self, x: f32, y: f32) -> Response {
        let t = if x < 0.0 || y < 0.0 {
            None
        } else {
            match self.target_at(x, y) {
                Some(PressTarget::Content) => None,
                // Only highlight things that react (rows with a tap, buttons…).
                other => other,
            }
        };
        if t != self.hover {
            self.hover = t;
            return Response::repaint();
        }
        Response::none()
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
