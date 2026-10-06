//! Dev-only software renderer: draws `KeyboardView` scenes to PNGs for design iteration on a
//! machine without Windows. Text uses Noto Sans CJK (fallback DejaVu Sans); Segoe MDL2 icons
//! are approximated with simple vector shapes.
//!
//! cargo run -p dianmo-ui --example preview -- <out_dir>

use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont as _};
use dianmo_core::{Action, Candidate, Schema, Snapshot};
use dianmo_ui::settings::{
    EngineStatus, LayoutChoice, Level, OnboardingView, Page, SettingsModel, SettingsView, Status, UpdateState,
    VoiceEngines,
};
use dianmo_ui::{
    Align, Canvas, ClipItem, Color, Font, InputState, KeyboardConfig, KeyboardView, PointerEvent, PointerPhase, Rect,
    TextStyle, ThemeKind, UiAction, View,
};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, Stroke, Transform};

const SCALE: f32 = 2.0;

struct Fonts {
    regular: FontVec,
    bold: FontVec,
    fallback: FontVec,
}

fn load(path: &str, index: u32) -> FontVec {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    FontVec::try_from_vec_and_index(data, index).expect("font")
}

impl Fonts {
    fn new() -> Self {
        let dir = "/usr/share/fonts/opentype/noto";
        Self {
            // Index 2 = "Noto Sans CJK SC" in the collection.
            regular: load(&format!("{dir}/NotoSansCJK-Regular.ttc"), 2),
            bold: load(&format!("{dir}/NotoSansCJK-Bold.ttc"), 2),
            fallback: load("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
        }
    }
}

struct SkiaCanvas<'a> {
    pm: Pixmap,
    fonts: &'a Fonts,
    clips: Vec<Rect>,
    mask: Option<Mask>,
    /// Text size multiplier: 1.0 = ab_glyph's PxScale (line height = size, the keyboard scenes
    /// were tuned with it); `em_exact()` makes 1 em = size like DirectWrite (settings scenes).
    em: f32,
}

fn color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::BLACK)
}

fn paint_of(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c));
    p.anti_alias = true;
    p
}

fn rounded(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x * SCALE, r.y * SCALE, r.w * SCALE, r.h * SCALE);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rad = (radius * SCALE).min(w / 2.0).min(h / 2.0);
    if rad <= 0.1 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?));
    }
    let k = 0.5523 * rad;
    let mut pb = PathBuilder::new();
    pb.move_to(x + rad, y);
    pb.line_to(x + w - rad, y);
    pb.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
    pb.line_to(x + w, y + h - rad);
    pb.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
    pb.line_to(x + rad, y + h);
    pb.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
    pb.line_to(x, y + rad);
    pb.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
    pb.close();
    pb.finish()
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

impl<'a> SkiaCanvas<'a> {
    fn new(w: f32, h: f32, fonts: &'a Fonts) -> Self {
        let pm = Pixmap::new((w * SCALE).ceil() as u32, (h * SCALE).ceil() as u32).unwrap();
        Self { pm, fonts, clips: Vec::new(), mask: None, em: 1.0 }
    }

    fn clip_rect(&self) -> Option<Rect> {
        self.clips.last().copied()
    }

    fn rebuild_mask(&mut self) {
        self.mask = self.clip_rect().map(|r| {
            let mut m = Mask::new(self.pm.width(), self.pm.height()).unwrap();
            if let Some(p) = rounded(r, 0.0) {
                m.fill_path(&p, FillRule::Winding, false, Transform::identity());
            }
            m
        });
    }

    fn fill_path(&mut self, path: &tiny_skia::Path, c: Color) {
        self.pm.fill_path(path, &paint_of(c), FillRule::Winding, Transform::identity(), self.mask.as_ref());
    }

