//! Selection, trackpad and clipboard (TODO #32, DESIGN.md §2「选择、复制与剪贴板」).
//!
//! - Trackpad: holding the space bar turns the key area into a trackpad. Dragging moves the caret
//!   (faster drags take bigger steps); a tap with another finger starts selecting (the moves carry
//!   Shift), further taps extend by a word. Lifting the finger ends it; if something was selected
//!   the selection bar opens.
//! - Selection mode (选择 key, or 复制 with nothing selected): the selection bar replaces the
//!   toolbar (by character / word / line, line start / end, select all; copy, cut, paste, delete,
//!   done) and the arrow keys extend the selection.
//! - After a copy: a short 「已复制」 toast, and the idle bar shows the clipboard history as cards
//!   (tap = paste) until the user types. The clipboard panel shows all entries; long-press a card
//!   to pin or delete it. The history itself lives in the host (`set_clips`).

use dianmo_core::{EditKey, KeyChord, KeyCode};

use crate::canvas::{Align, Font, Rect, TextStyle};
use crate::keyboard::{KeyboardView, Mode, Panel, TOAST_MS, Target, Touch, estimate_width};
use crate::layout::{self, Key, KeyAction, SelAct, Tone};
use crate::scroll::Scroller;
use crate::view::{ClipItem, PointerEvent, Response, UiAction};

/// Trackpad: finger travel (DIPs, after the speed gain) per caret step.
const PAD_STEP_X: f32 = 14.0;
const PAD_STEP_Y: f32 = 26.0;
/// Most caret steps sent for one pointer sample.
const PAD_MAX_STEPS: i32 = 40;

/// Arrow-like keys that extend the selection in selection mode.
pub(crate) fn extends_selection(e: EditKey) -> bool {
    matches!(
        e,
        EditKey::Left | EditKey::Right | EditKey::Up | EditKey::Down | EditKey::Home | EditKey::End | EditKey::PageUp | EditKey::PageDown
    )
}

/// Targets that scroll horizontally.
pub(crate) fn horizontal(t: &Target) -> bool {
    matches!(t, Target::Strip(_) | Target::ClipCard(_))
}

/// One line of `text` for a card: whitespace runs (newlines too) become one space, at most
/// `max` characters, "…" when cut.
pub fn clip_preview(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    let mut space = false;
    for c in text.trim().chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && n > 0 {
            if n + 1 >= max {
                out.push('…');
                return out;
            }
            out.push(' ');
            n += 1;
        }
        space = false;
        if n >= max {
            out.push('…');
            return out;
        }
        out.push(c);
        n += 1;
    }
    out
}

impl KeyboardView {
    // -----------------------------------------------------------------------------------------
    // Host API
    // -----------------------------------------------------------------------------------------

    /// The clipboard history (most recent first). Returns true if it changed; the caller repaints.
    pub fn set_clips(&mut self, clips: Vec<ClipItem>) -> bool {
        if clips == self.clips {
            return false;
        }
        self.clips = clips;
        if let Some(id) = self.clip_menu {
            if !self.clips.iter().any(|c| c.id == id) {
                self.clip_menu = None;
            }
        }
        self.rebuild();
        true
    }

    pub fn clips(&self) -> &[ClipItem] {
        &self.clips
    }

    /// What the paste key would paste (the system clipboard's text), shown small on the key.
    /// Returns true if it changed.
    pub fn set_paste_preview(&mut self, text: Option<&str>) -> bool {
        let p = text.map(|t| clip_preview(t, 6)).filter(|p| !p.is_empty());
        if p == self.paste_preview {
            return false;
        }
        self.paste_preview = p;
        self.rebuild();
        true
    }

    /// Something was just copied: 「已复制」 for a second, and the idle bar shows the clipboard
    /// cards (until the user types).
    pub fn notify_copied(&mut self, now_ms: u64) -> Response {
        if !self.pc {
            self.clip_bar = true;
            self.selecting = false;
            self.clip_strip.reset();
            self.rebuild();
        }
        self.show_toast("已复制", now_ms)
    }

    /// Shows a short message over the keys for [`TOAST_MS`] (a one-shot timer, no
    /// polling).
    pub fn show_toast(&mut self, text: &str, now_ms: u64) -> Response {
        self.now = self.now.max(now_ms);
        self.toast = Some((text.to_owned(), now_ms + TOAST_MS));
        self.finish(Response::repaint())
    }

    /// Opens selection mode (the host does this when 复制 found nothing selected). False if it
    /// does not apply (电脑键盘, composing) or is already open.
    pub fn enter_select_mode(&mut self) -> bool {
        if self.pc || self.composing() || self.selecting {
            return false;
        }
        self.set_selecting(true);
        true
    }

