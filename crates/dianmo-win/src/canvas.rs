//! [`dianmo_ui::Canvas`] on Direct2D + DirectWrite.
//!
//! Pipeline: D3D11 device → DXGI flip-model swap chain *for composition* →
//! `ID2D1DeviceContext` draws into the back buffer → DirectComposition shows it in the window.
//! The window uses `WS_EX_NOREDIRECTIONBITMAP`, so there is no GDI redirection surface (a full
//! width keyboard at 200% is ~8 MB per surface), and DirectComposition is already wired for the
//! key-pop bubbles and slide animations planned for M2.
//!
//! The D3D11 device is WARP (software) by default. Measured on the Surface (Intel Iris Xe,
//! 2880×520 px keyboard): the hardware driver stack (igc64/igd10um64xe) adds ~47 MB of private
//! memory (56 MB vs 9.8 MB) while a full repaint costs the same ~6–7 ms CPU either way, so the
//! GPU buys nothing for a keyboard. `HostOptions::hardware_gpu` opts into the driver.
//!
//! Device loss (`D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED/RESET`) drops every device
//! resource; the next paint recreates them.

use std::collections::HashMap;

use dianmo_ui::{Align, Canvas, Color, Font, Rect, TextStyle};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED, D3D11_SDK_VERSION, D3D11CreateDevice,
    ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::core::{BOOL, HSTRING, Interface, Result, w};

const D2DERR_RECREATE_TARGET: windows::core::HRESULT = windows::core::HRESULT(0x8899000C_u32 as i32);

/// Owns the factories (process lifetime) and the device resources (recreated after loss).
pub(crate) struct Renderer {
    hwnd: HWND,
    hardware: bool,
    d2d: ID2D1Factory1,
    text: TextCache,
    dev: Option<Device>,
    size: (u32, u32),
    dpi: f32,
}

struct Device {
    swap: IDXGISwapChain1,
    dc: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    comp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    _d3d: ID3D11Device,
}

impl Renderer {
    /// `hardware`: use the GPU driver instead of WARP (see the module docs for the trade-off).
    /// The `DIANMO_D3D=hardware|warp` environment variable overrides it.
    pub(crate) fn new(hwnd: HWND, hardware: bool) -> Result<Self> {
        let hardware = match std::env::var("DIANMO_D3D").as_deref() {
            Ok("hardware") => true,
            Ok("warp") => false,
            _ => hardware,
        };
        let d2d: ID2D1Factory1 = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let icon_family = pick_icon_family(&dwrite);
        Ok(Self {
            hwnd,
            hardware,
            d2d,
            text: TextCache { dwrite, icon_family, formats: HashMap::new(), widths: HashMap::new() },
            dev: None,
            size: (0, 0),
            dpi: 96.0,
        })
    }

    /// Back-buffer size in physical pixels and the DPI used to map DIPs onto it.
    pub(crate) fn resize(&mut self, width: u32, height: u32, dpi: f32) {
        if (width, height, dpi) == (self.size.0, self.size.1, self.dpi) {
            return;
        }
        self.size = (width, height);
        self.dpi = dpi;
        if let Some(dev) = &self.dev
            && (width == 0 || height == 0 || dev.retarget(width, height, dpi).is_err())
        {
            self.dev = None;
        }
    }

    /// Draws one frame. `Ok(false)` means the device was lost and dropped: paint again.
    pub(crate) fn render(&mut self, paint: impl FnOnce(&mut dyn Canvas)) -> Result<bool> {
        let (w, h) = self.size;
        if w == 0 || h == 0 {
            return Ok(true);
        }
        if self.dev.is_none() {
            self.dev = Some(Device::new(self.hwnd, &self.d2d, self.hardware, w, h, self.dpi)?);
        }
        let dev = self.dev.as_ref().unwrap();
        unsafe { dev.dc.BeginDraw() };
        paint(&mut Frame { dc: &dev.dc, brush: &dev.brush, text: &mut self.text, clips: 0 });
        let end = unsafe { dev.dc.EndDraw(None, None) };
        let lost = match end {
            Err(e) if e.code() == D2DERR_RECREATE_TARGET => true,
            Err(e) => return Err(e),
            Ok(()) => {
                let hr = unsafe { dev.swap.Present(1, DXGI_PRESENT(0)) };
                if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
                    true
                } else {
                    hr.ok()?;
                    false
                }
            }
        };
        if lost {
            self.dev = None;
        }
        Ok(!lost)
    }
}

impl Device {
    fn new(hwnd: HWND, d2d: &ID2D1Factory1, hardware: bool, w: u32, h: u32, dpi: f32) -> Result<Self> {
        let d3d = if hardware { create_d3d(D3D_DRIVER_TYPE_HARDWARE) } else { Err(windows::core::Error::empty()) }
            .or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        unsafe {
            let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w,
                Height: h,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            let swap = factory.CreateSwapChainForComposition(&d3d, &desc, None)?;
            let dc = d2d.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, None)?;
            let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = comp.CreateTargetForHwnd(hwnd, true)?;
            let visual = comp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            let dev = Self { swap, dc, brush, comp, _target: target, _visual: visual, _d3d: d3d };
            dev.bind_target(dpi)?;
            dev.comp.Commit()?;
            Ok(dev)
        }
    }

    fn bind_target(&self, dpi: f32) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bitmap = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.dc.SetTarget(&bitmap);
            self.dc.SetDpi(dpi, dpi);
        }
        Ok(())
    }

    fn retarget(&self, w: u32, h: u32, dpi: f32) -> Result<()> {
        unsafe {
            self.dc.SetTarget(None);
            self.swap.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        }
        self.bind_target(dpi)
    }
}

fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

fn pick_icon_family(dwrite: &IDWriteFactory) -> HSTRING {
    let mut fonts: Option<IDWriteFontCollection> = None;
    unsafe {
        let (mut index, mut exists) = (0u32, BOOL(0));
        if dwrite.GetSystemFontCollection(&mut fonts, false).is_ok()
            && let Some(fonts) = fonts
            && fonts.FindFamilyName(w!("Segoe Fluent Icons"), &mut index, &mut exists).is_ok()
            && exists.as_bool()
        {
            return HSTRING::from("Segoe Fluent Icons");
        }
    }
    HSTRING::from("Segoe MDL2 Assets")
}

/// Text formats keyed by (font, bold, size in 1/8 DIP) and a bounded width cache.
struct TextCache {
    dwrite: IDWriteFactory,
    icon_family: HSTRING,
    formats: HashMap<(u8, bool, u32), IDWriteTextFormat>,
    widths: HashMap<(String, u8, bool, u32), f32>,
}

// `Font` doesn't derive `Hash`.
fn font_key(font: Font) -> u8 {
    match font {
        Font::Ui => 0,
        Font::Icon => 1,
    }
}

fn size_key(size: f32) -> u32 {
    (size.max(1.0) * 8.0).round() as u32
}

impl TextCache {
    fn format(&mut self, style: &TextStyle) -> Option<IDWriteTextFormat> {
        let key = (font_key(style.font), style.bold, size_key(style.size));
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        let family: HSTRING = match style.font {
            Font::Ui => HSTRING::from("Microsoft YaHei UI"),
            Font::Icon => self.icon_family.clone(),
        };
        let weight = if style.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        let format = unsafe {
            let f = self
                .dwrite
                .CreateTextFormat(
                    &family,
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    key.2 as f32 / 8.0,
                    w!("zh-cn"),
                )
                .ok()?;
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            f
        };
        self.formats.insert(key, format.clone());
        Some(format)
    }

    fn measure(&mut self, text: &str, style: &TextStyle) -> f32 {
        let key = (text.to_owned(), font_key(style.font), style.bold, size_key(style.size));
        if let Some(&w) = self.widths.get(&key) {
            return w;
        }
        let Some(format) = self.format(style) else { return 0.0 };
        let _ = unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING) };
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let width = unsafe {
            self.dwrite.CreateTextLayout(&utf16, &format, 1.0e6, 1.0e4).ok().and_then(|layout| {
                let mut m = DWRITE_TEXT_METRICS::default();
                layout.GetMetrics(&mut m).ok().map(|_| m.widthIncludingTrailingWhitespace)
            })
        }
        .unwrap_or(0.0);
        if self.widths.len() >= 1024 {
            self.widths.clear();
        }
        self.widths.insert(key, width);
        width
    }
}

/// One frame's drawing surface, handed to `View::paint`.
struct Frame<'a> {
    dc: &'a ID2D1DeviceContext,
    brush: &'a ID2D1SolidColorBrush,
    text: &'a mut TextCache,
    clips: u32,
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        // A view that forgets pop_clip must not break EndDraw.
        for _ in 0..self.clips {
            unsafe { self.dc.PopAxisAlignedClip() };
        }
    }
}

fn d2d_color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn d2d_rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

impl Frame<'_> {
    fn brush(&self, color: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&d2d_color(color)) };
        self.brush
    }
}

impl Canvas for Frame<'_> {
    fn clear(&mut self, color: Color) {
        unsafe { self.dc.Clear(Some(&d2d_color(color))) };
    }

    fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        let brush = self.brush(color);
        unsafe {
            if radius > 0.0 {
                let rr = D2D1_ROUNDED_RECT { rect: d2d_rect(rect), radiusX: radius, radiusY: radius };
                self.dc.FillRoundedRectangle(&rr, brush);
            } else {
                self.dc.FillRectangle(&d2d_rect(rect), brush);
            }
        }
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        let brush = self.brush(color);
        // Keep the stroke inside `rect`.
        let r = rect.inset(width / 2.0);
        unsafe {
            if radius > 0.0 {
                let rr = D2D1_ROUNDED_RECT {
                    rect: d2d_rect(r),
                    radiusX: (radius - width / 2.0).max(0.0),
                    radiusY: (radius - width / 2.0).max(0.0),
                };
                self.dc.DrawRoundedRectangle(&rr, brush, width, None);
            } else {
                self.dc.DrawRectangle(&d2d_rect(r), brush, width, None);
            }
        }
    }

    fn text(&mut self, text: &str, rect: Rect, style: TextStyle) {
        if text.is_empty() || rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let Some(format) = self.text.format(&style) else { return };
        let align: DWRITE_TEXT_ALIGNMENT = match style.align {
            Align::Start => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::End => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let brush = self.brush(style.color);
        unsafe {
            let _ = format.SetTextAlignment(align);
            self.dc.DrawText(
                &utf16,
                &format,
                &d2d_rect(rect),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn measure_text(&mut self, text: &str, style: TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.text.measure(text, &style)
    }

    fn push_clip(&mut self, rect: Rect) {
        unsafe { self.dc.PushAxisAlignedClip(&d2d_rect(rect), D2D1_ANTIALIAS_MODE_ALIASED) };
        self.clips += 1;
    }

    fn pop_clip(&mut self) {
        if self.clips > 0 {
            unsafe { self.dc.PopAxisAlignedClip() };
            self.clips -= 1;
        }
    }
}