    fn stroke_path(&mut self, path: &tiny_skia::Path, width: f32, c: Color) {
        let stroke = Stroke { width: width * SCALE, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        self.pm.stroke_path(path, &paint_of(c), &stroke, Transform::identity(), self.mask.as_ref());
    }

    fn font_for(&self, ch: char, bold: bool) -> &FontVec {
        let main = if bold { &self.fonts.bold } else { &self.fonts.regular };
        if main.glyph_id(ch).0 != 0 { main } else { &self.fonts.fallback }
    }

    fn em_exact(mut self) -> Self {
        let f = &self.fonts.regular;
        self.em = f.height_unscaled() / f.units_per_em().unwrap_or(1000.0);
        self
    }

    fn text_width(&self, text: &str, style: &TextStyle) -> f32 {
        let px = style.size * SCALE * self.em;
        text.chars()
            .map(|ch| {
                let f = self.font_for(ch, style.bold);
                f.as_scaled(PxScale::from(px * f_scale(f))).h_advance(f.glyph_id(ch))
            })
            .sum::<f32>()
            / SCALE
    }

    fn blend(&mut self, x: i32, y: i32, cov: f32, c: Color) {
        if x < 0 || y < 0 || x >= self.pm.width() as i32 || y >= self.pm.height() as i32 {
            return;
        }
        if let Some(clip) = self.clip_rect() {
            let (fx, fy) = (x as f32 / SCALE, y as f32 / SCALE);
            if !clip.contains(fx, fy) {
                return;
            }
        }
        let a = (cov * c.a).clamp(0.0, 1.0);
        let idx = (y as usize * self.pm.width() as usize + x as usize) * 4;
        let data = self.pm.data_mut();
        // Premultiplied RGBA.
        for (i, v) in [c.r, c.g, c.b].iter().enumerate() {
            let d = data[idx + i] as f32 / 255.0;
            data[idx + i] = ((v * a + d * (1.0 - a)) * 255.0).round() as u8;
        }
        let da = data[idx + 3] as f32 / 255.0;
        data[idx + 3] = ((a + da * (1.0 - a)) * 255.0).round() as u8;
    }

    fn draw_icon(&mut self, glyph: &str, rect: Rect, style: TextStyle) {
        let s = style.size;
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let lw = (s * 0.075).max(1.2);
        let p = |x: f32, y: f32| ((cx + x * s) * SCALE, (cy + y * s) * SCALE);
        let mut pb = PathBuilder::new();
        let line = |pb: &mut PathBuilder, pts: &[(f32, f32)]| {
            let (x, y) = p(pts[0].0, pts[0].1);
            pb.move_to(x, y);
            for &(px, py) in &pts[1..] {
                let (x, y) = p(px, py);
                pb.line_to(x, y);
            }
        };
        match glyph {
            "\u{E720}" => {
                // Microphone.
                if let Some(path) = rounded(Rect::new(cx - 0.17 * s, cy - 0.45 * s, 0.34 * s, 0.58 * s), 0.17 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
                let (x0, y0) = p(-0.3, -0.05);
                pb.move_to(x0, y0);
                let (c1x, c1y) = p(-0.3, 0.3);
                let (mx, my) = p(0.0, 0.3);
                pb.quad_to(c1x, c1y, mx, my);
                let (c2x, c2y) = p(0.3, 0.3);
                let (x1, y1) = p(0.3, -0.05);
                pb.quad_to(c2x, c2y, x1, y1);
                line(&mut pb, &[(0.0, 0.3), (0.0, 0.45)]);
                line(&mut pb, &[(-0.18, 0.45), (0.18, 0.45)]);
            }
            "\u{E765}" => {
                if let Some(path) = rounded(Rect::new(cx - 0.5 * s, cy - 0.32 * s, s, 0.64 * s), 0.08 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
                for row in 0..2 {
                    for col in 0..5 {
                        let x = -0.32 + col as f32 * 0.16;
                        let y = -0.15 + row as f32 * 0.15;
                        line(&mut pb, &[(x, y), (x + 0.001, y)]);
                    }
                }
                line(&mut pb, &[(-0.2, 0.16), (0.2, 0.16)]);
            }
            "\u{E72B}" => {
                line(&mut pb, &[(0.4, 0.0), (-0.4, 0.0)]);
                line(&mut pb, &[(-0.1, -0.3), (-0.4, 0.0), (-0.1, 0.3)]);
            }
            "\u{E70D}" => line(&mut pb, &[(-0.35, -0.15), (0.0, 0.2), (0.35, -0.15)]),
            "\u{E76B}" => line(&mut pb, &[(0.15, -0.35), (-0.2, 0.0), (0.15, 0.35)]),
            "\u{E76C}" => line(&mut pb, &[(-0.15, -0.35), (0.2, 0.0), (-0.15, 0.35)]),
            "\u{E7A7}" | "\u{E7A6}" => {
                // Undo / redo: a hooked arrow.
                let m = if glyph == "\u{E7A7}" { 1.0 } else { -1.0 };
                let (x0, y0) = p(-0.3 * m, -0.1);
                pb.move_to(x0, y0);
                let (c1, c2) = p(0.45 * m, -0.25);
                let (x1, y1) = p(0.25 * m, 0.35);
                pb.quad_to(c1, c2, x1, y1);
                line(&mut pb, &[(-0.1 * m, -0.32), (-0.3 * m, -0.1), (-0.08 * m, 0.08)]);
            }
            "\u{E8B3}" => {
                for (a, b) in [((-0.4, -0.35), (0.4, -0.35)), ((0.4, -0.35), (0.4, 0.35)), ((0.4, 0.35), (-0.4, 0.35)), ((-0.4, 0.35), (-0.4, -0.35))] {
                    for i in 0..4 {
                        let t0 = i as f32 / 4.0;
                        let t1 = t0 + 0.14;
                        line(&mut pb, &[(a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0), (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1)]);
                    }
                }
            }
            "\u{E8C6}" => {
                for cx0 in [-0.2, 0.2] {
                    let (x, y) = p(cx0, 0.25);
                    pb.push_circle(x, y, 0.13 * s * SCALE);
                }
                line(&mut pb, &[(-0.1, 0.15), (0.2, -0.45)]);
                line(&mut pb, &[(0.1, 0.15), (-0.2, -0.45)]);
            }
            "\u{E8C8}" => {
                line(&mut pb, &[(-0.35, 0.25), (-0.35, -0.4), (0.2, -0.4)]);
                if let Some(path) = rounded(Rect::new(cx - 0.18 * s, cy - 0.25 * s, 0.52 * s, 0.65 * s), 0.05 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
            }
            "\u{E77F}" => {
                if let Some(path) = rounded(Rect::new(cx - 0.35 * s, cy - 0.32 * s, 0.7 * s, 0.75 * s), 0.06 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
                line(&mut pb, &[(-0.15, -0.42), (0.15, -0.42), (0.15, -0.28), (-0.15, -0.28), (-0.15, -0.42)]);
            }
            "\u{E70E}" => line(&mut pb, &[(-0.35, 0.15), (0.0, -0.2), (0.35, 0.15)]),
            "\u{E711}" => {
                line(&mut pb, &[(-0.3, -0.3), (0.3, 0.3)]);
                line(&mut pb, &[(0.3, -0.3), (-0.3, 0.3)]);
            }
            "\u{E762}" => {
                // Multi-select: ticked boxes.
                for y in [-0.25, 0.2] {
                    if let Some(path) = rounded(Rect::new(cx - 0.4 * s, cy + (y - 0.12) * s, 0.24 * s, 0.24 * s), 0.03 * s) {
                        self.stroke_path(&path, lw, style.color);
                    }
                    line(&mut pb, &[(-0.02, y), (0.42, y)]);
                }
            }
            "\u{E81C}" => {
                // History: a clock.
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.4 * s * SCALE);
                line(&mut pb, &[(0.0, -0.22), (0.0, 0.0), (0.16, 0.12)]);
            }
            "\u{E718}" => {
                line(&mut pb, &[(-0.2, -0.4), (0.2, -0.4), (0.12, -0.05), (0.3, 0.1), (-0.3, 0.1), (-0.12, -0.05), (-0.2, -0.4)]);
                line(&mut pb, &[(0.0, 0.1), (0.0, 0.45)]);
            }
            "\u{E76E}" => {
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.42 * s * SCALE);
                line(&mut pb, &[(-0.15, -0.1), (-0.149, -0.1)]);
                line(&mut pb, &[(0.15, -0.1), (0.151, -0.1)]);
                let (x0, y0) = p(-0.2, 0.1);
                pb.move_to(x0, y0);
                let (c1, c2) = p(0.0, 0.32);
                let (x1, y1) = p(0.2, 0.1);
                pb.quad_to(c1, c2, x1, y1);
            }
            "\u{E750}" => {
                line(&mut pb, &[(-0.2, -0.3), (0.5, -0.3), (0.5, 0.3), (-0.2, 0.3), (-0.5, 0.0), (-0.2, -0.3)]);
                line(&mut pb, &[(0.0, -0.12), (0.24, 0.12)]);
                line(&mut pb, &[(0.24, -0.12), (0.0, 0.12)]);
            }
            "\u{E751}" => {
                line(&mut pb, &[(0.4, -0.35), (0.4, 0.1), (-0.4, 0.1)]);
                line(&mut pb, &[(-0.15, -0.15), (-0.4, 0.1), (-0.15, 0.35)]);
            }
            "\u{E752}" => {
                line(
                    &mut pb,
                    &[(0.0, -0.42), (0.42, 0.02), (0.18, 0.02), (0.18, 0.38), (-0.18, 0.38), (-0.18, 0.02), (-0.42, 0.02), (0.0, -0.42)],
                );
            }
            // ---- Settings window glyphs (approximations).
            "\u{E713}" => {
                // Gear: circle with eight teeth.
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.28 * s * SCALE);
                pb.push_circle(x, y, 0.1 * s * SCALE);
                for i in 0..8 {
                    let a = i as f32 * std::f32::consts::PI / 4.0;
                    line(&mut pb, &[(0.28 * a.cos(), 0.28 * a.sin()), (0.42 * a.cos(), 0.42 * a.sin())]);
                }
            }
            "\u{E8D2}" => {
                // Font: a capital A with a short underline.
                line(&mut pb, &[(-0.32, 0.35), (0.0, -0.4), (0.32, 0.35)]);
                line(&mut pb, &[(-0.18, 0.08), (0.18, 0.08)]);
            }
            "\u{E946}" => {
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.42 * s * SCALE);
                line(&mut pb, &[(0.0, -0.05), (0.0, 0.22)]);
                line(&mut pb, &[(0.0, -0.2), (0.0, -0.19)]);
            }
            "\u{E73E}" => line(&mut pb, &[(-0.35, 0.0), (-0.1, 0.25), (0.38, -0.25)]),
            "\u{E8A7}" => {
                // Open in new window: box with an arrow out of the corner.
                line(&mut pb, &[(0.05, -0.35), (-0.35, -0.35), (-0.35, 0.35), (0.35, 0.35), (0.35, -0.05)]);
                line(&mut pb, &[(-0.02, 0.02), (0.38, -0.38)]);
                line(&mut pb, &[(0.12, -0.38), (0.38, -0.38), (0.38, -0.12)]);
            }
            "\u{E77B}" => {
                // Contact: head and shoulders.
                let (x, y) = p(0.0, -0.15);
                pb.push_circle(x, y, 0.18 * s * SCALE);
                let (x0, y0) = p(-0.36, 0.42);
                pb.move_to(x0, y0);
                let (c1, c2) = p(0.0, -0.1);
                let (x1, y1) = p(0.36, 0.42);
                pb.quad_to(c1, c2, x1, y1);
            }
            "\u{E774}" => {
                // Globe.
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.42 * s * SCALE);
                line(&mut pb, &[(-0.42, 0.0), (0.42, 0.0)]);
                line(&mut pb, &[(0.0, -0.42), (0.0, 0.42)]);
                if let Some(path) = rounded(Rect::new(cx - 0.2 * s, cy - 0.42 * s, 0.4 * s, 0.84 * s), 0.2 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
            }
            "\u{E8A5}" => {
                // Document.
                line(&mut pb, &[(-0.3, -0.42), (0.12, -0.42), (0.3, -0.24), (0.3, 0.42), (-0.3, 0.42), (-0.3, -0.42)]);
                line(&mut pb, &[(-0.15, -0.05), (0.15, -0.05)]);
                line(&mut pb, &[(-0.15, 0.15), (0.15, 0.15)]);
            }
            "\u{ED15}" => {
                // Feedback: speech bubble.
                line(&mut pb, &[(-0.4, -0.35), (0.4, -0.35), (0.4, 0.2), (-0.05, 0.2), (-0.25, 0.4), (-0.25, 0.2), (-0.4, 0.2), (-0.4, -0.35)]);
            }
            "\u{E9D9}" => {
                // Diagnostic: pulse line.
                line(&mut pb, &[(-0.45, 0.0), (-0.2, 0.0), (-0.08, -0.3), (0.08, 0.3), (0.2, 0.0), (0.45, 0.0)]);
            }
            "\u{E838}" => {
                // Folder.
                line(&mut pb, &[(-0.42, -0.3), (-0.1, -0.3), (0.0, -0.18), (0.42, -0.18), (0.42, 0.34), (-0.42, 0.34), (-0.42, -0.3)]);
            }
            "\u{E7BE}" => {
                // Education: mortarboard.
                line(&mut pb, &[(-0.45, -0.1), (0.0, -0.32), (0.45, -0.1), (0.0, 0.12), (-0.45, -0.1)]);
                line(&mut pb, &[(-0.25, 0.0), (-0.25, 0.25), (0.25, 0.25), (0.25, 0.0)]);
            }
            "\u{E8BD}" => {
                // Message: rounded bubble.
                if let Some(path) = rounded(Rect::new(cx - 0.42 * s, cy - 0.34 * s, 0.84 * s, 0.6 * s), 0.18 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
                line(&mut pb, &[(-0.2, 0.26), (-0.3, 0.42), (0.0, 0.26)]);
            }
            "\u{E7F8}" => {
                // Device: laptop.
                if let Some(path) = rounded(Rect::new(cx - 0.34 * s, cy - 0.32 * s, 0.68 * s, 0.48 * s), 0.05 * s) {
                    self.stroke_path(&path, lw, style.color);
                }
                line(&mut pb, &[(-0.45, 0.3), (0.45, 0.3)]);
            }
            _ => {
                let (x, y) = p(0.0, 0.0);
                pb.push_circle(x, y, 0.3 * s * SCALE);
            }
        }
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, lw, style.color);
        }
    }
}