    pub fn selecting(&self) -> bool {
        self.selecting
    }

    /// The keyboard was hidden: drop selection mode, the clipboard bar, toasts and panels that
    /// belong to the last text field. Returns true if anything changed.
    pub fn reset_transient(&mut self) -> bool {
        let changed = self.selecting || self.clip_bar || self.toast.is_some() || self.clip_menu.is_some() || self.panel == Panel::Clipboard;
        self.selecting = false;
        self.clip_bar = false;
        self.toast = None;
        self.clip_menu = None;
        self.pad_select = false;
        self.pad_selected = false;
        if self.panel == Panel::Clipboard {
            self.panel = Panel::Keys;
        }
        if changed {
            self.rebuild();
        }
        changed
    }

    // -----------------------------------------------------------------------------------------
    // State helpers
    // -----------------------------------------------------------------------------------------

    pub(crate) fn set_selecting(&mut self, on: bool) {
        self.selecting = on;
        if on {
            self.clip_bar = false;
        }
        self.rebuild();
    }

    /// The user typed text: selection mode and the clipboard bar end (as on phones).
    pub(crate) fn typed(&mut self) {
        if self.selecting || self.clip_bar {
            self.selecting = false;
            self.clip_bar = false;
            self.rebuild();
        }
    }

    pub(crate) fn clip_bar_shown(&self) -> bool {
        self.clip_bar
            && !self.clips.is_empty()
            && !self.pc
            && !self.selecting
            && !self.composing()
            && self.panel != Panel::Candidates
    }

    pub(crate) fn trackpad_active(&self) -> bool {
        self.touches.iter().any(|t| matches!(t.mode, Mode::Trackpad { .. }))
    }

    pub(crate) fn scrollers(&self) -> [&Scroller; 6] {
        [&self.strip, &self.grid_scroll, &self.column_scroll, &self.sym_scroll, &self.clip_strip, &self.clip_scroll]
    }

    // -----------------------------------------------------------------------------------------
    // Trackpad
    // -----------------------------------------------------------------------------------------

    pub(crate) fn trackpad_move(&mut self, t: &mut Touch, e: PointerEvent, r: &mut Response) {
        let Mode::Trackpad { mut acc_x, mut acc_y, lx, ly, lt } = t.mode else { return };
        let (dx, dy) = (e.x - lx, e.y - ly);
        let dt = e.time_ms.saturating_sub(lt).max(1) as f32;
        // DIPs per ms; fast flicks move several characters / lines per DIP.
        let speed = (dx * dx + dy * dy).sqrt() / dt;
        let gain = (1.0 + (speed - 0.3).max(0.0) * 3.0).min(5.0);
        // The dominant axis moves; the other decays so a horizontal drag does not drift lines.
        if dx.abs() >= dy.abs() {
            acc_x += dx * gain;
            acc_y *= 0.6;
        } else {
            acc_y += dy * gain;
            acc_x *= 0.6;
        }
        let sx = ((acc_x / PAD_STEP_X).trunc() as i32).clamp(-PAD_MAX_STEPS, PAD_MAX_STEPS);
        acc_x -= sx as f32 * PAD_STEP_X;
        let sy = ((acc_y / PAD_STEP_Y).trunc() as i32).clamp(-PAD_MAX_STEPS, PAD_MAX_STEPS);
        acc_y -= sy as f32 * PAD_STEP_Y;
        for _ in 0..sx.abs() {
            self.pad_step(if sx > 0 { EditKey::Right } else { EditKey::Left }, false, r);
        }
        for _ in 0..sy.abs() {
            self.pad_step(if sy > 0 { EditKey::Down } else { EditKey::Up }, false, r);
        }
        t.mode = Mode::Trackpad { acc_x, acc_y, lx: e.x, ly: e.y, lt: e.time_ms };
        if sx != 0 || sy != 0 {
            r.repaint = true;
        }
    }

    fn pad_step(&mut self, key: EditKey, word: bool, r: &mut Response) {
        let a = if self.pad_select {
            self.pad_selected = true;
            dianmo_core::Action::Key(KeyChord { shift: true, ctrl: word, ..KeyChord::key(KeyCode::Edit(key)) })
        } else {
            dianmo_core::Action::Edit(key)
        };
        r.actions.push(UiAction::Input(a));
    }

    /// Another finger tapped the trackpad: start selecting; once selecting, extend by a word.
    pub(crate) fn trackpad_aux_tap(&mut self, r: &mut Response) {
        if !self.trackpad_active() {
            return;
        }
        if self.pad_select {
            self.pad_step(EditKey::Right, true, r);
        } else {
            self.pad_select = true;
        }
        r.repaint = true;
    }

