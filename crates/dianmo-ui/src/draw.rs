//! Painting of [`KeyboardView`]. Everything is drawn from scratch on each paint (cheap with
//! Direct2D); overlays (bubbles, popups) go last so they may cover the candidate bar.

use crate::canvas::{Align, Canvas, Color, Font, Rect, TextStyle};
use crate::clip::clip_preview;
use crate::keyboard::{KeyboardView, Mode, Panel, Target, Touch, estimate_width};
use crate::layout::{self, ColumnKind, Key, KeyAction, Latch, Modifier, Tone};

impl KeyboardView {
    pub(crate) fn draw(&self, c: &mut dyn Canvas) {
        c.clear(self.theme.background);
        self.draw_bar(c);
        if self.trackpad_active() {
            self.draw_trackpad(c);
            self.draw_toast(c);
            return;
        }
        match self.panel {
            Panel::Candidates => self.draw_candidate_grid(c),
            Panel::Symbols(_) => self.draw_symbol_grid(c),
            Panel::Menu => self.draw_menu_title(c),
            Panel::Clipboard => self.draw_clip_panel(c),
            _ => {}
        }
        if self.column.is_some() {
            self.draw_column(c);
        }
        for k in &self.keys {
            self.draw_key(c, k);
        }
        // Overlays.
        for t in &self.touches {
            self.draw_overlay(c, t);
        }
        self.draw_toast(c);
    }

    fn style(&self, size: f32, color: Color) -> TextStyle {
        TextStyle { size, color, align: Align::Center, bold: false, font: Font::Ui }
    }

    fn icon_style(&self, size: f32, color: Color) -> TextStyle {
        TextStyle { font: Font::Icon, ..self.style(size, color) }
    }

    /// The live touch pressing `k`, if any.
    fn press_of(&self, k: &Key, bar: bool) -> Option<&Touch> {
        self.touches.iter().find(|t| {
            !t.consumed
                && match &t.target {
                    Target::Key(tk) if !bar => tk.cell == k.cell && tk.action == k.action,
                    Target::Bar(tk) if bar => tk.cell == k.cell && tk.action == k.action,
                    _ => false,
                }
        })
    }

    fn pressed_index(&self, f: impl Fn(&Target) -> Option<usize>) -> Option<usize> {
        self.touches.iter().find(|t| !t.consumed && !t.stopped_fling && t.mode == Mode::Press).and_then(|t| f(&t.target))
    }

    // -----------------------------------------------------------------------------------------
    // Candidate bar
    // -----------------------------------------------------------------------------------------

