//! Small illustrations drawn with the canvas (no images): keyboards, the voice ball, gestures.
//! Used by the input-mode cards and onboarding.

use crate::canvas::{Canvas, Color, Rect, TextStyle};
use crate::theme::{SettingsTheme, ThemeKind};

use super::model::LayoutChoice;
use super::widgets::{centered, icon, mix, style, text_w};

// ---------------------------------------------------------------------------------------------
// Illustrations (drawn with the canvas, no images)
// ---------------------------------------------------------------------------------------------

pub(crate) fn key_colors(t: &SettingsTheme) -> (Color, Color, Color) {
    match t.kind {
        ThemeKind::Light => (Color::rgb(231, 233, 238), Color::rgb(255, 255, 255), Color::rgb(205, 210, 219)),
        ThemeKind::Dark => (Color::rgb(21, 22, 25), Color::rgb(66, 68, 75), Color::rgb(44, 46, 52)),
    }
}

/// A small keyboard picture for the layout cards.
pub(crate) fn mini_keyboard(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, l: LayoutChoice, sel: bool) {
    let (bg, key, func) = key_colors(t);
    c.fill_rect(r, 10.0, bg);
    let pad = 6.0;
    let inner = Rect::new(r.x + pad, r.y + pad, r.w - 2.0 * pad, r.h - 2.0 * pad);
    let gap = (inner.w / 60.0).clamp(2.0, 4.0);
    let acc = if sel { t.accent } else { mix(t.text_faint, t.accent, 0.4) };
    match l {
        LayoutChoice::T9 => {
            let rows = 4;
            let kh = (inner.h - gap * (rows as f32 - 1.0)) / rows as f32;
            let kw = (inner.w - gap * 4.0) / 5.0;
            let labels = ["@/.", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ"];
            for row in 0..rows {
                let y = inner.y + row as f32 * (kh + gap);
                for col in 0..5 {
                    let x = inner.x + col as f32 * (kw + gap);
                    let kr = Rect::new(x, y, kw, kh);
                    let is_func = col == 0 || col == 4;
                    let fill = if row == 3 && col == 4 { acc } else if is_func || row == 3 { func } else { key };
                    if row == 3 && (1..4).contains(&col) {
                        if col == 1 {
                            c.fill_rect(Rect::new(x, y, kw * 3.0 + gap * 2.0, kh), 3.0, key);
                        }
                        continue;
                    }
                    c.fill_rect(kr, 3.0, fill);
                    if !is_func && row < 3 {
                        let lab = labels[row * 3 + col - 1];
                        c.text(lab, kr, centered((kh * 0.3).min(11.0), t.text_secondary));
                    }
                }
            }
        }
        _ => {
            let rows: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
            let n = 4.0;
            let kh = (inner.h - gap * (n - 1.0)) / n;
            let kw = (inner.w - gap * 9.0) / 10.0;
            for (ri, letters) in rows.iter().enumerate() {
                let y = inner.y + ri as f32 * (kh + gap);
                let count = letters.len() as f32;
                let x0 = inner.x + (inner.w - (count * kw + (count - 1.0) * gap)) / 2.0;
                if ri == 2 {
                    c.fill_rect(Rect::new(inner.x, y, x0 - inner.x - gap, kh), 3.0, func);
                    let ex = x0 + count * (kw + gap);
                    c.fill_rect(Rect::new(ex, y, inner.x + inner.w - ex, kh), 3.0, func);
                }
                for (i, ch) in letters.chars().enumerate() {
                    let kr = Rect::new(x0 + i as f32 * (kw + gap), y, kw, kh);
                    c.fill_rect(kr, 3.0, key);
                    if kw >= 9.0 {
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        if l == LayoutChoice::Shuangpin {
                            c.text(s, Rect::new(kr.x, kr.y, kr.w, kr.h * 0.7), centered((kh * 0.38).min(11.0), t.text_secondary));
                            c.fill_rect(Rect::new(kr.x + kr.w * 0.3, kr.y + kr.h * 0.72, kr.w * 0.4, 1.5), 0.75, acc);
                        } else {
                            c.text(s, kr, centered((kh * 0.42).min(12.0), t.text_secondary));
                        }
                    }
                }
            }
            let y = inner.y + 3.0 * (kh + gap);
            let fw = kw * 1.5;
            c.fill_rect(Rect::new(inner.x, y, fw, kh), 3.0, func);
            c.fill_rect(Rect::new(inner.x + fw + gap, y, inner.w - 2.0 * (fw + gap), kh), 3.0, key);
            c.fill_rect(Rect::new(inner.x + inner.w - fw, y, fw, kh), 3.0, acc);
        }
    }
}

pub(crate) fn finger(c: &mut dyn Canvas, t: &SettingsTheme, cx: f32, cy: f32) {
    c.fill_rect(Rect::new(cx - 18.0, cy - 18.0, 36.0, 36.0), 18.0, t.accent.with_alpha(0.16));
    c.fill_rect(Rect::new(cx - 11.0, cy - 11.0, 22.0, 22.0), 11.0, t.accent.with_alpha(0.85));
    c.fill_rect(Rect::new(cx - 4.0, cy - 4.0, 8.0, 8.0), 4.0, t.on_accent.with_alpha(0.9));
}

pub(crate) fn art_trackpad(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect) {
    let (bg, key, _) = key_colors(t);
    // A text line with the caret.
    let line = Rect::new(r.x + 8.0, r.y + 6.0, r.w - 16.0, 30.0);
    c.fill_rect(line, 6.0, t.fill);
    let txt = "指尖一点";
    let tw = text_w(txt, 13.0);
    let tx = line.x + (line.w - tw) / 2.0 - 4.0;
    c.text(txt, Rect::new(tx, line.y, tw + 4.0, line.h), style(13.0, t.text));
    c.fill_rect(Rect::new(tx + tw * 0.5, line.y + 7.0, 2.0, line.h - 14.0), 1.0, t.accent);
    // Space bar turned into a pad.
    let top = line.y + line.h + 14.0;
    let pad = Rect::new(r.x, top, r.w, r.y + r.h - top);
    c.fill_rect(pad, 10.0, bg);
    let bar = pad.inset(10.0);
    c.fill_rect(bar, 8.0, key);
    let (cx, cy) = (bar.x + bar.w / 2.0, bar.y + bar.h / 2.0);
    c.text("\u{E76B}", Rect::new(cx - 52.0, cy - 10.0, 20.0, 20.0), icon(14.0, t.text_faint));
    c.text("\u{E76C}", Rect::new(cx + 32.0, cy - 10.0, 20.0, 20.0), icon(14.0, t.text_faint));
    finger(c, t, cx, cy);
}

pub(crate) fn art_swipe(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect) {
    let (bg, key, _) = key_colors(t);
    c.fill_rect(r, 10.0, bg);
    let k = (r.h * 0.42).min(r.w * 0.3);
    let kr = Rect::new(r.x + (r.w - k) / 2.0, r.y + r.h - k - 14.0, k, k);
    // Neighbouring keys.
    for dx in [-1.0f32, 1.0] {
        let nr = Rect::new(kr.x + dx * (k + 8.0), kr.y, k, k);
        c.fill_rect(nr, 6.0, key.with_alpha(0.6));
    }
    c.fill_rect(kr, 6.0, key);
    c.text("e", Rect::new(kr.x, kr.y, k, k * 0.62), centered(k * 0.4, t.text));
    c.text("3", Rect::new(kr.x + k - 16.0, kr.y + 2.0, 14.0, 14.0), centered(10.0, t.text_faint));
    // Bubble showing the secondary.
    let b = Rect::new(kr.x - 4.0, (kr.y - k * 0.95 - 26.0).max(r.y + 8.0), k + 8.0, k * 0.95);
    c.fill_rect(b, 8.0, t.accent);
    c.text("3", b, TextStyle { bold: true, ..centered(k * 0.5, t.on_accent) });
    c.text("\u{E70E}", Rect::new(kr.x + k / 2.0 - 10.0, b.y + b.h + 1.0, 20.0, 24.0), icon(14.0, t.accent));
    finger(c, t, kr.x + k * 0.55, kr.y + k * 0.82);
}

pub(crate) fn art_ball(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect) {
    let (bg, _, _) = key_colors(t);
    c.push_clip(r);
    c.fill_rect(r, 10.0, bg.with_alpha(0.6));
    // Screen edge on the right with the ball docked against it.
    c.fill_rect(Rect::new(r.x + r.w - 6.0, r.y + 8.0, 3.0, r.h - 16.0), 1.5, t.fill_strong);
    let d = (r.h * 0.42).min(64.0);
    let (cx, cy) = (r.x + r.w - 14.0 - d / 2.0, r.y + r.h / 2.0);
    for (k, a) in [(1.9, 0.10), (1.45, 0.18)] {
        let dd = d * k;
        c.fill_rect(Rect::new(cx - dd / 2.0, cy - dd / 2.0, dd, dd), dd / 2.0, t.accent.with_alpha(a));
    }
    c.fill_rect(Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), d / 2.0, t.accent);
    c.text("\u{E720}", Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), icon(d * 0.42, t.on_accent));
    // A speech line being typed on the left.
    let lx = r.x + 12.0;
    for (i, w) in [0.5f32, 0.36, 0.44].iter().enumerate() {
        let y = r.y + r.h * 0.3 + i as f32 * 16.0;
        c.fill_rect(Rect::new(lx, y, r.w * w, 7.0), 3.5, if i == 2 { t.accent.with_alpha(0.5) } else { t.fill_strong });
    }
    c.pop_clip();
}