/// Noto CJK's em box is large relative to Microsoft YaHei; scale to roughly match.
fn f_scale(_f: &FontVec) -> f32 {
    1.0
}

impl Canvas for SkiaCanvas<'_> {
    fn clear(&mut self, c: Color) {
        self.pm.fill(color(c));
    }

    fn fill_rect(&mut self, rect: Rect, radius: f32, c: Color) {
        if let Some(p) = rounded(rect, radius) {
            self.fill_path(&p, c);
        }
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, c: Color) {
        if let Some(p) = rounded(rect, radius) {
            self.stroke_path(&p, width, c);
        }
    }

    fn text(&mut self, text: &str, rect: Rect, style: TextStyle) {
        if style.font == Font::Icon {
            self.clips.push(self.clip_rect().map_or(rect, |c| intersect(c, rect)));
            self.draw_icon(text, rect, style);
            self.clips.pop();
            return;
        }
        let width = self.text_width(text, &style);
        let x0 = match style.align {
            Align::Start => rect.x,
            Align::Center => rect.x + (rect.w - width) / 2.0,
            Align::End => rect.x + rect.w - width,
        };
        let px = style.size * SCALE * self.em;
        let main = if style.bold { &self.fonts.bold } else { &self.fonts.regular };
        let sf = main.as_scaled(PxScale::from(px));
        // Centre the line box like DirectWrite (ascent + descent), using YaHei-like metrics.
        let (asc, desc) = (sf.ascent() * 0.92, -sf.descent() * 0.8);
        let baseline = rect.y * SCALE + (rect.h * SCALE - (asc + desc)) / 2.0 + asc;
        let clip = self.clip_rect().map_or(rect, |c| intersect(c, rect));
        self.clips.push(clip);
        let mut x = x0 * SCALE;
        let mut outlines = Vec::new();
        for ch in text.chars() {
            let f = self.font_for(ch, style.bold);
            let scaled = f.as_scaled(PxScale::from(px));
            let id = f.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(x, baseline));
            x += scaled.h_advance(id);
            if let Some(o) = f.outline_glyph(g) {
                outlines.push(o);
            } else if !ch.is_whitespace() && id.0 == 0 {
                // Missing glyph (emoji): draw a placeholder dot.
                let r = px * 0.3;
                let cx = x - scaled.h_advance(id) / 2.0;
                let mut pb = PathBuilder::new();
                pb.push_circle(cx, baseline - px * 0.35, r);
                if let Some(path) = pb.finish() {
                    let c = Color { a: 0.6, ..Color::rgb(250, 190, 40) };
                    self.pm.fill_path(&path, &paint_of(c), FillRule::Winding, Transform::identity(), None);
                }
            }
        }
        for o in outlines {
            let b = o.px_bounds();
            o.draw(|gx, gy, cov| self.blend(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, cov, style.color));
        }
        self.clips.pop();
    }

    fn measure_text(&mut self, text: &str, style: TextStyle) -> f32 {
        self.text_width(text, &style)
    }

    fn push_clip(&mut self, rect: Rect) {
        let r = self.clip_rect().map_or(rect, |c| intersect(c, rect));
        self.clips.push(r);
        self.rebuild_mask();
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
        self.rebuild_mask();
    }

    fn image(&mut self, name: &str, rect: Rect) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/res").join(format!("{name}.png"));
        let Ok(img) = Pixmap::load_png(&path) else { return };
        let k = (rect.w / img.width() as f32).min(rect.h / img.height() as f32) * SCALE;
        let (w, h) = (img.width() as f32 * k, img.height() as f32 * k);
        let x = rect.x * SCALE + (rect.w * SCALE - w) / 2.0;
        let y = rect.y * SCALE + (rect.h * SCALE - h) / 2.0;
        let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
        let mask = self.mask.clone();
        self.pm.draw_pixmap(0, 0, img.as_ref(), &paint, Transform::from_row(k, 0.0, 0.0, k, x, y), mask.as_ref());
    }
}

