//! Contract between the keyboard UI and the platform window (implemented with Direct2D /
//! DirectWrite in `dianmo-win`). All coordinates are DIPs (device-independent pixels) with the
//! origin at the window's top-left; the platform applies the DPI scale.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn inset(&self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
}

/// Straight (non-premultiplied) RGBA, 0.0..=1.0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Font {
    /// "Microsoft YaHei UI" (falls back to the system CJK font).
    #[default]
    Ui,
    /// Icon font: "Segoe Fluent Icons" if present, else "Segoe MDL2 Assets" (Windows 10).
    /// Draw icons by passing their code point as text, e.g. "\u{E720}" (microphone).
    Icon,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in DIPs.
    pub size: f32,
    pub color: Color,
    /// Horizontal alignment inside the rect; text is always vertically centred.
    pub align: Align,
    pub bold: bool,
    pub font: Font,
}

pub trait Canvas {
    fn clear(&mut self, color: Color);
    /// Filled rectangle with rounded corners (`radius` 0 = square).
    fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color);
    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color);
    /// Single-line text, clipped to `rect`, vertically centred.
    fn text(&mut self, text: &str, rect: Rect, style: TextStyle);
    /// Width in DIPs of `text` drawn with `style` (alignment and colour ignored).
    fn measure_text(&mut self, text: &str, style: TextStyle) -> f32;
    fn push_clip(&mut self, rect: Rect);
    fn pop_clip(&mut self);
}
