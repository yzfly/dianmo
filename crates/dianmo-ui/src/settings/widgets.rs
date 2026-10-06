//! Settings component library: grouped cards of rows (title + description + control on the
//! right), switches, segmented choices, sliders, buttons, status dots, links, inline
//! confirmation, chips and a progress bar. Pages are described declaratively as [`Block`]s,
//! laid out into [`Laid`] (content coordinates, recomputed on every change; it is cheap) and
//! painted from it. Hit regions come out of the same layout pass.
//!
//! Sizes follow PRODUCT.md §1: cards radius 12, buttons 8, pill switches; title 20, group title
//! 13, body 15, description 12. Every tappable region is at least [`MIN_HIT`] DIPs tall.

use crate::canvas::{Align, Canvas, Color, Font, Rect, TextStyle};
use crate::theme::SettingsTheme;

use super::model::{Level, SettingsAction, Status};

pub(crate) const MIN_HIT: f32 = 44.0;
pub(crate) const CARD_RADIUS: f32 = 12.0;
pub(crate) const BTN_RADIUS: f32 = 8.0;
pub(crate) const PAGE_TITLE: f32 = 20.0;
pub(crate) const GROUP_TITLE: f32 = 13.0;
pub(crate) const BODY: f32 = 15.0;
pub(crate) const CAPTION: f32 = 12.0;

const ROW_PAD_X: f32 = 20.0;
const ROW_PAD_Y: f32 = 15.0;
const ROW_MIN_H: f32 = 60.0;
const TITLE_LINE: f32 = 22.0;
const DESC_LINE: f32 = 18.0;
const DESC_GAP: f32 = 3.0;
const CONTROL_GAP: f32 = 24.0;
/// The text column keeps at least this much width before the control moves below it.
const MIN_TEXT_W: f32 = 200.0;

pub(crate) const SWITCH_W: f32 = 46.0;
pub(crate) const SWITCH_H: f32 = 28.0;
const SEG_H: f32 = 36.0;
const SEG_PAD: f32 = 3.0;
const BTN_H: f32 = 36.0;
const SLIDER_W: f32 = 260.0;
const CHIP_H: f32 = 40.0;

// ---------------------------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------------------------

/// Width estimate (no canvas during layout): CJK = 1 em, ASCII ≈ 0.55 em.
pub(crate) fn text_w(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| match c {
            ' ' => 0.3,
            'i' | 'l' | 'j' | '.' | ',' | ':' | ';' | '|' | '!' | '\'' | '/' | '(' | ')' => 0.3,
            'm' | 'w' | 'M' | 'W' => 0.85,
            c if c.is_ascii_uppercase() || c.is_ascii_digit() => 0.6,
            c if c.is_ascii() => 0.53,
            '·' | '…' => 1.0,
            _ => 1.0,
        })
        .sum::<f32>()
        * size
}

/// Greedy line wrap for the estimate above; keeps ASCII words together when possible.
pub(crate) fn wrap(text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut w = 0.0;
        let mut tokens: Vec<String> = Vec::new();
        let mut word = String::new();
        for ch in para.chars() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/' | ':' | '%') {
                word.push(ch);
            } else {
                if !word.is_empty() {
                    tokens.push(std::mem::take(&mut word));
                }
                tokens.push(ch.to_string());
            }
        }
        if !word.is_empty() {
            tokens.push(word);
        }
        for t in tokens {
            let tw = text_w(&t, size);
            if w + tw > max_w && !line.is_empty() {
                out.push(std::mem::take(&mut line).trim_end().to_string());
                w = 0.0;
                if t == " " {
                    continue;
                }
            }
            // A single token longer than the line: split by chars.
            if tw > max_w {
                for ch in t.chars() {
                    let cw = text_w(&ch.to_string(), size);
                    if w + cw > max_w && !line.is_empty() {
                        out.push(std::mem::take(&mut line));
                        w = 0.0;
                    }
                    line.push(ch);
                    w += cw;
                }
            } else {
                line.push_str(&t);
                w += tw;
            }
        }
        out.push(line);
    }
    out
}

pub(crate) fn style(size: f32, color: Color) -> TextStyle {
    TextStyle { size, color, align: Align::Start, bold: false, font: Font::Ui }
}

pub(crate) fn centered(size: f32, color: Color) -> TextStyle {
    TextStyle { align: Align::Center, ..style(size, color) }
}

pub(crate) fn bold(size: f32, color: Color) -> TextStyle {
    TextStyle { bold: true, ..style(size, color) }
}

pub(crate) fn icon(size: f32, color: Color) -> TextStyle {
    TextStyle { font: Font::Icon, ..centered(size, color) }
}