    fn draw_bar(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        if self.panel == Panel::Candidates {
            let lead = self.strip_lead();
            let st = TextStyle { align: Align::Start, ..self.style(17.0 * s, th.text_secondary) };
            c.text(&self.snapshot.preedit, Rect::new(lead, 0.0, m.w * 0.7, m.bar_h), st);
        } else if self.composing() {
            self.draw_strip(c);
        } else if self.clip_bar_shown() {
            self.draw_clip_bar(c);
        } else if self.pc && !self.voice_mode {
            let st = self.style(13.0 * self.bar_s(), th.text_faint);
            c.text("电脑键盘 · 按键直通，由当前应用的输入法处理", Rect::new(0.0, 0.0, m.w, m.bar_h), st);
        }
        let bs = self.bar_s();
        for k in &self.bar_keys {
            let pressed = self.press_of(k, true).is_some();
            if pressed {
                let d = m.bar_h * 0.8;
                let r = if k.sub.is_some() {
                    Rect::new(k.cell.x + 3.0 * bs, k.cell.y + (k.cell.h - d) / 2.0, k.cell.w - 6.0 * bs, d)
                } else {
                    Rect::new(k.cell.x + (k.cell.w - d) / 2.0, k.cell.y + (k.cell.h - d) / 2.0, d, d)
                };
                c.fill_rect(r, d / 2.0, th.flat_pressed);
            }
            if k.action == KeyAction::VoiceBall {
                // 「退出语音模式」 (voice mode): a filled accent pill, the most visible thing here.
                let h = m.bar_h * 0.72;
                let r = Rect::new(k.cell.x + 3.0 * bs, k.cell.y + (k.cell.h - h) / 2.0, k.cell.w - 6.0 * bs, h);
                c.fill_rect(r, h / 2.0, if pressed { th.accent_pressed } else { th.accent });
                let st = TextStyle { bold: true, ..self.style(16.0 * bs, th.on_accent) };
                c.text(&k.label, r, st);
                continue;
            }
            if k.action == KeyAction::Voice && self.voice_active {
                // Recording: a red disc behind a white microphone.
                let d = m.bar_h * 0.72;
                let r = Rect::new(k.cell.x + (k.cell.w - d) / 2.0, k.cell.y + (k.cell.h - d) / 2.0, d, d);
                c.fill_rect(r, d / 2.0, if pressed { RECORDING_PRESSED } else { RECORDING });
                c.text(&k.label, k.cell, self.icon_style(19.0 * s * k.scale / 0.78, Color::rgb(255, 255, 255)));
                continue;
            }
            let color = if matches!(k.action, KeyAction::ExpandCandidates | KeyAction::CollapseCandidates) {
                th.text
            } else if k.action == KeyAction::SelectMode && self.selecting {
                th.accent
            } else {
                th.text_secondary
            };
            match &k.sub {
                // Toolbar tool: icon and caption side by side.
                Some(cap) => {
                    let ist = self.icon_style(17.0 * bs, color);
                    let cst = TextStyle { align: Align::Start, ..self.style(14.5 * bs, color) };
                    let iw = 20.0 * bs;
                    let gap = 5.0 * bs;
                    let tw = c.measure_text(cap, cst);
                    let x = k.cell.x + (k.cell.w - iw - gap - tw) / 2.0;
                    c.text(&k.label, Rect::new(x, k.cell.y, iw, k.cell.h), ist);
                    c.text(cap, Rect::new(x + iw + gap, k.cell.y, tw + 1.0, k.cell.h), cst);
                }
                None if k.icon => c.text(&k.label, k.cell, self.icon_style(19.0 * s * k.scale / 0.78, color)),
                // Text button (selection bar).
                None => {
                    let accent = k.tone == Tone::Accent;
                    let st = TextStyle { bold: accent, ..self.style(16.0 * bs, if accent { th.accent } else { th.text }) };
                    c.text(&k.label, k.cell, st);
                }
            }
        }
    }

    fn draw_strip(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = self.bar_s();
        let sr = self.strip_rect();
        let lead = self.strip_lead();
        let pre_h = if m.wide { 19.0 * s } else { 17.0 * s };
        let pre = TextStyle { align: Align::Start, ..self.style(if m.wide { 15.5 * s } else { 14.5 * s }, th.text_secondary) };
        c.text(&self.snapshot.preedit, Rect::new(lead + 2.0 * s, 2.0 * s, sr.w - lead, pre_h), pre);

        let y0 = pre_h + 1.0 * s;
        let h = m.bar_h - y0 - 2.0 * s;
        let pressed = self.pressed_index(|t| if let Target::Strip(i) = t { *i } else { None });
        c.push_clip(sr);
        let cs = self.cand_style();
        let cms = self.comment_style();
        for (i, &(x, w)) in self.layout_cache.strip.iter().enumerate() {
            let rx = sr.x + x - self.strip.offset;
            if rx + w < sr.x || rx > sr.x + sr.w {
                continue;
            }
            let Some(cand) = self.cands.get(i) else { break };
            let cell = Rect::new(rx, y0, w, h);
            if i == 0 && m.wide {
                // Wide bars make the default choice stand out with a soft pill.
                c.fill_rect(Rect::new(rx + 3.0 * s, y0 + 1.0 * s, w - 6.0 * s, h - 2.0 * s), 9.0 * s, th.accent_soft);
            }
            if pressed == Some(i) {
                c.fill_rect(cell.inset(1.0), 8.0 * s, th.flat_pressed);
            }
            let color = if i == 0 { th.accent } else { th.text };
            match &cand.comment {
                None => c.text(&cand.text, cell, TextStyle { color, align: Align::Center, bold: i == 0, ..cs }),
                Some(comment) => {
                    let tw = c.measure_text(&cand.text, cs);
                    let cw = c.measure_text(comment, cms);
                    let x0 = rx + (w - tw - 4.0 * s - cw) / 2.0;
                    c.text(&cand.text, Rect::new(x0, y0, tw + 1.0, h), TextStyle { color, bold: i == 0, ..cs });
                    c.text(comment, Rect::new(x0 + tw + 4.0 * s, y0 + 2.0 * s, cw + 1.0, h), cms);
                }
            }
        }
        c.pop_clip();
        // Soft fade into the chevron when more candidates are off to the right.
        if self.strip.offset < self.strip.max {
            let fw = 28.0 * s;
            for i in 0..6 {
                let a = (i as f32 + 1.0) / 6.0;
                let x = sr.x + sr.w - fw + fw * i as f32 / 6.0;
                c.fill_rect(Rect::new(x, y0, fw / 6.0 + 0.5, h), 0.0, th.background.with_alpha(a * 0.9));
            }
        }
        let dh = m.bar_h * 0.42;
        c.fill_rect(Rect::new(sr.x + sr.w, (m.bar_h - dh) / 2.0 + y0 / 3.0, 1.0, dh), 0.0, th.divider);
    }

