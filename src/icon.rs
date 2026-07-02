use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

/// Extracts the large icon associated with an executable and returns
/// (BGRA8 pixels, width, height) - the byte order GDI+/Windows DIBs use
/// natively, so callers can hand this straight to `Surface::draw_bgra_image`
/// every frame with no per-frame conversion. Returns `None` on any failure.
pub fn extract_icon_bgra(exe_path: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
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

        // Already BGRA (DrawIconEx's native order) - no channel swap needed.
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