// ---------------------------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------------------------

fn cands(words: &[&str]) -> Vec<Candidate> {
    words.iter().map(|w| Candidate::new(*w)).collect()
}

fn state(chinese: bool, schema: Schema, preedit: &str, words: &[&str]) -> InputState {
    InputState { snapshot: Snapshot { preedit: preedit.into(), candidates: cands(words), commit: None }, chinese, schema }
}

const NIHAO: &[&str] = &[
    "你好", "拟好", "你", "尼", "泥", "呢", "妮", "倪", "腻", "逆", "匿", "霓", "昵", "拟", "溺", "旎", "睨", "坭",
];

struct Scene {
    view: KeyboardView,
    w: f32,
    h: f32,
    t: u64,
}

impl Scene {
    fn new(theme: ThemeKind, w: f32, st: InputState) -> Self {
        let mut view = KeyboardView::new(KeyboardConfig { theme, schema: st.schema, chinese: st.chinese });
        let h = view.preferred_height(w);
        view.resize(w, h);
        let r = view.set_input_state(st);
        let mut s = Self { view, w, h, t: 1000 };
        s.serve(r.actions);
        s
    }

    /// Plays the host for data requests.
    fn serve(&mut self, actions: Vec<UiAction>) {
        for a in actions {
            match a {
                UiAction::WantMoreCandidates { start, count } => {
                    let more: Vec<Candidate> = (start..start + count.min(40))
                        .map(|i| Candidate::new(["你", "拟", "妮", "腻", "逆好", "泥土", "倪", "昵称", "霓虹"][i % 9]))
                        .collect();
                    let r = self.view.set_more_candidates(start, more);
                    self.serve(r.actions);
                }
                UiAction::WantT9Spellings => {
                    let r = self.view.set_t9_spellings(
                        ["ni", "mi", "mh", "ng", "oh", "oi", "nh", "mg"].iter().map(|s| s.to_string()).collect(),
                    );
                    self.serve(r.actions);
                }
                _ => {}
            }
        }
    }