    fn draw_candidate_grid(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let s = self.m.s;
        let Some(area) = self.grid else { return };
        let pressed = self.pressed_index(|t| if let Target::Grid(i) = t { *i } else { None });
        c.push_clip(area);
        let cs = self.cand_style();
        let cell_h = self.grid_cell_h();
        let mut last_row_y = f32::NAN;
        for (i, r) in self.layout_cache.grid.iter().enumerate() {
            let rect = Rect::new(area.x + r.x, area.y + r.y - self.grid_scroll.offset, r.w, r.h);
            if rect.y + rect.h < area.y || rect.y > area.y + area.h {
                continue;
            }
            let Some(cand) = self.cands.get(i) else { break };
            if rect.y != last_row_y {
                last_row_y = rect.y;
                c.fill_rect(Rect::new(area.x + 6.0 * s, rect.y + cell_h - 0.5, area.w - 12.0 * s, 1.0), 0.0, th.divider);
            }
            if pressed == Some(i) {
                c.fill_rect(rect.inset(3.0 * s), 8.0 * s, th.flat_pressed);
            }
            if rect.x + rect.w < area.x + area.w - 2.0 {
                let dh = cell_h * 0.36;
                c.fill_rect(Rect::new(rect.x + rect.w, rect.y + (cell_h - dh) / 2.0, 1.0, dh), 0.0, th.divider);
            }
            let color = if i == 0 { th.accent } else { th.text };
            let label = match &cand.comment {
                Some(comment) => format!("{} {}", cand.text, comment),
                None => cand.text.clone(),
            };
            c.text(&label, rect, TextStyle { color, align: Align::Center, bold: i == 0, ..cs });
        }
        c.pop_clip();
    }

    // -----------------------------------------------------------------------------------------
    // Panels
    // -----------------------------------------------------------------------------------------

    fn draw_column(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let Some((rect, kind)) = self.column else { return };
        let face = m.face(rect);
        c.fill_rect(Rect::new(face.x, face.y + 1.2 * s, face.w, face.h), m.radius, th.shadow);
        c.fill_rect(face, m.radius, th.func);
        let items = self.column_items();
        let item_h = self.column_item_h();
        let spelling = kind == ColumnKind::T9 && self.composing() && !self.spellings.is_empty();
        let size = if spelling { 18.0 * s } else { 20.0 * s };
        let pressed = self.pressed_index(|t| if let Target::Column(i) = t { *i } else { None });
        c.push_clip(face);
        for (i, item) in items.iter().enumerate() {
            let y = rect.y + i as f32 * item_h - self.column_scroll.offset;
            if y + item_h < face.y || y > face.y + face.h {
                continue;
            }
            let cell = Rect::new(face.x, y, face.w, item_h);
            if pressed == Some(i) {
                c.fill_rect(cell, 0.0, th.func_pressed);
            }
            if i > 0 {
                c.fill_rect(Rect::new(face.x + face.w * 0.2, y, face.w * 0.6, 1.0), 0.0, th.divider);
            }
            c.text(item, punct_centered(item, cell, size), self.style(size, th.text));
        }
        c.pop_clip();
    }