    /// The trackpad finger lifted: back to the keys; the selection bar if something was selected.
    pub(crate) fn trackpad_end(&mut self, r: &mut Response) {
        for t in &mut self.touches {
            if t.target == Target::PadAux {
                t.consumed = true;
            }
        }
        let selected = self.pad_selected;
        self.pad_select = false;
        self.pad_selected = false;
        if selected {
            self.set_selecting(true);
        }
        r.repaint = true;
    }

    // -----------------------------------------------------------------------------------------
    // Bars
    // -----------------------------------------------------------------------------------------

    /// The selection bar: ‹ › by character, by word, ↑ ↓ by line, 行首 行尾 全选 on the left;
    /// 复制 剪切 粘贴 删除 完成 on the right. Every move carries Shift.
    pub(crate) fn build_select_bar(&self) -> Vec<Key> {
        let m = &self.m;
        let bh = m.bar_h;
        let mv = |e: EditKey, ctrl: bool, glyph: &str, cap: &str| {
            Key::icon(KeyAction::Press(KeyChord { ctrl, shift: true, ..KeyChord::key(KeyCode::Edit(e)) }), glyph, Tone::Flat).with_sub(cap)
        };
        let text = |a: KeyAction, label: &str| Key::new(a, label, Tone::Flat);
        let shift_edit = |e: EditKey| KeyAction::Press(KeyChord { shift: true, ..KeyChord::key(KeyCode::Edit(e)) });
        let left: Vec<(f32, Key)> = vec![
            (1.3, mv(EditKey::Left, false, layout::icon::CHEVRON_LEFT, "字")),
            (1.3, mv(EditKey::Right, false, layout::icon::CHEVRON_RIGHT, "字")),
            (1.3, mv(EditKey::Left, true, layout::icon::CHEVRON_LEFT, "词")),
            (1.3, mv(EditKey::Right, true, layout::icon::CHEVRON_RIGHT, "词")),
            (1.3, mv(EditKey::Up, false, layout::icon::CHEVRON_UP, "行")),
            (1.3, mv(EditKey::Down, false, layout::icon::CHEVRON_DOWN, "行")),
            (1.15, text(shift_edit(EditKey::Home), "行首")),
            (1.15, text(shift_edit(EditKey::End), "行尾")),
            (1.15, text(KeyAction::Chord(KeyChord::SELECT_ALL), "全选")),
        ];
        let mut done = text(KeyAction::Sel(SelAct::Done), "完成");
        done.tone = Tone::Accent;
        let right: Vec<(f32, Key)> = vec![
            (1.15, text(KeyAction::Sel(SelAct::Copy), "复制")),
            (1.15, text(KeyAction::Sel(SelAct::Cut), "剪切")),
            (1.15, text(KeyAction::Sel(SelAct::Paste), "粘贴")),
            (1.15, text(KeyAction::Sel(SelAct::Delete), "删除")),
            (1.35, done),
        ];
        let units: f32 = left.iter().chain(&right).map(|(u, _)| u).sum();
        let avail = m.w - 2.0 * m.pad_x - 12.0 * self.bar_s();
        let unit = (avail / units).min(bh);
        let mut keys = Vec::new();
        let mut x = m.pad_x + 4.0;
        for (u, mut k) in left {
            k.cell = Rect::new(x, 0.0, u * unit, bh);
            x += u * unit;
            keys.push(k);
        }
        let right_w: f32 = right.iter().map(|(u, _)| u * unit).sum();
        let mut x = m.w - m.pad_x - 4.0 - right_w;
        for (u, mut k) in right {
            k.cell = Rect::new(x, 0.0, u * unit, bh);
            x += u * unit;
            keys.push(k);
        }
        keys
    }

    /// Clipboard bar: cards on the left (see [`Self::clip_cards`]); 全部 (panel), close, hide.
    pub(crate) fn build_clip_bar(&self, right: Rect) -> Vec<Key> {
        let w = self.m.bar_h * 1.2;
        let mut hide = Key::icon(KeyAction::Hide, layout::icon::CHEVRON_DOWN, Tone::Flat);
        hide.cell = right;
        let mut close = Key::icon(KeyAction::CloseClipBar, layout::icon::CLOSE, Tone::Flat).scaled(0.6);
        close.cell = Rect::new(right.x - w, 0.0, w, self.m.bar_h);
        let mut all = Key::icon(KeyAction::ClipPanel, layout::icon::CLIPBOARD, Tone::Flat).with_sub("全部");
        let aw = self.m.bar_h * 1.9;
        all.cell = Rect::new(close.cell.x - aw, 0.0, aw, self.m.bar_h);
        vec![all, close, hide]
    }