/// A miniature PC keyboard (function row, number row, letters, modifiers, arrows).
pub(crate) fn art_pc(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, sel: bool) {
    let (bg, key, func) = key_colors(t);
    c.fill_rect(r, 10.0, bg);
    let inner = r.inset(6.0);
    let gap = 2.0;
    // Six rows, the function row at 3/4 height.
    let units = 5.75;
    let kh = (inner.h - gap * 5.0) / units;
    let cols = 15.0;
    let kw = (inner.w - gap * (cols - 1.0)) / cols;
    let mut y = inner.y;
    let acc = if sel { t.accent } else { mix(t.text_faint, t.accent, 0.4) };
    for row in 0..6 {
        let h = if row == 0 { kh * 0.75 } else { kh };
        // (width in keys, function?) per row; widths sum to 15.
        let spec: &[(f32, bool)] = match row {
            0 => &[(1.0, true), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (2.0, true)],
            1 => &[(1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (2.0, true)],
            2 => &[(1.5, true), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.5, false)],
            3 => &[(1.75, true), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (2.25, true)],
            4 => &[(2.25, true), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.0, false), (1.75, true), (1.0, true)],
            _ => &[(1.25, true), (1.25, true), (1.25, true), (6.25, false), (1.25, true), (1.25, true), (1.0, true), (1.0, true), (1.0, true)],
        };
        let mut x = inner.x;
        let n = spec.len();
        let total_w: f32 = spec.iter().map(|s| s.0).sum();
        let unit = (inner.w - gap * (n as f32 - 1.0)) / total_w;
        for (i, &(w, f)) in spec.iter().enumerate() {
            let kwid = w * unit;
            let fill = if row == 3 && i == n - 1 { acc } else if f { func } else { key };
            c.fill_rect(Rect::new(x, y, kwid, h), 2.0, fill);
            x += kwid + gap;
        }
        y += h + gap;
    }
    let _ = kw;
}

/// The keyboard mode picture: a phone-style keyboard docked under a text line.
pub(crate) fn art_keyboard_mode(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, sel: bool) {
    mini_keyboard(c, t, r, LayoutChoice::Pinyin, sel);
}