    fn draw_symbol_grid(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let (Panel::Symbols(tab), Some(grid)) = (self.panel, self.grid) else { return };
        let (cols, cell_h) = self.sym_cell(grid);
        let cell_w = grid.w / cols as f32;
        let pressed = self.pressed_index(|t| if let Target::SymGrid(i) = t { *i } else { None });
        let size = if tab == layout::SymTab::Emoji { 26.0 * s } else { 22.0 * s };
        c.push_clip(Rect::new(0.0, grid.y, m.w, grid.h));
        for (i, sym) in layout::symbols(tab).iter().enumerate() {
            let cell = Rect::new(
                grid.x + (i % cols) as f32 * cell_w,
                grid.y + (i / cols) as f32 * cell_h - self.sym_scroll.offset,
                cell_w,
                cell_h,
            );
            if cell.y + cell.h < grid.y || cell.y > grid.y + grid.h {
                continue;
            }
            let face = m.face(cell);
            let color = if pressed == Some(i) { th.key_pressed } else { th.key };
            c.fill_rect(Rect::new(face.x, face.y + 1.2 * s, face.w, face.h), m.radius, th.shadow);
            c.fill_rect(face, m.radius, color);
            c.text(sym, punct_centered(sym, face, size), self.style(size, th.text));
        }
        c.pop_clip();
    }

    fn draw_menu_title(&self, c: &mut dyn Canvas) {
        let m = &self.m;
        let area = m.keys_area();
        let st = self.style(13.5 * m.s, self.theme.text_secondary);
        c.text("选择键盘", Rect::new(area.x, area.y, area.w, m.row_h * 0.35), st);
    }

    // -----------------------------------------------------------------------------------------
    // Keys
    // -----------------------------------------------------------------------------------------

    fn draw_key(&self, c: &mut dyn Canvas, k: &Key) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let touch = self.press_of(k, false);
        let pressed = touch.is_some();
        let face = m.face(k.cell);
        if k.tone == Tone::Tile {
            return self.draw_tile(c, k, face, pressed);
        }
        let (bg, fg) = match k.tone {
            Tone::Char if pressed => (th.key_pressed, th.text),
            Tone::Char => (th.key, th.text),
            Tone::Func if pressed => (th.func_pressed, th.text),
            Tone::Accent if pressed => (th.accent_pressed, th.on_accent),
            Tone::Accent => (th.accent, th.on_accent),
            Tone::Active => (th.accent_soft, th.accent),
            _ => (th.func, th.text),
        };
        c.fill_rect(Rect::new(face.x, face.y + 1.3 * s, face.w, face.h), m.radius, th.shadow);
        c.fill_rect(face, m.radius, bg);