    fn ptr(&mut self, id: u32, phase: PointerPhase, x: f32, y: f32) -> Vec<UiAction> {
        self.t += 30;
        let r = self.view.pointer(PointerEvent { id, phase, x, y, time_ms: self.t });
        let acts = r.actions.clone();
        self.serve(r.actions);
        acts
    }

    fn wait(&mut self, ms: u64) {
        self.t += ms;
        let r = self.view.timer(self.t);
        self.serve(r.actions);
    }

    fn render(&mut self, fonts: &Fonts, out: &Path, name: &str) {
        let mut c = SkiaCanvas::new(self.w, self.h, fonts);
        self.view.paint(&mut c);
        let path = out.join(format!("{name}.png"));
        c.pm.save_png(&path).unwrap();
        println!("{}", path.display());
    }
}

impl Scene {
    fn at(&self, name: &str) -> (f32, f32) {
        self.view.key_center(name).unwrap_or_else(|| panic!("no key {name}"))
    }

    fn tap(&mut self, name: &str) {
        let (x, y) = self.at(name);
        self.ptr(9, PointerPhase::Down, x, y);
        self.ptr(9, PointerPhase::Up, x, y);
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "ui-preview".into()));
    std::fs::create_dir_all(&out).unwrap();
    let fonts = Fonts::new();
    settings_scenes(&fonts, &out);
    if std::env::args().nth(2).as_deref() == Some("settings") {
        return;
    }
    let w = 1440.0;
    let light = ThemeKind::Light;
    let dark = ThemeKind::Dark;

    Scene::new(light, w, state(true, Schema::Pinyin, "", &[])).render(&fonts, &out, "01-pinyin-idle");
    Scene::new(light, w, state(true, Schema::Pinyin, "ni hao", NIHAO)).render(&fonts, &out, "02-pinyin-composing");

    let mut s = Scene::new(light, w, state(true, Schema::Shuangpin, "", &[]));
    s.render(&fonts, &out, "03-shuangpin-idle");

    // Pressed keys with bubbles: two thumbs, top row and middle row.
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "ni", NIHAO));
    let (x, y) = s.at("w");
    s.ptr(1, PointerPhase::Down, x, y);
    let (x, y) = s.at("k");
    s.ptr(2, PointerPhase::Down, x, y);
    s.render(&fonts, &out, "04-pressed-bubbles");

    // Long-press on the punctuation key.
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    let (x, y) = s.at("，");
    s.ptr(1, PointerPhase::Down, x, y);
    s.wait(400);
    s.ptr(1, PointerPhase::Move, x + 70.0, y);
    s.render(&fonts, &out, "05-longpress-punct");

    // Swipe-up on 'e' shows its secondary.
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    let (x, y) = s.at("e");
    s.ptr(1, PointerPhase::Down, x, y);
    s.ptr(1, PointerPhase::Move, x, y - 40.0);
    s.render(&fonts, &out, "06-swipeup");

    // Expanded candidate grid.
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "ni hao", NIHAO));
    s.tap("expand");
    s.render(&fonts, &out, "07-expanded-grid");

    Scene::new(light, w, state(true, Schema::T9, "", &[])).render(&fonts, &out, "08-t9-idle");
    Scene::new(light, w, state(true, Schema::T9, "ni", &["你", "尼", "泥", "呢", "妮", "倪", "腻"]))
        .render(&fonts, &out, "09-t9-composing");

    // Wide: the 123 panel is a tab of the symbol panel (digits have their own row).
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("符号");
    s.tap("123");
    s.render(&fonts, &out, "10-numbers");

    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("符号");
    s.render(&fonts, &out, "11-symbols");

    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("layout");
    s.render(&fonts, &out, "12-layout-menu");

    // English with shift latched (double tap).
    let mut s = Scene::new(light, w, state(false, Schema::Pinyin, "", &[]));
    s.tap("shift");
    s.tap("shift");
    s.render(&fonts, &out, "13-english-capslock");

    // Space drag (cursor mode).
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    let (x, y) = s.at("space");
    s.ptr(1, PointerPhase::Down, x, y);
    let acts = s.ptr(1, PointerPhase::Move, x + 90.0, y);
    assert!(acts.iter().any(|a| matches!(a, UiAction::Input(Action::Edit(_)))));
    s.render(&fonts, &out, "14-space-cursor");

    Scene::new(dark, w, state(true, Schema::Pinyin, "ni hao", NIHAO)).render(&fonts, &out, "20-dark-composing");
    Scene::new(dark, w, state(true, Schema::Shuangpin, "", &[])).render(&fonts, &out, "21-dark-shuangpin");
    Scene::new(dark, w, state(true, Schema::T9, "ni", &["你", "尼", "泥", "呢", "妮", "倪", "腻"]))
        .render(&fonts, &out, "22-dark-t9");
    let mut s = Scene::new(dark, w, state(true, Schema::Pinyin, "", &[]));
    let (x, y) = s.at("h");
    s.ptr(1, PointerPhase::Down, x, y);
    s.render(&fonts, &out, "23-dark-bubble");

    // Wide-only scenes (DESIGN.md §2「Surface 宽屏布局」「电脑按键」).
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.view.set_edit_area(false);
    s.render(&fonts, &out, "15-wide-no-edit-area");
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("ctrl");
    s.render(&fonts, &out, "16-wide-ctrl-hints");
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("fn");
    s.render(&fonts, &out, "17-wide-fn-layer");
    let mut s = Scene::new(light, w, state(false, Schema::Pinyin, "", &[]));
    s.tap("shift");
    s.render(&fonts, &out, "18-wide-english-shift");
    Scene::new(light, w, state(true, Schema::Pinyin, "n h", &[
        "你好", "女孩", "男孩", "你会", "那会", "南湖", "能会", "宁海", "你", "那", "年", "女", "男", "内", "能",
    ]))
    .render(&fonts, &out, "19-wide-abbrev");

    // Portrait (Surface held upright: 960 DIPs wide).
    let mut s = Scene::new(light, 960.0, state(true, Schema::Pinyin, "", &[]));
    s.render(&fonts, &out, "32-portrait-idle-toolbar");
    s.tap("pc");
    s.render(&fonts, &out, "33-portrait-pc-keys");
    // Portrait (Surface held upright: 960 DIPs wide).
    Scene::new(light, 960.0, state(true, Schema::Pinyin, "ni hao", NIHAO)).render(&fonts, &out, "30-portrait-composing");
    Scene::new(light, 960.0, state(true, Schema::T9, "", &[])).render(&fonts, &out, "31-portrait-t9");

    // 电脑键盘 (TODO #31) and selection / clipboard (TODO #32).
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.tap("layout");
    s.render(&fonts, &out, "40-layout-menu-pc");
    s.tap("pcmode");
    s.render(&fonts, &out, "41-pc-keyboard");
    let (x, y) = s.at("shift");
    s.ptr(2, PointerPhase::Down, x, y);
    let (x, y) = s.at("A");
    s.ptr(1, PointerPhase::Down, x, y);
    s.render(&fonts, &out, "42-pc-keyboard-shift");
    s.ptr(1, PointerPhase::Up, x, y);
    let (x, y) = s.at("shift");
    s.ptr(2, PointerPhase::Up, x, y);
    s.tap("caps");
    s.tap("fn");
    s.render(&fonts, &out, "43-pc-keyboard-caps-fn");
    let mut s = Scene::new(dark, w, state(true, Schema::Pinyin, "", &[]));
    s.view.set_pc_keyboard(true);
    s.render(&fonts, &out, "44-dark-pc-keyboard");
    let mut s = Scene::new(light, 960.0, state(true, Schema::Pinyin, "", &[]));
    s.view.set_pc_keyboard(true);
    s.render(&fonts, &out, "45-portrait-pc-keyboard");

    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    let (x, y) = s.at("space");
    s.ptr(1, PointerPhase::Down, x, y);
    s.wait(500);
    for i in 1..=6 {
        s.ptr(1, PointerPhase::Move, x + i as f32 * 12.0, y - i as f32 * 8.0);
    }
    s.render(&fonts, &out, "46-trackpad");
    let (qx, qy) = s.at("q");
    s.ptr(2, PointerPhase::Down, qx, qy);
    s.ptr(2, PointerPhase::Up, qx, qy);
    s.ptr(1, PointerPhase::Move, x + 120.0, y - 48.0);
    s.render(&fonts, &out, "47-trackpad-selecting");
    s.ptr(1, PointerPhase::Up, x + 120.0, y - 48.0);
    s.render(&fonts, &out, "48-select-bar");

    let sample = [
        "会议改到周四下午三点，地点不变",
        "https://dianmo.example/docs/clipboard",
        "收到，谢谢！",
        "点墨 Dianmo：让 Surface 上的中文输入像手机一样好用。复制、粘贴、选择都在键盘上完成。",
        "13800138000",
        "Cargo.toml",
    ];
    let items = |pin: bool| -> Vec<ClipItem> {
        sample
            .iter()
            .enumerate()
            .map(|(i, t)| ClipItem { id: i as u64 + 1, text: t.to_string(), pinned: pin && i == 4 })
            .collect()
    };
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.view.set_clips(items(false));
    s.view.set_paste_preview(Some(sample[0]));
    s.t += 10;
    s.view.notify_copied(s.t);
    s.render(&fonts, &out, "49-clipboard-bar");
    s.wait(1100);
    s.render(&fonts, &out, "50-clipboard-bar-after-toast");
    let mut s = Scene::new(light, w, state(true, Schema::Pinyin, "", &[]));
    s.view.set_clips(items(true));
    s.tap("clipboard");
    let (x, y) = s.at("clip2");
    s.ptr(1, PointerPhase::Down, x, y);
    s.wait(400);
    s.ptr(1, PointerPhase::Up, x, y);
    s.render(&fonts, &out, "51-clipboard-panel");
    let mut s = Scene::new(dark, 960.0, state(true, Schema::Pinyin, "", &[]));
    s.view.set_clips(items(false));
    s.t += 10;
    s.view.notify_copied(s.t);
    s.render(&fonts, &out, "52-portrait-dark-clipboard-bar");
    s.tap("clipclose");
    s.tap("select");
    s.render(&fonts, &out, "53-portrait-select-bar");
}