    /// The part of the bar the clipboard cards scroll in.
    pub(crate) fn clip_bar_rect(&self) -> Rect {
        let right = self.bar_keys.iter().map(|k| k.cell.x).fold(self.m.w, f32::min);
        Rect::new(0.0, 0.0, (right - 4.0).max(0.0), self.m.bar_h)
    }

    pub(crate) fn clip_text_style(&self) -> TextStyle {
        let s = self.bar_s();
        TextStyle { size: if self.m.wide { 16.0 * s } else { 15.0 * self.m.s }, color: self.theme.text, align: Align::Center, bold: false, font: Font::Ui }
    }

    /// Bar cards in content coordinates: (x, width) and the text shown.
    pub(crate) fn clip_cards(&self) -> Vec<(f32, f32, String)> {
        let s = self.bar_s();
        let st = self.clip_text_style();
        let mut x = self.strip_lead();
        let mut out = Vec::with_capacity(self.clips.len());
        for c in &self.clips {
            let text = clip_preview(&c.text, 14);
            let w = (estimate_width(&text, st.size) + 28.0 * s).clamp(72.0 * s, 260.0 * s);
            out.push((x, w, text));
            x += w + 8.0 * s;
        }
        out
    }

    pub(crate) fn clip_card_at(&self, x: f32) -> Option<usize> {
        let cx = x + self.clip_strip.offset;
        self.clip_cards().iter().position(|&(ix, w, _)| cx >= ix && cx < ix + w)
    }

    /// Clipboard panel order: pinned entries first, then the rest, each most recent first.
    pub(crate) fn clip_panel_order(&self) -> Vec<usize> {
        let pinned = (0..self.clips.len()).filter(|&i| self.clips[i].pinned);
        pinned.chain((0..self.clips.len()).filter(|&i| !self.clips[i].pinned)).collect()
    }

    /// Clipboard panel grid: columns and cell size.
    pub(crate) fn clip_grid_geom(&self, grid: Rect) -> (usize, f32, f32) {
        let cols = ((grid.w / (self.m.row_h * 4.6)).floor() as usize).clamp(2, 5);
        (cols, grid.w / cols as f32, self.m.row_h * 1.25)
    }

    /// Cell of panel position `pos` on screen.
    pub(crate) fn clip_grid_cell(&self, grid: Rect, pos: usize) -> Rect {
        let (cols, cw, ch) = self.clip_grid_geom(grid);
        Rect::new(grid.x + (pos % cols) as f32 * cw, grid.y + (pos / cols) as f32 * ch - self.clip_scroll.offset, cw, ch)
    }

    pub(crate) fn clip_grid_hit(&self, x: f32, y: f32) -> Target {
        let Some(grid) = self.grid else { return Target::None };
        if !grid.contains(x, y) {
            return Target::None;
        }
        let (cols, cw, ch) = self.clip_grid_geom(grid);
        let col = ((x - grid.x) / cw).floor().clamp(0.0, cols as f32 - 1.0) as usize;
        let row = ((y - grid.y + self.clip_scroll.offset) / ch).floor();
        let order = self.clip_panel_order();
        let pos = row.max(0.0) as usize * cols + col;
        let Some(&i) = order.get(pos).filter(|_| row >= 0.0) else { return Target::ClipGrid(None) };
        let id = self.clips[i].id;
        if self.clip_menu == Some(id) {
            let cell = self.clip_grid_cell(grid, pos);
            return Target::ClipBtn { id, delete: x >= cell.x + cell.w / 2.0 };
        }
        Target::ClipGrid(Some(pos))
    }

    pub(crate) fn update_clip_scroll(&mut self) {
        let bar = self.clip_bar_rect();
        let end = self.clip_cards().last().map_or(0.0, |&(x, w, _)| x + w + 8.0);
        self.clip_strip.set_max(end - bar.w);
        if let (Panel::Clipboard, Some(grid)) = (self.panel, self.grid) {
            let (cols, _, ch) = self.clip_grid_geom(grid);
            let rows = self.clips.len().div_ceil(cols) as f32;
            self.clip_scroll.set_max(rows * ch - grid.h);
        }
    }

    /// Centre of clipboard card `i` (panel position when the panel is open, else bar index).
    pub(crate) fn clip_card_center(&self, i: usize) -> Option<(f32, f32)> {
        if self.panel == Panel::Clipboard {
            let grid = self.grid?;
            (i < self.clips.len()).then(|| {
                let c = self.clip_grid_cell(grid, i);
                (c.x + c.w / 2.0, c.y + c.h / 2.0)
            })
        } else if self.clip_bar_shown() {
            let cards = self.clip_cards();
            let &(x, w, _) = cards.get(i)?;
            let cx = x - self.clip_strip.offset + w / 2.0;
            (cx < self.clip_bar_rect().w).then_some((cx, self.m.bar_h / 2.0))
        } else {
            None
        }
    }
}
