use windows::core::PCWSTR;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetObjectW, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIBSECTION, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::System::Com::{CoInitializeEx, IBindCtx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    SHCreateItemFromParsingName, SHGetFileInfoW, IShellItemImageFactory, SHFILEINFOW, SHGFI_ICON,
    SHGFI_LARGEICON, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

/// Extracts the icon associated with an executable at `size` x `size` pixels
/// and returns (BGRA8 pixels, width, height) - the byte order GDI+/Windows
/// DIBs use natively, so callers can hand this straight to
/// `Surface::draw_bgra_image`. Returns `None` on any failure.
///
/// The primary path renders the icon through the shell's image factory (the
/// same one Explorer uses), which produces a crisp image at exactly the
/// requested size by scaling from the best-matching resolution embedded in
/// the exe. That's what keeps icons sharp instead of the blurry look you get
/// from grabbing the fixed 32px shell icon and stretching it up.
pub fn extract_icon_bgra(exe_path: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    shell_image(exe_path, size).or_else(|| legacy_icon(exe_path, size))
}

/// High-quality path: `IShellItemImageFactory::GetImage` at the exact target
/// size.
fn shell_image(exe_path: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        // GetImage needs COM on this thread. It's already initialised by the
        // time we render (winit does it), but initialising again is cheap and
        // idempotent - we deliberately never uninitialise.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>).ok()?;
        let hbitmap = factory
            .GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY)
            .ok()?;
        let out = bitmap_to_bgra(hbitmap);
        let _ = DeleteObject(hbitmap.into());
        out
    }
}

/// Copies a 32bpp premultiplied-BGRA HBITMAP (as returned by `GetImage`) into
/// a straight-alpha, top-down BGRA buffer our draw path can consume. Reads the
/// DIB header's `biHeight` sign to know the source orientation and flips a
/// bottom-up DIB, rather than assuming - `GetImage` hands back a bottom-up DIB
/// on some systems, which showed up as upside-down icons.
unsafe fn bitmap_to_bgra(hbitmap: HBITMAP) -> Option<(Vec<u8>, u32, u32)> {
    let mut ds = DIBSECTION::default();
    let n = unsafe {
        GetObjectW(
            hbitmap.into(),
            std::mem::size_of::<DIBSECTION>() as i32,
            Some(&mut ds as *mut _ as *mut _),
        )
    };
    // A DIB section returns the full DIBSECTION; anything smaller means we
    // couldn't read the header we need.
    if n < std::mem::size_of::<DIBSECTION>() as i32
        || ds.dsBm.bmBits.is_null()
        || ds.dsBm.bmBitsPixel != 32
    {
        return None;
    }
    let w = ds.dsBm.bmWidth.max(0) as usize;
    let h = ds.dsBm.bmHeight.unsigned_abs() as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let stride = ds.dsBm.bmWidthBytes as usize;
    let src = ds.dsBm.bmBits as *const u8;
    // biHeight < 0 => top-down (memory row 0 is the visual top); biHeight > 0
    // => bottom-up (memory row 0 is the visual bottom, so read in reverse).
    let bottom_up = ds.dsBmih.biHeight > 0;

    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_row = if bottom_up { h - 1 - y } else { y };
        unsafe {
            std::ptr::copy_nonoverlapping(
                src.add(src_row * stride),
                out.as_mut_ptr().add(y * w * 4),
                w * 4,
            );
        }
    }

    // GetImage hands back premultiplied alpha; undo it so the straight-alpha
    // ARGB draw path doesn't double-darken the anti-aliased edges.
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            px[0] = ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[1] = ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[2] = ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }

    Some((out, w as u32, h as u32))
}

/// Fallback: the classic shell large-icon (~32px) drawn into a `size` DIB.
/// Lower quality, but a safety net for anything the image factory can't
/// resolve.
fn legacy_icon(exe_path: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut shfi = SHFILEINFOW::default();
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if shfi.hIcon.is_invalid() {
            return None;
        }
        let hicon = shfi.hIcon;

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // negative = top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm = match CreateDIBSection(
            Some(hdc_mem),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        ) {
            Ok(h) => h,
            Err(_) => {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(None, hdc_screen);
                let _ = DestroyIcon(hicon);
                return None;
            }
        };
        let old_bm = SelectObject(hdc_mem, hbm.into());

        let byte_len = (size as usize) * (size as usize) * 4;
        std::ptr::write_bytes(bits_ptr as *mut u8, 0, byte_len);

        let _ = DrawIconEx(hdc_mem, 0, 0, hicon, size, size, 0, None, DI_NORMAL);

        let mut buf = vec![0u8; byte_len];
        std::ptr::copy_nonoverlapping(bits_ptr as *const u8, buf.as_mut_ptr(), byte_len);

        let _ = SelectObject(hdc_mem, old_bm);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);
        let _ = DestroyIcon(hicon);

        // Some legacy icons don't carry a real alpha channel; if the whole
        // buffer came back fully transparent, treat any non-black pixel as
        // opaque so the icon doesn't disappear.
        let any_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
        if !any_alpha {
            for px in buf.chunks_exact_mut(4) {
                if px[0] != 0 || px[1] != 0 || px[2] != 0 {
                    px[3] = 255;
                }
            }
        }

        Some((buf, size as u32, size as u32))
    }
}