// ---------------------------------------------------------------------------------------------
// Settings window, about page, onboarding (TODO #33)
// ---------------------------------------------------------------------------------------------

fn render_view(v: &mut dyn View, w: f32, h: f32, fonts: &Fonts, out: &Path, name: &str) {
    v.resize(w, h);
    let mut c = SkiaCanvas::new(w, h, fonts).em_exact();
    v.paint(&mut c);
    let path = out.join(format!("{name}.png"));
    c.pm.save_png(&path).unwrap();
    println!("{}", path.display());
}

fn demo_model() -> SettingsModel {
    SettingsModel {
        admin_task: Status::ok("已注册，管理员窗口里也能打字"),
        dictionary: Status::ok("雾凇拼音 2026.09 · 已加载 · 约 50 万词"),
        user_words: Some(1284),
        engines: VoiceEngines {
            wetype: EngineStatus {
                available: true,
                detail: "已安装 2.1.3.18".into(),
                note: "点墨以管理员权限运行时通过辅助进程调用".into(),
                download_url: None,
            },
            doubao_ime: EngineStatus {
                available: false,
                detail: "全局语音快捷键没有打开".into(),
                download_url: Some("https://shurufa.doubao.com/pc".into()),
                ..Default::default()
            },
            doubao: EngineStatus { available: false, detail: "未检测到豆包语音".into(), ..Default::default() },
            system: EngineStatus { available: true, detail: "Windows 自带，随时可用".into(), ..Default::default() },
        },
        clip_count: 36,
        pinned_clips: vec![
            ClipItem { id: 1, text: "hello@example.com".into(), pinned: true },
            ClipItem { id: 2, text: "上海市徐汇区漕溪北路 88 号 5 楼".into(), pinned: true },
        ],
        version: "0.3.0".into(),
        build_date: "2026-10-06".into(),
        update: UpdateState::UpToDate,
        ..SettingsModel::default()
    }
}