pub(crate) fn level_color(t: &SettingsTheme, l: Level) -> Color {
    match l {
        Level::Unknown => t.text_faint,
        Level::Ok => t.success,
        Level::Warn => t.warning,
        Level::Error => t.danger,
    }
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

// ---------------------------------------------------------------------------------------------
// Declarative page description
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    Primary,
    Secondary,
    /// Red text on a soft red fill (destructive, before confirmation).
    Danger,
    /// Filled red (the final "清空").
    DangerSolid,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Button {
    pub label: String,
    pub kind: ButtonKind,
    pub act: SettingsAction,
    pub enabled: bool,
}

impl Button {
    pub fn new(label: impl Into<String>, kind: ButtonKind, act: SettingsAction) -> Self {
        Self { label: label.into(), kind, act, enabled: true }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn width(&self) -> f32 {
        (text_w(&self.label, 14.0) + 32.0).max(72.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SliderSpec {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub act: fn(f32) -> SettingsAction,
    pub format: fn(f32) -> String,
    /// Captions under the track (left, right).
    pub ends: (&'static str, &'static str),
}

impl SliderSpec {
    pub fn snap(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        let s = ((v - self.min) / self.step).round() * self.step + self.min;
        (s * 1000.0).round() / 1000.0
    }

    fn frac(&self) -> f32 {
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chip {
    pub label: String,
    pub on: bool,
    pub act: SettingsAction,
}

#[derive(Clone, Debug)]
pub(crate) enum Control {
    None,
    /// The row's `tap` toggles it (see [`Row::switch`]).
    Switch { on: bool },
    /// Options with the action each one sends; `selected` = index. Disabled options are shown
    /// faint and can't be picked.
    Segmented { options: Vec<(String, SettingsAction, bool)>, selected: Option<usize> },
    Slider(SliderSpec),
    Buttons(Vec<Button>),
    /// Danger button that expands into「取消 / <confirm>」in place (no dialog).
    Confirm { id: &'static str, label: String, question: String, confirm: String, act: SettingsAction },
    /// Text + chevron on the right; the whole row is tappable (row `tap`).
    Link { text: String, external: bool },
    Value(String),
    Radio { selected: bool },
    Progress(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TagKind {
    Accent,
    Neutral,
    Warning,
}

#[derive(Clone, Debug)]
pub(crate) struct Row {
    /// Stable id: animations, tests and GUI automation (`SettingsView::element_center`).
    pub key: String,
    pub title: String,
    pub tag: Option<(String, TagKind)>,
    pub desc: Option<String>,
    pub status: Option<Status>,
    pub control: Control,
    /// Optional control under the text (chips, progress, buttons on narrow windows).
    pub below: Option<Control>,
    pub chips: Vec<Chip>,
    /// Action of tapping anywhere on the row (switch rows toggle, radio rows select, links open).
    pub tap: Option<SettingsAction>,
    pub enabled: bool,
    /// Small icon in front of the title (MDL2 code point).
    pub icon: Option<&'static str>,
}

impl Row {
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            tag: None,
            desc: None,
            status: None,
            control: Control::None,
            below: None,
            chips: Vec::new(),
            tap: None,
            enabled: true,
            icon: None,
        }
    }

    pub fn desc(mut self, d: impl Into<String>) -> Self {
        self.desc = Some(d.into());
        self
    }

    pub fn status(mut self, s: Status) -> Self {
        self.status = Some(s);
        self
    }

    pub fn tag(mut self, t: impl Into<String>, kind: TagKind) -> Self {
        self.tag = Some((t.into(), kind));
        self
    }

    pub fn icon(mut self, i: &'static str) -> Self {
        self.icon = Some(i);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn control(mut self, c: Control) -> Self {
        self.control = c;
        self
    }

    pub fn below(mut self, c: Control) -> Self {
        self.below = Some(c);
        self
    }

    pub fn chips(mut self, c: Vec<Chip>) -> Self {
        self.chips = c;
        self
    }

    pub fn tap(mut self, a: SettingsAction) -> Self {
        self.tap = Some(a);
        self
    }

    /// A switch row: tapping the row toggles.
    pub fn switch(self, on: bool, act: fn(bool) -> SettingsAction) -> Self {
        let tap = act(!on);
        self.control(Control::Switch { on }).tap(tap)
    }

    pub fn segmented(self, options: Vec<(String, SettingsAction)>, selected: Option<usize>) -> Self {
        self.control(Control::Segmented { options: options.into_iter().map(|(l, a)| (l, a, true)).collect(), selected })
    }

    pub fn link(self, text: impl Into<String>, act: SettingsAction, external: bool) -> Self {
        self.control(Control::Link { text: text.into(), external }).tap(act)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Art {
    Keyboard,
    VoiceBall,
    PcKeyboard,
}

#[derive(Clone, Debug)]
pub(crate) struct Card {
    pub key: String,
    pub title: String,
    pub desc: String,
    pub art: Art,
    pub selected: bool,
    pub act: SettingsAction,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hero {
    pub title: String,
    pub subtitle: String,
    pub slogan: String,
}

#[derive(Clone, Debug)]
pub(crate) enum Block {
    Group { title: Option<String>, rows: Vec<Row>, note: Option<String> },
    Hero(Hero),
    /// Big selectable cards with a picture (常规 › 输入模式).
    Cards { title: Option<String>, cards: Vec<Card> },
    /// Centered small text lines (page footer).
    Footer(Vec<String>),
}

// ---------------------------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) enum Target {
    Act(SettingsAction),
    /// Switch row: animates the knob, then `Act`.
    Toggle { key: String, act: SettingsAction },
    Slider { row: usize },
    ConfirmOpen(&'static str),
    ConfirmCancel,
}

#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub rect: Rect,
    pub target: Target,
    /// Area drawn darker while pressed (a button, or the row inset).
    pub press: Rect,
    pub press_radius: f32,
    /// Row key or button label (lookup for tests and automation).
    pub name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LaidRow {
    pub row: Row,
    pub rect: Rect,
    pub title_y: f32,
    pub text_x: f32,
    pub text_w: f32,
    pub desc_lines: Vec<String>,
    pub desc_y: f32,
    pub status_y: f32,
    /// Main control (right side, or below the text when `stacked`).
    pub control: Rect,
    pub below: Rect,
    pub chip_rects: Vec<Rect>,
    /// The inline confirmation is open on this row.
    pub confirming: bool,
    pub first: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Laid {
    pub cards: Vec<Rect>,
    pub group_titles: Vec<(String, Rect)>,
    pub notes: Vec<(Vec<String>, Rect)>,
    pub rows: Vec<LaidRow>,
    pub hero: Option<(Hero, Rect)>,
    /// Selectable picture cards: (card, rect, art rect, description lines).
    pub picks: Vec<(Card, Rect, Rect, Vec<String>)>,
    pub footers: Vec<(Vec<String>, Rect)>,
    pub hits: Vec<Hit>,
    pub height: f32,
}

fn control_size(c: &Control, confirming: bool) -> (f32, f32) {
    match c {
        Control::None => (0.0, 0.0),
        Control::Switch { .. } => (SWITCH_W, SWITCH_H),
        Control::Segmented { options, .. } => {
            let w: f32 = options.iter().map(|(l, _, _)| seg_w(l)).sum();
            (w + 2.0 * SEG_PAD, SEG_H)
        }
        Control::Slider(_) => (SLIDER_W + 64.0, 40.0),
        Control::Buttons(b) => (b.iter().map(|b| b.width()).sum::<f32>() + 10.0 * (b.len().max(1) - 1) as f32, BTN_H),
        Control::Confirm { label, confirm, .. } => {
            if confirming {
                (Button::new("取消", ButtonKind::Secondary, SettingsAction::Close).width() + 10.0 + text_w(confirm, 14.0) + 32.0, BTN_H)
            } else {
                ((text_w(label, 14.0) + 32.0).max(72.0), BTN_H)
            }
        }
        Control::Link { text, .. } => (text_w(text, 14.0) + 26.0, 22.0),
        Control::Value(t) => (text_w(t, 14.0), 22.0),
        Control::Radio { .. } => (22.0, 22.0),
        Control::Progress(_) => (240.0, 8.0),
    }
}

fn seg_w(label: &str) -> f32 {
    (text_w(label, 14.0) + 28.0).max(60.0)
}

pub(crate) struct LayoutCtx<'a> {
    /// Id of the open inline confirmation.
    pub confirming: Option<&'a str>,
}

/// Lays `blocks` out in a column of width `w` starting at x = `x0`.
pub(crate) fn layout(blocks: &[Block], x0: f32, w: f32, ctx: &LayoutCtx) -> Laid {
    let mut l = Laid::default();
    let mut y = 0.0;
    for (bi, b) in blocks.iter().enumerate() {
        match b {
            Block::Hero(h) => {
                let r = Rect::new(x0, y, w, 236.0);
                l.hero = Some((h.clone(), r));
                y += r.h;
            }
            Block::Cards { title, cards } => {
                if let Some(t) = title {
                    y += if bi == 0 { 0.0 } else { 14.0 };
                    l.group_titles.push((t.clone(), Rect::new(x0 + 4.0, y, w - 8.0, 30.0)));
                    y += 30.0;
                }
                let n = cards.len().max(1) as f32;
                let gap = 12.0;
                let row = w >= 150.0 * n + gap * (n - 1.0) && w >= 520.0;
                if row {
                    let cw = (w - gap * (n - 1.0)) / n;
                    let lines: Vec<Vec<String>> = cards.iter().map(|c| wrap(&c.desc, CAPTION, cw - 28.0)).collect();
                    let max_lines = lines.iter().map(|l| l.len()).max().unwrap_or(1) as f32;
                    let art_h = (cw * 0.42).clamp(72.0, 110.0);
                    let h = 14.0 + art_h + 12.0 + TITLE_LINE + 2.0 + max_lines * DESC_LINE + 14.0;
                    for (i, (c, ls)) in cards.iter().zip(lines).enumerate() {
                        let r = Rect::new(x0 + i as f32 * (cw + gap), y, cw, h);
                        let art = Rect::new(r.x + 14.0, r.y + 14.0, r.w - 28.0, art_h);
                        l.hits.push(Hit { rect: r, target: Target::Act(c.act.clone()), press: r, press_radius: CARD_RADIUS, name: c.key.clone() });
                        l.picks.push((c.clone(), r, art, ls));
                    }
                    y += h;
                } else {
                    // Narrow: one card per line, picture on the left.
                    for c in cards {
                        let art_w = 112.0;
                        let ls = wrap(&c.desc, CAPTION, w - art_w - 28.0 - 14.0 - 36.0);
                        let h = (14.0 + TITLE_LINE + 2.0 + ls.len() as f32 * DESC_LINE + 14.0).max(92.0);
                        let r = Rect::new(x0, y, w, h);
                        let art = Rect::new(r.x + 12.0, r.y + 12.0, art_w, h - 24.0);
                        l.hits.push(Hit { rect: r, target: Target::Act(c.act.clone()), press: r, press_radius: CARD_RADIUS, name: c.key.clone() });
                        l.picks.push((c.clone(), r, art, ls));
                        y += h + 10.0;
                    }
                    y -= 10.0;
                }
            }
            Block::Footer(lines) => {
                y += 8.0;
                let r = Rect::new(x0, y, w, lines.len() as f32 * 20.0 + 16.0);
                l.footers.push((lines.clone(), r));
                y += r.h;
            }
            Block::Group { title, rows, note } => {
                if let Some(t) = title {
                    y += if bi == 0 { 0.0 } else { 14.0 };
                    l.group_titles.push((t.clone(), Rect::new(x0 + 4.0, y, w - 8.0, 30.0)));
                    y += 30.0;
                } else if bi > 0 {
                    y += 16.0;
                }
                let card_y = y;
                for (ri, row) in rows.iter().enumerate() {
                    let lr = layout_row(row, x0, y, w, ri == 0, ctx);
                    y += lr.rect.h;
                    push_hits(&mut l.hits, &lr, l.rows.len());
                    l.rows.push(lr);
                }
                l.cards.push(Rect::new(x0, card_y, w, y - card_y));
                if let Some(n) = note {
                    let lines = wrap(n, CAPTION, w - 8.0);
                    let r = Rect::new(x0 + 4.0, y + 8.0, w - 8.0, lines.len() as f32 * DESC_LINE);
                    y = r.y + r.h;
                    l.notes.push((lines, r));
                }
            }
        }
    }
    l.height = y + 32.0;
    l
}

fn layout_row(row: &Row, x0: f32, y: f32, w: f32, first: bool, ctx: &LayoutCtx) -> LaidRow {
    let confirming = matches!(&row.control, Control::Confirm { id, .. } if ctx.confirming == Some(*id));
    let (cw, ch) = control_size(&row.control, confirming);
    let inner_x = x0 + ROW_PAD_X;
    let inner_w = w - 2.0 * ROW_PAD_X;
    let icon_w = if row.icon.is_some() { 34.0 } else { 0.0 };
    let text_x = inner_x + icon_w;
    let avail = inner_w - icon_w;
    let stacked = cw > 0.0 && avail - cw - CONTROL_GAP < MIN_TEXT_W;
    let text_w_ = if stacked || cw == 0.0 { avail } else { avail - cw - CONTROL_GAP };
    let desc_text = if confirming {
        match &row.control {
            Control::Confirm { question, .. } => Some(question.clone()),
            _ => None,
        }
    } else {
        row.desc.clone()
    };
    let desc_lines = desc_text.map(|d| wrap(&d, CAPTION, text_w_)).unwrap_or_default();
    let mut ty = y + ROW_PAD_Y;
    let mut title_y = ty;
    ty += TITLE_LINE;
    let desc_y = ty + DESC_GAP;
    if !desc_lines.is_empty() {
        ty += DESC_GAP + desc_lines.len() as f32 * DESC_LINE;
    }
    let status_y = ty + DESC_GAP + 1.0;
    if row.status.is_some() {
        ty += DESC_GAP + 1.0 + DESC_LINE;
    }
    let single = desc_lines.is_empty() && row.status.is_none() && row.below.is_none() && row.chips.is_empty() && !stacked;
    let text_bottom = ty;
    let mut bottom = text_bottom;
    let mut control = Rect::default();
    if stacked {
        let cy = bottom + 12.0;
        let cw2 = match &row.control {
            Control::Segmented { .. } | Control::Slider(_) | Control::Progress(_) => avail,
            _ => cw.min(avail),
        };
        control = Rect::new(text_x, cy, cw2, ch);
        bottom = cy + ch;
    }
    let mut below = Rect::default();
    if let Some(b) = &row.below {
        let (bw, bh) = control_size(b, false);
        let bw = match b {
            Control::Progress(_) | Control::Slider(_) => avail,
            _ => bw.min(avail),
        };
        below = Rect::new(text_x, bottom + 12.0, bw, bh);
        bottom = below.y + bh;
    }
    let mut chip_rects = Vec::new();
    if !row.chips.is_empty() {
        let mut cx = text_x;
        let mut cy = bottom + 12.0;
        for c in &row.chips {
            let cw = text_w(&c.label, 14.0) + 32.0;
            if cx + cw > text_x + avail && cx > text_x {
                cx = text_x;
                cy += CHIP_H + 10.0;
            }
            chip_rects.push(Rect::new(cx, cy, cw, CHIP_H));
            cx += cw + 10.0;
        }
        bottom = cy + CHIP_H;
    }
    let h = (bottom - y + ROW_PAD_Y).max(ROW_MIN_H);
    if single {
        title_y = y + (h - TITLE_LINE) / 2.0;
    }
    if !stacked && cw > 0.0 {
        // Right-aligned, centred on the text block (or the whole row when it is short).
        let mid = if row.below.is_some() || !row.chips.is_empty() { (y + text_bottom + ROW_PAD_Y) / 2.0 } else { y + h / 2.0 };
        control = Rect::new(inner_x + inner_w - cw, mid - ch / 2.0, cw, ch);
    }
    LaidRow {
        row: row.clone(),
        rect: Rect::new(x0, y, w, h),
        title_y,
        text_x,
        text_w: text_w_,
        desc_lines,
        desc_y,
        status_y,
        control,
        below,
        chip_rects,
        confirming,
        first,
    }
}

/// Grows `r` vertically (centred) to at least [`MIN_HIT`], and a little horizontally.
fn hit_area(r: Rect) -> Rect {
    let h = r.h.max(MIN_HIT);
    let w = r.w.max(MIN_HIT);
    Rect::new(r.x - (w - r.w) / 2.0, r.y - (h - r.h) / 2.0, w, h)
}

fn button_rects(buttons: &[Button], area: Rect) -> Vec<Rect> {
    let total: f32 = buttons.iter().map(|b| b.width()).sum::<f32>() + 10.0 * buttons.len().saturating_sub(1) as f32;
    let mut x = area.x + (area.w - total).max(0.0);
    if area.w > total + 1.0 && area.w > 400.0 {
        // Stacked under the text: left-aligned.
        x = area.x;
    }
    buttons
        .iter()
        .map(|b| {
            let r = Rect::new(x, area.y + (area.h - BTN_H) / 2.0, b.width(), BTN_H);
            x += b.width() + 10.0;
            r
        })
        .collect()
}

pub(crate) fn segment_rects(options: &[(String, SettingsAction, bool)], area: Rect) -> Vec<Rect> {
    let natural: f32 = options.iter().map(|(l, _, _)| seg_w(l)).sum();
    let inner = area.w - 2.0 * SEG_PAD;
    let k = if inner > natural { inner / natural } else { 1.0 };
    let mut x = area.x + SEG_PAD;
    options
        .iter()
        .map(|(l, _, _)| {
            let w = seg_w(l) * k;
            let r = Rect::new(x, area.y + SEG_PAD, w, area.h - 2.0 * SEG_PAD);
            x += w;
            r
        })
        .collect()
}

/// Confirm control: (cancel, confirm) button rects.
pub(crate) fn confirm_rects(area: Rect, confirm: &str) -> (Rect, Rect) {
    let cw = text_w(confirm, 14.0) + 32.0;
    let kw = Button::new("取消", ButtonKind::Secondary, SettingsAction::Close).width();
    let x = if area.w > kw + cw + 20.0 && area.w > 400.0 { area.x } else { area.x + area.w - kw - 10.0 - cw };
    let y = area.y + (area.h - BTN_H) / 2.0;
    (Rect::new(x, y, kw, BTN_H), Rect::new(x + kw + 10.0, y, cw, BTN_H))
}

pub(crate) fn slider_track(area: Rect) -> Rect {
    // Value caption on the right (64 DIPs).
    Rect::new(area.x + 11.0, area.y, (area.w - 64.0 - 22.0).max(40.0), area.h)
}

fn control_hits(out: &mut Vec<Hit>, lr: &LaidRow, idx: usize, c: &Control, area: Rect, confirm: bool) {
    let name = lr.row.key.clone();
    match c {
        Control::Segmented { options, .. } => {
            for ((label, act, enabled), r) in options.iter().zip(segment_rects(options, area)) {
                if *enabled && lr.row.enabled {
                    out.push(Hit {
                        rect: hit_area(r),
                        target: Target::Act(act.clone()),
                        press: r,
                        press_radius: BTN_RADIUS - 2.0,
                        name: format!("{name}/{label}"),
                    });
                }
            }
        }
        Control::Slider(_) if lr.row.enabled => {
            let r = Rect::new(area.x - 8.0, area.y, area.w - 56.0, area.h);
            out.push(Hit { rect: hit_area(r), target: Target::Slider { row: idx }, press: Rect::default(), press_radius: 0.0, name });
        }
        Control::Buttons(bs) => {
            for (b, r) in bs.iter().zip(button_rects(bs, area)) {
                if b.enabled {
                    out.push(Hit {
                        rect: hit_area(r),
                        target: Target::Act(b.act.clone()),
                        press: r,
                        press_radius: BTN_RADIUS,
                        name: b.label.clone(),
                    });
                }
            }
        }
        Control::Confirm { id, label, confirm: ok, act, .. } if confirm => {
            if lr.confirming {
                let (k, c) = confirm_rects(area, ok);
                out.push(Hit { rect: hit_area(k), target: Target::ConfirmCancel, press: k, press_radius: BTN_RADIUS, name: "取消".into() });
                out.push(Hit { rect: hit_area(c), target: Target::Act(act.clone()), press: c, press_radius: BTN_RADIUS, name: ok.clone() });
            } else {
                out.push(Hit {
                    rect: hit_area(area),
                    target: Target::ConfirmOpen(id),
                    press: area,
                    press_radius: BTN_RADIUS,
                    name: label.clone(),
                });
            }
        }
        _ => {}
    }
}

fn push_hits(out: &mut Vec<Hit>, lr: &LaidRow, idx: usize) {
    // Specific controls first: hits are searched in order.
    control_hits(out, lr, idx, &lr.row.control, lr.control, true);
    if let Some(b) = &lr.row.below {
        control_hits(out, lr, idx, b, lr.below, false);
    }
    for (c, r) in lr.row.chips.iter().zip(&lr.chip_rects) {
        if lr.row.enabled {
            out.push(Hit {
                rect: hit_area(*r),
                target: Target::Act(c.act.clone()),
                press: *r,
                press_radius: CHIP_H / 2.0,
                name: format!("{}/{}", lr.row.key, c.label),
            });
        }
    }
    if let (Some(a), true) = (&lr.row.tap, lr.row.enabled) {
        let target = match &lr.row.control {
            Control::Switch { .. } => Target::Toggle { key: lr.row.key.clone(), act: a.clone() },
            _ => Target::Act(a.clone()),
        };
        let r = lr.rect;
        out.push(Hit { rect: r, target, press: r.inset(4.0), press_radius: CARD_RADIUS - 4.0, name: lr.row.key.clone() });
    }
}

// ---------------------------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------------------------

pub(crate) struct PaintCtx<'a> {
    pub t: &'a SettingsTheme,
    /// Content → window: y offset added to every rect (−scroll + top).
    pub dy: f32,
    pub pressed: Option<&'a Hit>,
    pub hovered: Option<&'a Hit>,
    /// Switch being animated: (row key, knob position 0..1).
    pub switch_anim: Option<(&'a str, f32)>,
}

pub(crate) fn shift(r: Rect, dy: f32) -> Rect {
    Rect::new(r.x, r.y + dy, r.w, r.h)
}

/// Soft drop shadow under cards (stacked translucent rounded rects).
pub(crate) fn shadow(c: &mut dyn Canvas, r: Rect, radius: f32, color: Color, depth: f32) {
    for i in 0..3 {
        let d = (i + 1) as f32 * depth / 3.0;
        let rr = Rect::new(r.x - d * 0.3, r.y + d * 0.6, r.w + d * 0.6, r.h + d * 0.4);
        c.fill_rect(rr, radius + d * 0.3, color.with_alpha(color.a / 3.0));
    }
}

pub(crate) fn paint_laid(c: &mut dyn Canvas, l: &Laid, p: &PaintCtx, hero: &dyn Fn(&mut dyn Canvas, &Hero, Rect)) {
    let t = p.t;
    if let Some((h, r)) = &l.hero {
        hero(c, h, shift(*r, p.dy));
    }
    for (title, r) in &l.group_titles {
        c.text(title, shift(*r, p.dy), bold(GROUP_TITLE, t.text_secondary));
    }
    for (card, r, art, lines) in &l.picks {
        paint_pick(c, p, card, shift(*r, p.dy), shift(*art, p.dy), lines);
    }
    for r in &l.cards {
        let r = shift(*r, p.dy);
        c.fill_rect(r, CARD_RADIUS, t.card);
        c.stroke_rect(r, CARD_RADIUS, 1.0, t.card_border);
    }
    for (lines, r) in &l.notes {
        for (i, line) in lines.iter().enumerate() {
            c.text(line, Rect::new(r.x, r.y + p.dy + i as f32 * DESC_LINE, r.w, DESC_LINE), style(CAPTION, t.text_faint));
        }
    }
    for (lines, r) in &l.footers {
        for (i, line) in lines.iter().enumerate() {
            c.text(line, Rect::new(r.x, r.y + p.dy + 8.0 + i as f32 * 20.0, r.w, 20.0), centered(CAPTION, t.text_faint));
        }
    }
    // Hover / pressed overlays of whole rows go under the row content.
    for (h, col) in [(p.hovered, t.hover), (p.pressed, t.pressed)] {
        if let Some(h) = h.filter(|h| h.press.w > 0.0 && h.press == h.rect.inset(4.0)) {
            c.fill_rect(shift(h.press, p.dy), h.press_radius, col);
        }
    }
    for lr in &l.rows {
        paint_row(c, lr, p);
    }
}

fn paint_pick(c: &mut dyn Canvas, p: &PaintCtx, card: &Card, r: Rect, art: Rect, lines: &[String]) {
    let t = p.t;
    let pressed = p.pressed.is_some_and(|h| h.name == card.key);
    let hovered = p.hovered.is_some_and(|h| h.name == card.key);
    let bg = if pressed {
        mix(t.card, t.text, 0.06)
    } else if hovered {
        mix(t.card, t.text, 0.025)
    } else {
        t.card
    };
    c.fill_rect(r, CARD_RADIUS, bg);
    if card.selected {
        c.stroke_rect(r.inset(1.0), CARD_RADIUS - 1.0, 2.0, t.accent);
    } else {
        c.stroke_rect(r, CARD_RADIUS, 1.0, t.card_border);
    }
    match card.art {
        Art::Keyboard => super::art::art_keyboard_mode(c, t, art, card.selected),
        Art::VoiceBall => super::art::art_ball(c, t, art),
        Art::PcKeyboard => super::art::art_pc(c, t, art, card.selected),
    }
    let side = art.h >= r.h - 24.5;
    let (tx, ty, tw) = if side {
        let tx = art.x + art.w + 16.0;
        let h = TITLE_LINE + 2.0 + lines.len() as f32 * DESC_LINE;
        (tx, r.y + (r.h - h) / 2.0, r.x + r.w - tx - 44.0)
    } else {
        (r.x + 14.0, art.y + art.h + 12.0, r.w - 28.0)
    };
    let title_col = if card.selected { t.accent } else { t.text };
    c.text(&card.title, Rect::new(tx, ty, tw, TITLE_LINE), bold(BODY, title_col));
    for (i, line) in lines.iter().enumerate() {
        c.text(line, Rect::new(tx, ty + TITLE_LINE + 2.0 + i as f32 * DESC_LINE, tw, DESC_LINE), style(CAPTION, t.text_faint));
    }
    // Radio-style check in the corner.
    let d = 22.0;
    let b = if side {
        Rect::new(r.x + r.w - 36.0, r.y + (r.h - d) / 2.0, d, d)
    } else {
        Rect::new(r.x + r.w - d - 14.0, ty + (TITLE_LINE - d) / 2.0, d, d)
    };
    if card.selected {
        c.fill_rect(b, d / 2.0, t.accent);
        c.text("\u{E73E}", b, icon(12.0, t.on_accent));
    } else {
        c.stroke_rect(b.inset(0.75), d / 2.0, 1.5, t.switch_off);
    }
}

fn paint_row(c: &mut dyn Canvas, lr: &LaidRow, p: &PaintCtx) {
    let t = p.t;
    let dy = p.dy;
    let row = &lr.row;
    if !lr.first {
        c.fill_rect(Rect::new(lr.rect.x + ROW_PAD_X, lr.rect.y + dy, lr.rect.w - 2.0 * ROW_PAD_X, 1.0), 0.0, t.divider);
    }
    let dim = |col: Color| if row.enabled { col } else { col.with_alpha(col.a * 0.45) };
    if let Some(ic) = row.icon {
        c.text(ic, Rect::new(lr.text_x - 34.0, lr.title_y + dy, 22.0, TITLE_LINE), icon(18.0, dim(t.text_secondary)));
    }
    let ts = style(BODY, dim(t.text));
    c.text(&row.title, Rect::new(lr.text_x, lr.title_y + dy, lr.text_w, TITLE_LINE), ts);
    if let Some((tag, kind)) = &row.tag {
        let x = lr.text_x + c.measure_text(&row.title, ts).min(lr.text_w) + 8.0;
        let w = c.measure_text(tag, style(11.0, fg_of(t, *kind))) + 12.0;
        let (bg, fg) = match kind {
            TagKind::Accent => (t.accent_soft, t.accent),
            TagKind::Neutral => (t.fill, t.text_faint),
            TagKind::Warning => (t.warning_soft, t.warning),
        };
        let r = Rect::new(x, lr.title_y + dy + 2.0, w, 18.0);
        c.fill_rect(r, 4.0, bg);
        c.text(tag, r, centered(11.0, fg));
    }
    if let Some(s) = &row.status {
        status_line(c, t, s, Rect::new(lr.text_x, lr.status_y + dy, lr.text_w, DESC_LINE));
    }
    let desc_color = if lr.confirming { t.danger } else { t.text_faint };
    for (i, line) in lr.desc_lines.iter().enumerate() {
        c.text(line, Rect::new(lr.text_x, lr.desc_y + dy + i as f32 * DESC_LINE, lr.text_w, DESC_LINE), style(CAPTION, dim(desc_color)));
    }
    paint_control(c, lr, &row.control, shift(lr.control, dy), p, true);
    if let Some(b) = &row.below {
        paint_control(c, lr, b, shift(lr.below, dy), p, false);
    }
    for (chip, r) in row.chips.iter().zip(&lr.chip_rects) {
        let r = shift(*r, dy);
        let pressed = p.pressed.is_some_and(|h| shift(h.press, dy) == r);
        let (bg, fg, border) = if chip.on { (t.accent_soft, t.accent, t.accent) } else { (t.card, t.text_secondary, t.fill_strong) };
        c.fill_rect(r, r.h / 2.0, if pressed { mix(bg, t.text, 0.08) } else { bg });
        c.stroke_rect(r, r.h / 2.0, 1.0, border);
        let check = if chip.on { "\u{E73E}" } else { "" };
        let tw = text_w(&chip.label, 14.0);
        let total = tw + if chip.on { 20.0 } else { 0.0 };
        let x = r.x + (r.w - total) / 2.0;
        if chip.on {
            c.text(check, Rect::new(x - 2.0, r.y, 16.0, r.h), icon(12.0, fg));
        }
        c.text(&chip.label, Rect::new(x + total - tw, r.y, tw + 4.0, r.h), style(14.0, dim(fg)));
    }
}

fn fg_of(t: &SettingsTheme, k: TagKind) -> Color {
    match k {
        TagKind::Accent => t.accent,
        TagKind::Neutral => t.text_faint,
        TagKind::Warning => t.warning,
    }
}

pub(crate) fn status_line(c: &mut dyn Canvas, t: &SettingsTheme, s: &Status, r: Rect) {
    let col = level_color(t, s.level);
    c.fill_rect(Rect::new(r.x, r.y + r.h / 2.0 - 4.0, 8.0, 8.0), 4.0, col);
    let fg = match s.level {
        Level::Warn => t.warning,
        Level::Error => t.danger,
        _ => t.text_secondary,
    };
    c.text(&s.text, Rect::new(r.x + 14.0, r.y, r.w - 14.0, r.h), style(CAPTION, fg));
}

fn paint_control(c: &mut dyn Canvas, lr: &LaidRow, ctrl: &Control, r: Rect, p: &PaintCtx, main: bool) {
    let t = p.t;
    let enabled = lr.row.enabled;
    let pressed_rect = p.pressed.map(|h| shift(h.press, p.dy));
    let hovered_rect = p.hovered.map(|h| shift(h.press, p.dy));
    match ctrl {
        Control::None => {}
        Control::Switch { on, .. } => {
            let pos = match p.switch_anim {
                Some((k, pos)) if k == lr.row.key => pos,
                _ => {
                    if *on {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
            switch(c, t, r, pos, enabled);
        }
        Control::Segmented { options, selected } => {
            c.fill_rect(r, BTN_RADIUS, t.fill);
            for (i, ((label, _, en), sr)) in options.iter().zip(segment_rects(options, r)).enumerate() {
                let sel = *selected == Some(i);
                if sel {
                    if t.kind == crate::theme::ThemeKind::Light {
                        c.fill_rect(Rect::new(sr.x, sr.y + 1.0, sr.w, sr.h), BTN_RADIUS - 2.0, t.shadow);
                    }
                    c.fill_rect(sr, BTN_RADIUS - 2.0, if t.kind == crate::theme::ThemeKind::Light { t.card } else { t.fill_strong });
                } else if pressed_rect == Some(sr) {
                    c.fill_rect(sr, BTN_RADIUS - 2.0, t.pressed);
                }
                let col = if !en || !enabled {
                    t.text_faint.with_alpha(t.text_faint.a * 0.7)
                } else if sel {
                    t.accent
                } else {
                    t.text_secondary
                };
                let st = TextStyle { bold: sel, ..centered(14.0, col) };
                c.text(label, sr, st);
            }
        }
        Control::Slider(s) => slider(c, t, r, s, enabled),
        Control::Buttons(bs) => {
            for (b, br) in bs.iter().zip(button_rects(bs, r)) {
                let st = if pressed_rect == Some(br) {
                    BtnState::Pressed
                } else if hovered_rect == Some(br) {
                    BtnState::Hover
                } else {
                    BtnState::Normal
                };
                button(c, t, br, &b.label, b.kind, b.enabled, st);
            }
        }
        Control::Confirm { label, confirm, .. } => {
            if lr.confirming && main {
                let (k, ok) = confirm_rects(r, confirm);
                let st = |x: Rect| if pressed_rect == Some(x) { BtnState::Pressed } else { BtnState::Normal };
                button(c, t, k, "取消", ButtonKind::Secondary, true, st(k));
                button(c, t, ok, confirm, ButtonKind::DangerSolid, true, st(ok));
            } else {
                let st = if pressed_rect == Some(r) { BtnState::Pressed } else { BtnState::Normal };
                button(c, t, r, label, ButtonKind::Danger, enabled, st);
            }
        }
        Control::Link { text, external } => {
            let st = TextStyle { align: Align::End, ..style(14.0, t.text_faint) };
            c.text(text, Rect::new(r.x - 40.0, r.y, r.w + 40.0 - 24.0, r.h), st);
            let glyph = if *external { "\u{E8A7}" } else { "\u{E76C}" };
            c.text(glyph, Rect::new(r.x + r.w - 18.0, r.y, 18.0, r.h), icon(12.0, t.text_faint));
        }
        Control::Value(v) => {
            let st = TextStyle { align: Align::End, ..style(14.0, t.text_secondary) };
            c.text(v, r, st);
        }
        Control::Radio { selected } => radio(c, t, r, *selected),
        Control::Progress(f) => progress(c, t, r, *f),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BtnState {
    Normal,
    Hover,
    Pressed,
}

pub(crate) fn button(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, label: &str, kind: ButtonKind, enabled: bool, st: BtnState) {
    let (bg, fg) = match kind {
        ButtonKind::Primary => (if st == BtnState::Pressed { t.accent_pressed } else { t.accent }, t.on_accent),
        ButtonKind::Secondary => (t.fill, t.text),
        ButtonKind::Danger => (t.danger_soft, t.danger),
        ButtonKind::DangerSolid => (t.danger, t.on_accent),
    };
    let bg = match (kind, st) {
        (ButtonKind::Primary, _) => bg,
        (_, BtnState::Pressed) => mix(bg, t.text, 0.10),
        (_, BtnState::Hover) => mix(bg, t.text, 0.04),
        _ => bg,
    };
    let (bg, fg) = if enabled { (bg, fg) } else { (bg.with_alpha(bg.a * 0.5), fg.with_alpha(fg.a * 0.45)) };
    c.fill_rect(r, BTN_RADIUS, bg);
    let st = TextStyle { bold: kind == ButtonKind::Primary || kind == ButtonKind::DangerSolid, ..centered(14.0, fg) };
    c.text(label, r, st);
}

/// Pill switch; `pos` 0 = off, 1 = on (fractional while animating).
pub(crate) fn switch(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, pos: f32, enabled: bool) {
    let track = mix(t.switch_off, t.accent, pos);
    let track = if enabled { track } else { track.with_alpha(track.a * 0.45) };
    c.fill_rect(r, r.h / 2.0, track);
    let k = r.h - 6.0;
    let x = r.x + 3.0 + (r.w - 6.0 - k) * pos;
    let kr = Rect::new(x, r.y + 3.0, k, k);
    c.fill_rect(Rect::new(kr.x, kr.y + 1.0, k, k), k / 2.0, Color::rgb(0, 0, 0).with_alpha(0.12));
    c.fill_rect(kr, k / 2.0, t.knob);
}

pub(crate) fn radio(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, selected: bool) {
    let d = r.w.min(r.h);
    let o = Rect::new(r.x + (r.w - d) / 2.0, r.y + (r.h - d) / 2.0, d, d);
    if selected {
        c.fill_rect(o, d / 2.0, t.accent);
        c.fill_rect(o.inset(d * 0.3), d * 0.2, t.on_accent);
    } else {
        c.stroke_rect(o.inset(0.75), d / 2.0, 1.5, t.switch_off);
    }
}

pub(crate) fn progress(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, f: f32) {
    let tr = Rect::new(r.x, r.y + (r.h - 6.0) / 2.0, r.w, 6.0);
    c.fill_rect(tr, 3.0, t.fill_strong);
    c.fill_rect(Rect::new(tr.x, tr.y, (tr.w * f.clamp(0.0, 1.0)).max(6.0), 6.0), 3.0, t.accent);
}

fn slider(c: &mut dyn Canvas, t: &SettingsTheme, r: Rect, s: &SliderSpec, enabled: bool) {
    let tr = slider_track(r);
    let cy = tr.y + 14.0;
    let f = s.frac();
    let line = Rect::new(tr.x, cy - 2.0, tr.w, 4.0);
    c.fill_rect(line, 2.0, t.fill_strong);
    let acc = if enabled { t.accent } else { t.text_faint };
    c.fill_rect(Rect::new(tr.x, cy - 2.0, tr.w * f, 4.0), 2.0, acc);
    // Tick at the default (1.0) when in range.
    let k = 22.0;
    let kx = tr.x + tr.w * f - k / 2.0;
    let kr = Rect::new(kx, cy - k / 2.0, k, k);
    c.fill_rect(Rect::new(kr.x, kr.y + 1.5, k, k), k / 2.0, Color::rgb(0, 0, 0).with_alpha(0.16));
    c.fill_rect(kr, k / 2.0, t.knob);
    c.stroke_rect(kr.inset(0.5), k / 2.0, 1.0, t.fill_strong);
    c.fill_rect(kr.inset(7.0), 4.0, acc);
    let ends = Rect::new(tr.x - 4.0, cy + 12.0, tr.w + 8.0, 16.0);
    c.text(s.ends.0, ends, style(11.0, t.text_faint));
    c.text(s.ends.1, ends, TextStyle { align: Align::End, ..style(11.0, t.text_faint) });
    let vr = Rect::new(r.x + r.w - 56.0, cy - 12.0, 56.0, 24.0);
    c.fill_rect(vr, 6.0, t.fill);
    c.text(&(s.format)(s.value), vr, TextStyle { bold: true, ..centered(13.0, t.text) });
}

/// Value for a finger at window x on the slider of row `lr`.
pub(crate) fn slider_value(lr: &LaidRow, x: f32) -> Option<f32> {
    let Control::Slider(s) = &lr.row.control else { return None };
    let tr = slider_track(lr.control);
    let f = ((x - tr.x) / tr.w).clamp(0.0, 1.0);
    Some(s.snap(s.min + f * (s.max - s.min)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_long_text() {
        let lines = wrap("点输入框时自动弹出键盘，点别处时自动收起", 12.0, 120.0);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| text_w(l, 12.0) <= 120.0 + 0.1));
        assert_eq!(wrap("a\nb", 12.0, 100.0), vec!["a", "b"]);
    }

    #[test]
    fn slider_snaps_to_steps() {
        let s = SliderSpec {
            value: 1.0,
            min: 0.7,
            max: 1.5,
            step: 0.05,
            act: SettingsAction::SetKeyboardHeight,
            format: |v| format!("{v}"),
            ends: ("", ""),
        };
        assert_eq!(s.snap(1.02), 1.0);
        assert_eq!(s.snap(2.0), 1.5);
        assert_eq!(s.snap(0.0), 0.7);
    }
}
