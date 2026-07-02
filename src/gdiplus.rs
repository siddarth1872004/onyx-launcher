//! Minimal safe-ish wrapper around GDI+ for rendering the drawer's UI onto a
//! layered window. This deliberately replaces a GPU-accelerated renderer
//! (OpenGL/egui) with GDI+, which is a plain system DLL every Windows
//! process can use without loading a dedicated GPU driver stack - the
//! tradeoff is we lose egui's widget conveniences and have to hand-roll
//! hit-testing, text input, and layout ourselves in `app.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

// GDI+ pixel format values from gdiplusenums.h. windows-rs doesn't expose
// these (they're C macros, not part of the Win32 metadata), so they're
// hardcoded here using their well-known, stable values.
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

pub enum TextAlign {
    Near,
    Center,
}

/// RAII handle for the process-wide GDI+ runtime. Create exactly one and
/// keep it alive for as long as any other GDI+ call might happen.
pub struct GdiplusToken(usize);

impl GdiplusToken {
    pub fn init() -> Self {
        unsafe {
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                ..Default::default()
            };
            let mut token: usize = 0;
            let mut output = GdiplusStartupOutput::default();
            GdiplusStartup(&mut token, &input, &mut output);
            Self(token)
        }
    }
}

impl Drop for GdiplusToken {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.0) }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An off-screen 32bpp ARGB surface that GDI+ draws into directly (it wraps
/// the same memory as a DIB section), which is then handed to
/// `UpdateLayeredWindow` in one shot for real per-pixel-alpha compositing.
pub struct Surface {
    width: i32,
    height: i32,
    hbitmap: HBITMAP,
    mem_dc: HDC,
    old_bitmap: HGDIOBJ,
    bits_ptr: *mut u8,
    gp_bitmap: *mut GpBitmap,
    gp_graphics: *mut GpGraphics,
    font_family: *mut GpFontFamily,
    // A single reusable brush (color set per draw via `GdipSetSolidFillColor`)
    // and pre-built string formats, so hot render paths avoid a
    // create/destroy round trip to GDI+ for every fill and text draw call -
    // there can be dozens of these per frame while animating.
    brush: *mut GpSolidFill,
    format_near: *mut GpStringFormat,
    format_center: *mut GpStringFormat,
    fonts: HashMap<u32, *mut GpFont>,
    // Per-icon GDI+ bitmaps, keyed by the app's path. Built once and reused
    // across frames instead of recreating a bitmap for every visible tile on
    // every animation frame. Each wraps (does not copy) the caller's pixel
    // buffer, so the buffer must outlive the cached bitmap - the app disposes
    // via `invalidate_image` before dropping the backing pixels.
    images: HashMap<String, *mut GpBitmap>,
}

impl Surface {
    pub fn new(width: i32, height: i32) -> Self {
        unsafe {
            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            ReleaseDC(None, screen_dc);

            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };

            let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbitmap =
                CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
                    .expect("CreateDIBSection failed");
            let old_bitmap = SelectObject(mem_dc, hbitmap.into());

            let mut gp_bitmap: *mut GpBitmap = std::ptr::null_mut();
            GdipCreateBitmapFromScan0(
                width,
                height,
                width * 4,
                PIXEL_FORMAT_32BPP_PARGB,
                Some(bits_ptr as *const u8),
                &mut gp_bitmap,
            );

            let mut gp_graphics: *mut GpGraphics = std::ptr::null_mut();
            GdipGetImageGraphicsContext(gp_bitmap as *mut GpImage, &mut gp_graphics);
            // High-quality shape anti-aliasing with a half-pixel offset so
            // rounded-rect edges land cleanly on the pixel grid instead of
            // looking jagged.
            GdipSetSmoothingMode(gp_graphics, SmoothingModeAntiAlias);
            GdipSetPixelOffsetMode(gp_graphics, PixelOffsetModeHalf);
            // Grid-fitted grayscale AA: snaps glyph stems to the pixel grid,
            // which is what keeps Segoe UI crisp at small sizes instead of the
            // soft/blurry look plain AntiAlias gives on a layered window.
            // (ClearType is deliberately avoided - subpixel AA produces broken
            // alpha / colour fringing on a per-pixel-alpha layered surface.)
            GdipSetTextRenderingHint(gp_graphics, TextRenderingHintAntiAliasGridFit);
            // Crisp icon downscaling (48px source -> ~36px tile).
            GdipSetInterpolationMode(gp_graphics, InterpolationModeHighQualityBicubic);

            let mut font_family: *mut GpFontFamily = std::ptr::null_mut();
            let name = wide("Segoe UI");
            GdipCreateFontFamilyFromName(PCWSTR(name.as_ptr()), std::ptr::null_mut(), &mut font_family);

            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(0, &mut brush);

            let mut format_near: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut format_near);
            GdipSetStringFormatAlign(format_near, StringAlignmentNear);
            GdipSetStringFormatLineAlign(format_near, StringAlignmentCenter);

            let mut format_center: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut format_center);
            GdipSetStringFormatAlign(format_center, StringAlignmentCenter);
            GdipSetStringFormatLineAlign(format_center, StringAlignmentCenter);

            Self {
                width,
                height,
                hbitmap,
                mem_dc,
                old_bitmap,
                bits_ptr: bits_ptr as *mut u8,
                gp_bitmap,
                gp_graphics,
                font_family,
                brush,
                format_near,
                format_center,
                fonts: HashMap::new(),
                images: HashMap::new(),
            }
        }
    }

    /// Returns a cached `GpFont` for this pixel size, creating it on first
    /// use. There are only a handful of distinct sizes used across a whole
    /// render pass, so this turns what would be a create+destroy per text
    /// draw (every frame) into a one-time cost per size for the life of the
    /// surface.
    fn font_for(&mut self, size_px: f32) -> *mut GpFont {
        *self.fonts.entry(size_px.to_bits()).or_insert_with(|| unsafe {
            let mut font: *mut GpFont = std::ptr::null_mut();
            GdipCreateFont(self.font_family, size_px, FontStyleRegular.0, UnitPixel, &mut font);
            font
        })
    }

    /// Resets the whole surface to fully transparent black.
    pub fn clear(&mut self) {
        unsafe {
            let len = (self.width as usize) * (self.height as usize) * 4;
            std::ptr::write_bytes(self.bits_ptr, 0, len);
        }
    }

    pub fn fill_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, argb: u32) {
        unsafe {
            let mut path: *mut GpPath = std::ptr::null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            let d = radius * 2.0;
            GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);

            GdipSetSolidFillColor(self.brush, argb);
            GdipFillPath(self.gp_graphics, self.brush as *mut GpBrush, path);
            GdipDeletePath(path);
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        unsafe {
            GdipSetSolidFillColor(self.brush, argb);
            GdipFillRectangle(self.gp_graphics, self.brush as *mut GpBrush, x, y, w, h);
        }
    }

    pub fn set_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        unsafe {
            GdipSetClipRect(self.gp_graphics, x, y, w, h, CombineModeReplace);
        }
    }

    pub fn reset_clip(&mut self) {
        unsafe {
            GdipResetClip(self.gp_graphics);
        }
    }

    pub fn draw_text(
        &mut self,
        text: &str,
        rect: (f32, f32, f32, f32),
        size_px: f32,
        argb: u32,
        align: TextAlign,
    ) {
        let font = self.font_for(size_px);
        let format = match align {
            TextAlign::Near => self.format_near,
            TextAlign::Center => self.format_center,
        };
        unsafe {
            GdipSetSolidFillColor(self.brush, argb);

            let layout = RectF {
                X: rect.0,
                Y: rect.1,
                Width: rect.2,
                Height: rect.3,
            };
            let wtext = wide(text);
            GdipDrawString(
                self.gp_graphics,
                PCWSTR(wtext.as_ptr()),
                -1,
                font,
                &layout,
                format,
                self.brush as *mut GpBrush,
            );
        }
    }

    /// Draws a BGRA image (as produced by `icon::extract_icon_bgra`, already
    /// in GDI+'s native byte order) scaled into `dst`, caching the wrapping
    /// `GpBitmap` under `key`. The bitmap wraps `bgra` without copying, so the
    /// caller must keep that buffer alive until it calls `invalidate_image`.
    pub fn draw_bgra_image(
        &mut self,
        key: &str,
        dst: (f32, f32, f32, f32),
        bgra: &[u8],
        src_w: u32,
        src_h: u32,
    ) {
        unsafe {
            let bitmap = *self.images.entry(key.to_string()).or_insert_with(|| {
                let mut bitmap: *mut GpBitmap = std::ptr::null_mut();
                GdipCreateBitmapFromScan0(
                    src_w as i32,
                    src_h as i32,
                    src_w as i32 * 4,
                    PIXEL_FORMAT_32BPP_ARGB,
                    Some(bgra.as_ptr()),
                    &mut bitmap,
                );
                bitmap
            });
            GdipDrawImageRect(
                self.gp_graphics,
                bitmap as *mut GpImage,
                dst.0,
                dst.1,
                dst.2,
                dst.3,
            );
        }
    }

    /// Disposes the cached bitmap for `key` (if any). Must be called before
    /// the pixel buffer that bitmap wraps is freed.
    pub fn invalidate_image(&mut self, key: &str) {
        if let Some(bitmap) = self.images.remove(key) {
            unsafe { GdipDisposeImage(bitmap as *mut GpImage) };
        }
    }

    /// Composites the fully-rendered surface onto `hwnd` at `(x, y)` using
    /// real per-pixel alpha (`UpdateLayeredWindow`), which is what lets DWM's
    /// blur-behind/backdrop material show through the transparent regions.
    pub fn present(&self, hwnd: HWND, x: i32, y: i32) {
        let pos = POINT { x, y };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let src_pos = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pos),
                Some(&size),
                Some(self.mem_dc),
                Some(&src_pos),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            for font in self.fonts.values() {
                GdipDeleteFont(*font);
            }
            for bitmap in self.images.values() {
                GdipDisposeImage(*bitmap as *mut GpImage);
            }
            GdipDeleteStringFormat(self.format_near);
            GdipDeleteStringFormat(self.format_center);
            GdipDeleteBrush(self.brush as *mut GpBrush);
            GdipDeleteFontFamily(self.font_family);
            GdipDeleteGraphics(self.gp_graphics);
            GdipDisposeImage(self.gp_bitmap as *mut GpImage);
            SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(self.hbitmap.into());
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

/// Keeps the [`Surface`] and the GDI+ runtime token bundled so both can be
/// stored as a single field.
pub struct Renderer {
    _token: Arc<GdiplusToken>,
    pub surface: Surface,
}

impl Renderer {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            _token: Arc::new(GdiplusToken::init()),
            surface: Surface::new(width, height),
        }
    }
}
