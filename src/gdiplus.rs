//! Minimal safe-ish wrapper around GDI+ for rendering the drawer's UI onto a
//! layered window. This deliberately replaces a GPU-accelerated renderer
//! (OpenGL/egui) with GDI+, which is a plain system DLL every Windows
//! process can use without loading a dedicated GPU driver stack - the
//! tradeoff is we lose egui's widget conveniences and have to hand-roll
//! hit-testing, text input, and layout ourselves in `app.rs`.

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
            GdipSetSmoothingMode(gp_graphics, SmoothingModeAntiAlias);
            GdipSetTextRenderingHint(gp_graphics, TextRenderingHintAntiAlias);

            let mut font_family: *mut GpFontFamily = std::ptr::null_mut();
            let name = wide("Segoe UI");
            GdipCreateFontFamilyFromName(PCWSTR(name.as_ptr()), std::ptr::null_mut(), &mut font_family);

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
            }
        }
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

            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(argb, &mut brush);
            GdipFillPath(self.gp_graphics, brush as *mut GpBrush, path);
            GdipDeleteBrush(brush as *mut GpBrush);
            GdipDeletePath(path);
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        unsafe {
            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(argb, &mut brush);
            GdipFillRectangle(self.gp_graphics, brush as *mut GpBrush, x, y, w, h);
            GdipDeleteBrush(brush as *mut GpBrush);
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
        unsafe {
            let mut font: *mut GpFont = std::ptr::null_mut();
            GdipCreateFont(self.font_family, size_px, FontStyleRegular.0, UnitPixel, &mut font);

            let mut format: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut format);
            GdipSetStringFormatAlign(
                format,
                match align {
                    TextAlign::Near => StringAlignmentNear,
                    TextAlign::Center => StringAlignmentCenter,
                },
            );
            // Vertically center within the layout rect; GDI+ defaults to
            // top-aligned otherwise, which looks off in a pill/tile.
            GdipSetStringFormatLineAlign(format, StringAlignmentCenter);

            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(argb, &mut brush);

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
                brush as *mut GpBrush,
            );

            GdipDeleteBrush(brush as *mut GpBrush);
            GdipDeleteStringFormat(format);
            GdipDeleteFont(font);
        }
    }

    pub fn measure_text_width(&mut self, text: &str, size_px: f32) -> f32 {
        unsafe {
            let mut font: *mut GpFont = std::ptr::null_mut();
            GdipCreateFont(self.font_family, size_px, FontStyleRegular.0, UnitPixel, &mut font);
            let mut format: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut format);

            let layout = RectF {
                X: 0.0,
                Y: 0.0,
                Width: 10_000.0,
                Height: 200.0,
            };
            let mut bounds = RectF::default();
            let mut fitted = 0i32;
            let mut lines = 0i32;
            let wtext = wide(text);
            GdipMeasureString(
                self.gp_graphics,
                PCWSTR(wtext.as_ptr()),
                -1,
                font,
                &layout,
                format,
                &mut bounds,
                &mut fitted,
                &mut lines,
            );
            GdipDeleteStringFormat(format);
            GdipDeleteFont(font);
            bounds.Width
        }
    }

    /// Draws a straight (non-premultiplied), RGBA-ordered image (as produced
    /// by `icon::extract_icon_rgba`) scaled into `dst`.
    pub fn draw_rgba_image(&mut self, dst: (f32, f32, f32, f32), rgba: &[u8], src_w: u32, src_h: u32) {
        // GDI+ / Windows DIBs expect BGRA byte order; flip R and B.
        let mut bgra = rgba.to_vec();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        unsafe {
            let mut bitmap: *mut GpBitmap = std::ptr::null_mut();
            GdipCreateBitmapFromScan0(
                src_w as i32,
                src_h as i32,
                src_w as i32 * 4,
                PIXEL_FORMAT_32BPP_ARGB,
                Some(bgra.as_ptr()),
                &mut bitmap,
            );
            GdipDrawImageRect(
                self.gp_graphics,
                bitmap as *mut GpImage,
                dst.0,
                dst.1,
                dst.2,
                dst.3,
            );
            GdipDisposeImage(bitmap as *mut GpImage);
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