        match k.action {
            KeyAction::ToggleChinese => return self.draw_toggle(c, face),
            KeyAction::Space => return self.draw_space(c, k, face, touch),
            _ => {}
        }
        let size = m.letter_size() * k.scale;
        let mods = self.mods();
        let locked = match k.action {
            KeyAction::Mod(md) => mods.latch(md) == Latch::Locked,
            KeyAction::CapsLock => mods.latch(Modifier::Shift) == Latch::Locked,
            _ => false,
        };
        if locked {
            let w = 14.0 * s;
            c.fill_rect(Rect::new(face.x + (face.w - w) / 2.0, face.y + face.h * 0.78, w, 2.0 * s), 1.0 * s, th.accent);
        }
        if k.action == KeyAction::Raw(dianmo_core::KeyCode::CapsLock) {
            // Caps Lock light.
            let d = 6.0 * s;
            let color = if self.pc_caps { th.accent } else { th.divider };
            c.fill_rect(Rect::new(face.x + face.w - d - 7.0 * s, face.y + 7.0 * s, d, d), d / 2.0, color);
        }
        if let Some(tl) = &k.top_left {
            let r = Rect::new(face.x + 6.0 * s, face.y + 2.0 * s, face.w * 0.5, 17.0 * s);
            c.text(tl, r, TextStyle { align: Align::Start, ..self.style(13.0 * s, th.text_secondary) });
        }
        if k.icon {
            let color = match k.action {
                KeyAction::Mod(Modifier::Shift) if mods.on(Modifier::Shift) => th.accent,
                _ => fg,
            };
            c.text(&k.label, face, self.icon_style(size, color));
        } else if let Some(sub) = &k.sub {
            // 小鹤: letter above, finals below.
            let main = Rect::new(face.x, face.y + face.h * 0.04, face.w, face.h * 0.62);
            c.text(&k.label, main, self.style(size * 0.92, fg));
            let sub_r = Rect::new(face.x + 2.0, face.y + face.h * 0.60, face.w - 4.0, face.h * 0.32);
            // Shortcut hints (Ctrl+C 复制) are in the accent colour; 小鹤 finals are grey.
            let color = if mods.chording() { th.accent } else { th.text_secondary };
            c.text(sub, sub_r, self.style(12.5 * s, color));
        } else if let Some(top) = &k.top {
            // T9: digit above, letters below.
            let top_r = Rect::new(face.x, face.y + face.h * 0.10, face.w, face.h * 0.30);
            c.text(top, top_r, self.style(12.5 * s, th.text_secondary));
            let main = Rect::new(face.x, face.y + face.h * 0.36, face.w, face.h * 0.52);
            c.text(&k.label, main, TextStyle { bold: false, ..self.style(size, fg) });
        } else {
            let r = Rect::new(face.x, face.y - 1.0 * s, face.w, face.h);
            c.text(&k.label, punct_centered(&k.label, r, size), self.style(size, fg));
        }
        if let Some(corner) = &k.corner {
            let cw = 24.0 * s;
            let r = Rect::new(face.x + face.w - cw - 5.0 * s, face.y + 3.0 * s, cw, 17.0 * s);
            c.text(corner, r, TextStyle { align: Align::End, ..self.style(12.5 * s, th.text_faint) });
        }
    }

    fn draw_toggle(&self, c: &mut dyn Canvas, face: Rect) {
        let th = &self.theme;
        let s = self.m.s;
        let big = self.style(19.0 * s, th.text);
        let small = self.style(13.0 * s, th.text_faint);
        let (a, b) = if self.chinese { (("中", big), ("英", small)) } else { (("中", small), ("英", big)) };
        let slash = ("/", self.style(12.0 * s, th.text_faint));
        let parts = [a, slash, b];
        let widths: Vec<f32> = parts.iter().map(|(t, st)| c.measure_text(t, *st)).collect();
        let total: f32 = widths.iter().sum::<f32>() + 2.0 * s;
        let mut x = face.x + (face.w - total) / 2.0;
        for ((t, st), w) in parts.iter().zip(widths) {
            let y_off = if st.size < 15.0 * s { 2.0 * s } else { 0.0 };
            c.text(t, Rect::new(x, face.y + y_off, w + 1.0, face.h), TextStyle { align: Align::Start, ..*st });
            x += w + 1.0 * s;
        }
    }

    fn draw_space(&self, c: &mut dyn Canvas, k: &Key, face: Rect, touch: Option<&Touch>) {
        let th = &self.theme;
        let s = self.m.s;
        match touch.map(|t| &t.mode) {
            Some(Mode::Cursor { .. }) => {
                c.text("‹   移动光标   ›", face, self.style(14.0 * s, th.accent));
            }
            _ => {
                if !k.label.is_empty() {
                    c.text(&k.label, face, self.style(13.0 * s, th.text_faint));
                }
            }
        }
    }

    fn draw_tile(&self, c: &mut dyn Canvas, k: &Key, face: Rect, pressed: bool) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let face = Rect::new(face.x + 4.0 * s, face.y, face.w - 8.0 * s, face.h);
        let bg = if pressed { th.key_pressed } else { th.key };
        c.fill_rect(Rect::new(face.x, face.y + 1.3 * s, face.w, face.h), m.radius * 1.5, th.shadow);
        c.fill_rect(face, m.radius * 1.5, bg);
        let fg = if k.selected { th.accent } else { th.text };
        if k.selected {
            c.stroke_rect(face.inset(1.0 * s), m.radius * 1.5, 2.0 * s, th.accent);
        }
        let glyph = Rect::new(face.x, face.y + face.h * 0.12, face.w, face.h * 0.5);
        if k.icon {
            c.text(&k.label, glyph, self.icon_style(30.0 * s, fg));
        } else {
            c.text(&k.label, glyph, TextStyle { bold: true, ..self.style(34.0 * s, fg) });
        }
        if let Some(sub) = &k.sub {
            let r = Rect::new(face.x, face.y + face.h * 0.62, face.w, face.h * 0.26);
            c.text(sub, r, self.style(15.0 * s, if k.selected { th.accent } else { th.text_secondary }));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Overlays
    // -----------------------------------------------------------------------------------------

    fn draw_overlay(&self, c: &mut dyn Canvas, t: &Touch) {
        if t.consumed {
            return;
        }
        let Target::Key(k) = &t.target else { return };
        match &t.mode {
            Mode::Press if k.bubble && self.key_popup => {
                let label = match &k.action {
                    KeyAction::Letter(ch) if self.mods().on(Modifier::Shift) => ch.to_ascii_uppercase().to_string(),
                    KeyAction::Letter(ch) => ch.to_string(),
                    _ => k.label.clone(),
                };
                self.draw_bubble(c, k, &label, false);
            }
            Mode::SwipeUp => {
                if let Some(sec) = &k.secondary {
                    self.draw_bubble(c, k, sec, true);
                }
            }
            Mode::ClearArmed => {
                self.draw_bubble(c, k, "清空", true);
            }
            Mode::Long { alts, sel, popup, cell_w, .. } => self.draw_popup(c, alts, *sel, *popup, *cell_w),
            _ => {}
        }
    }

    fn bubble_body(&self, c: &mut dyn Canvas, r: Rect) {
        let th = &self.theme;
        let s = self.m.s;
        let rad = self.m.radius * 1.6;
        c.fill_rect(Rect::new(r.x - 1.0 * s, r.y + 1.0 * s, r.w + 2.0 * s, r.h + 3.0 * s), rad + 1.0, th.bubble_shadow.with_alpha(th.bubble_shadow.a * 0.5));
        c.fill_rect(Rect::new(r.x, r.y + 1.5 * s, r.w, r.h), rad, th.bubble_shadow);
        c.fill_rect(r, rad, th.bubble);
    }

    fn draw_bubble(&self, c: &mut dyn Canvas, k: &Key, text: &str, accent: bool) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let face = m.face(k.cell);
        let bw = (face.w * 0.9).clamp(m.row_h * 1.0, m.row_h * 1.35);
        let bh = m.row_h * 1.18;
        let bx = (face.x + face.w / 2.0 - bw / 2.0).clamp(2.0, m.w - bw - 2.0);
        let by = (face.y + face.h * 0.32 - bh).max(1.0);
        let r = Rect::new(bx, by, bw, bh);
        self.bubble_body(c, r);
        let n = text.chars().count().max(1) as f32;
        let size = if n <= 1.0 { 40.0 * s } else { (36.0 * s * 1.6 / n).clamp(16.0 * s, 26.0 * s) };
        let tr = punct_centered(text, Rect::new(r.x, r.y - 1.0 * s, r.w, r.h), size);
        c.text(text, tr, self.style(size, if accent { th.accent } else { th.text }));
    }

    fn draw_popup(&self, c: &mut dyn Canvas, alts: &[String], sel: usize, popup: Rect, cell_w: f32) {
        let th = &self.theme;
        let s = self.m.s;
        self.bubble_body(c, popup);
        for (i, a) in alts.iter().enumerate() {
            let cell = Rect::new(popup.x + i as f32 * cell_w, popup.y, cell_w, popup.h);
            let selected = i == sel;
            if selected {
                c.fill_rect(cell.inset(4.0 * s), self.m.radius * 1.2, th.accent);
            }
            let n = a.chars().count().max(1) as f32;
            let size = if n <= 1.0 { 28.0 * s } else { (28.0 * s * 1.5 / n).clamp(14.0 * s, 22.0 * s) };
            c.text(a, punct_centered(a, cell, size), self.style(size, if selected { th.on_accent } else { th.text }));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Selection, trackpad, clipboard (TODO #32)
    // -----------------------------------------------------------------------------------------

    /// The key area as a trackpad: key faces hidden, a hint, and the finger.
    fn draw_trackpad(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let area = m.keys_area();
        let face = area.inset(4.0 * s);
        c.fill_rect(face, m.radius * 2.0, th.func);
        if self.pad_select {
            c.stroke_rect(face.inset(1.0 * s), m.radius * 2.0, 2.0 * s, th.accent);
        }
        let (title, hint) = if self.pad_select {
            ("选择中", "拖动扩大选区 · 再用另一根手指点一下，按词扩选 · 松手结束")
        } else {
            ("触控板", "拖动移动光标，拖得快走得远 · 另一根手指点一下开始选择")
        };
        let mid = face.y + face.h * 0.42;
        c.text(title, Rect::new(face.x, mid - 34.0 * s, face.w, 30.0 * s), TextStyle { bold: true, ..self.style(22.0 * s, if self.pad_select { th.accent } else { th.text_secondary }) });
        c.text(hint, Rect::new(face.x, mid + 2.0 * s, face.w, 24.0 * s), self.style(14.5 * s, th.text_faint));
        for t in &self.touches {
            if let Mode::Trackpad { .. } = t.mode {
                let d = 34.0 * s;
                c.fill_rect(Rect::new(t.x - d / 2.0, t.y - d / 2.0, d, d), d / 2.0, th.accent.with_alpha(0.25));
                c.fill_rect(Rect::new(t.x - d / 4.0, t.y - d / 4.0, d / 2.0, d / 2.0), d / 4.0, th.accent.with_alpha(0.6));
            }
        }
    }

    /// Clipboard cards in the idle bar (most recent first).
    fn draw_clip_bar(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let s = self.bar_s();
        let rect = self.clip_bar_rect();
        let pressed = self.pressed_index(|t| if let Target::ClipCard(i) = t { *i } else { None });
        let st = self.clip_text_style();
        let h = self.m.bar_h * 0.7;
        let y = (self.m.bar_h - h) / 2.0;
        c.push_clip(rect);
        for (i, (x, w, text)) in self.clip_cards().into_iter().enumerate() {
            let rx = rect.x + x - self.clip_strip.offset;
            if rx + w < rect.x || rx > rect.x + rect.w {
                continue;
            }
            let card = Rect::new(rx, y, w, h);
            c.fill_rect(Rect::new(card.x, card.y + 1.0 * s, card.w, card.h), 9.0 * s, th.shadow);
            c.fill_rect(card, 9.0 * s, if pressed == Some(i) { th.key_pressed } else { th.key });
            if i == 0 {
                c.stroke_rect(card.inset(0.5 * s), 9.0 * s, 1.5 * s, th.accent.with_alpha(0.6));
            }
            c.text(&text, card.inset(6.0 * s), st);
        }
        c.pop_clip();
    }

    /// Splits `text` (one line, see [`clip_preview`]) into at most `lines` lines of width `w`.
    fn wrap_lines(text: &str, size: f32, w: f32, lines: usize) -> Vec<String> {
        let mut out: Vec<String> = vec![String::new()];
        let mut width = 0.0;
        for ch in text.chars() {
            let cw = estimate_width(ch.encode_utf8(&mut [0; 4]), size);
            if width + cw > w && !out.last().is_some_and(|l| l.is_empty()) {
                if out.len() == lines {
                    let last = out.last_mut().unwrap();
                    last.pop();
                    last.push('…');
                    return out;
                }
                out.push(String::new());
                width = 0.0;
            }
            out.last_mut().unwrap().push(ch);
            width += cw;
        }
        out
    }

    /// Clipboard panel: title, and the cards (pinned first). Long-pressed card: 固定 / 删除.
    fn draw_clip_panel(&self, c: &mut dyn Canvas) {
        let th = &self.theme;
        let m = &self.m;
        let s = m.s;
        let area = m.keys_area();
        let head_h = m.row_h * 0.85;
        let title = if self.clips.is_empty() { "剪贴板" } else { "剪贴板 · 点一下粘贴，长按固定或删除" };
        c.text(title, Rect::new(area.x, area.y, area.w, head_h), self.style(15.0 * s, th.text_secondary));
        let Some(grid) = self.grid else { return };
        if self.clips.is_empty() {
            c.text("复制的文字会出现在这里", grid, self.style(16.0 * s, th.text_faint));
            return;
        }
        let pressed = self.pressed_index(|t| if let Target::ClipGrid(i) = t { *i } else { None });
        let size = 16.0 * s;
        c.push_clip(grid);
        for (pos, &i) in self.clip_panel_order().iter().enumerate() {
            let cell = self.clip_grid_cell(grid, pos);
            if cell.y + cell.h < grid.y || cell.y > grid.y + grid.h {
                continue;
            }
            let item = &self.clips[i];
            let face = Rect::new(cell.x + 4.0 * s, cell.y + 4.0 * s, cell.w - 8.0 * s, cell.h - 8.0 * s);
            c.fill_rect(Rect::new(face.x, face.y + 1.3 * s, face.w, face.h), m.radius * 1.4, th.shadow);
            c.fill_rect(face, m.radius * 1.4, if pressed == Some(pos) { th.key_pressed } else { th.key });
            if self.clip_menu == Some(item.id) {
                // 固定 / 删除 halves.
                let half = face.w / 2.0;
                let l = Rect::new(face.x, face.y, half, face.h);
                let r = Rect::new(face.x + half, face.y, half, face.h);
                c.fill_rect(l.inset(4.0 * s), m.radius, th.accent_soft);
                c.fill_rect(r.inset(4.0 * s), m.radius, th.func);
                c.text(if item.pinned { "取消固定" } else { "固定" }, l, TextStyle { bold: true, ..self.style(16.0 * s, th.accent) });
                c.text("删除", r, TextStyle { bold: true, ..self.style(16.0 * s, th.text) });
                continue;
            }
            let pad = 12.0 * s;
            let text = clip_preview(&item.text, 120);
            let lines = Self::wrap_lines(&text, size, face.w - 2.0 * pad - if item.pinned { 18.0 * s } else { 0.0 }, 2);
            let lh = size * 1.45;
            let y0 = face.y + (face.h - lh * lines.len() as f32) / 2.0;
            for (j, line) in lines.iter().enumerate() {
                let r = Rect::new(face.x + pad, y0 + j as f32 * lh, face.w - 2.0 * pad, lh);
                c.text(line, r, TextStyle { align: Align::Start, ..self.style(size, th.text) });
            }
            if item.pinned {
                let r = Rect::new(face.x + face.w - 22.0 * s, face.y + 4.0 * s, 18.0 * s, 18.0 * s);
                c.text(layout::icon::PIN, r, self.icon_style(13.0 * s, th.accent));
            }
        }
        c.pop_clip();
    }

    /// 「已复制」: a dark pill over the keys.
    fn draw_toast(&self, c: &mut dyn Canvas) {
        let Some((text, _)) = &self.toast else { return };
        let th = &self.theme;
        let s = self.bar_s();
        let st = TextStyle { bold: true, ..self.style(16.0 * s, th.background) };
        let w = c.measure_text(text, st) + 40.0 * s;
        // Over the keys like a phone toast, so the new clipboard card in the bar stays visible.
        let h = (self.m.bar_h * 0.8).max(32.0);
        let area = self.m.keys_area();
        let r = Rect::new((self.m.w - w) / 2.0, area.y + area.h * 0.38 - h / 2.0, w, h);
        c.fill_rect(Rect::new(r.x, r.y + 2.0 * s, r.w, r.h), h / 2.0, th.bubble_shadow);
        c.fill_rect(r, h / 2.0, th.text.with_alpha(0.9));
        c.text(text, r, st);
    }
}

/// The microphone key while voice input runs (same red in both themes).
const RECORDING: Color = Color::rgb(0xE5, 0x48, 0x4D);
const RECORDING_PRESSED: Color = Color::rgb(0xC2, 0x36, 0x3B);

/// Full-width punctuation whose ink sits in the lower-left quarter of the em box (，。、．) looks
/// off-centre on a key when the advance is centred. Phone keyboards centre the ink; so do we by
/// shifting the text rect (measured on Microsoft YaHei UI on the Surface).
pub(crate) fn punct_centered(text: &str, r: Rect, size: f32) -> Rect {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some('，' | '。' | '、' | '．'), None) => Rect::new(r.x + 0.24 * size, r.y - 0.24 * size, r.w, r.h),
        _ => r,
    }
}