fn settings_scenes(fonts: &Fonts, out: &Path) {
    let (w, h) = (960.0, 680.0);
    let light = ThemeKind::Light;
    let dark = ThemeKind::Dark;
    let page = |p: Page, kind: ThemeKind, m: SettingsModel| {
        let mut v = SettingsView::new(m, kind);
        v.resize(w, h);
        v.set_page(p);
        v
    };
    for (p, name) in [
        (Page::General, "settings-general"),
        (Page::Keyboard, "settings-keyboard"),
        (Page::Input, "settings-input"),
        (Page::Voice, "settings-voice"),
        (Page::Clipboard, "settings-clipboard"),
        (Page::About, "about"),
    ] {
        render_view(&mut page(p, light, demo_model()), w, h, fonts, out, name);
        render_view(&mut page(p, dark, demo_model()), w, h, fonts, out, &format!("{name}-dark"));
    }
    // Scrolled pages (the rest of the content).
    for (p, name) in [(Page::General, "settings-general-scrolled"), (Page::Keyboard, "settings-keyboard-scrolled"), (Page::Input, "settings-input-scrolled"), (Page::About, "about-scrolled")] {
        let mut v = page(p, light, demo_model());
        v.wheel(600.0, 400.0, 10000.0);
        render_view(&mut v, w, h, fonts, out, name);
    }

    // States: admin task missing (fix button), inline confirmation, update available, download.
    let mut m = demo_model();
    m.admin_task = Status::warn("计划任务未注册，管理员窗口里键盘不能打字");
    let mut v = page(Page::General, light, m);
    v.wheel(600.0, 400.0, 10000.0);
    let (x, y) = v.element_center("恢复默认").unwrap();
    v.pointer(PointerEvent { id: 1, phase: PointerPhase::Down, x, y, time_ms: 10 });
    v.pointer(PointerEvent { id: 1, phase: PointerPhase::Up, x, y, time_ms: 40 });
    render_view(&mut v, w, h, fonts, out, "settings-general-fix-confirm");

    let mut m = demo_model();
    m.update = UpdateState::Available {
        version: "0.4.0".into(),
        notes: "· 新增设置窗口、关于页和新手引导\n· 九宫格支持英文\n· 修复管理员窗口里偶尔不弹出键盘".into(),
    };
    render_view(&mut page(Page::About, light, m.clone()), w, h, fonts, out, "about-update");
    m.update = UpdateState::Downloading(0.42);
    render_view(&mut page(Page::About, dark, m), w, h, fonts, out, "about-downloading-dark");

    let mut m = demo_model();
    m.engines.wetype = EngineStatus {
        available: false,
        detail: "未检测到微信输入法".into(),
        note: String::new(),
        download_url: Some("https://z.weixin.qq.com/".into()),
    };
    m.voice_engine = dianmo_ui::settings::VoiceEngineChoice::System;
    render_view(&mut page(Page::Voice, light, m), w, h, fonts, out, "settings-voice-missing");

    // Narrow window: tabs on top.
    let mut v = page(Page::Input, light, demo_model());
    render_view(&mut v, 560.0, 820.0, fonts, out, "settings-narrow");
    let mut v = page(Page::General, dark, demo_model());
    render_view(&mut v, 560.0, 820.0, fonts, out, "settings-narrow-dark");

    // Onboarding.
    let (ow, oh) = (760.0, 560.0);
    for kind in [light, dark] {
        let mut o = OnboardingView::new(SettingsModel { layout: LayoutChoice::Pinyin, ..demo_model() }, kind);
        o.resize(ow, oh);
        for i in 0..dianmo_ui::settings::ONBOARDING_PAGES {
            o.set_page(i);
            let suffix = if kind == dark { "-dark" } else { "" };
            render_view(&mut o, ow, oh, fonts, out, &format!("onboarding-{}{suffix}", i + 1));
        }
    }
    // Mid-swipe between card 1 and 2.
    let mut o = OnboardingView::new(demo_model(), light);
    o.resize(ow, oh);
    o.pointer(PointerEvent { id: 1, phase: PointerPhase::Down, x: 600.0, y: 300.0, time_ms: 10 });
    o.pointer(PointerEvent { id: 1, phase: PointerPhase::Move, x: 400.0, y: 300.0, time_ms: 30 });
    o.pointer(PointerEvent { id: 1, phase: PointerPhase::Move, x: 300.0, y: 300.0, time_ms: 50 });
    render_view(&mut o, ow, oh, fonts, out, "onboarding-swipe");
    let _ = Level::Ok;
}
