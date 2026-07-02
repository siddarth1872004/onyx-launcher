use onyx_launcher::{icon, resource_icon};

fn synthetic_test_png() -> Vec<u8> {
    let mut img = image::RgbaImage::new(64, 64);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let dx = x as i32 - 32;
        let dy = y as i32 - 32;
        let inside_circle = dx * dx + dy * dy < 28 * 28;
        *px = if inside_circle {
            image::Rgba([80, 180, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        };
    }
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode test png");
    bytes
}

/// End-to-end check: build an ICO from an arbitrary PNG, copy the built exe,
/// patch its icon resource, then read the icon back out with the same
/// SHGetFileInfoW-based extraction Explorer/the taskbar use. If this comes
/// back with real (non-empty) pixel data, the patched resource is valid.
#[test]
fn patch_and_read_back_icon() {
    let src_exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_onyx-launcher"));
    let dest_exe = std::env::temp_dir().join("onyx_launcher_icon_patch_test.exe");
    std::fs::copy(&src_exe, &dest_exe).expect("copy exe");

    let png_bytes = synthetic_test_png();

    let ico_bytes = resource_icon::build_ico(&png_bytes).expect("build ico");
    assert!(ico_bytes.len() > 6, "ico bytes should be non-trivial");

    resource_icon::patch_exe_icon(&dest_exe, &ico_bytes).expect("patch icon");

    let (bgra, w, h) = icon::extract_icon_bgra(dest_exe.to_str().unwrap(), 48)
        .expect("should be able to extract the icon we just embedded");
    assert_eq!((w, h), (48, 48));
    let any_opaque = bgra.chunks_exact(4).any(|px| px[3] > 0);
    assert!(any_opaque, "extracted icon should have visible pixels");

    let _ = std::fs::remove_file(&dest_exe);
}
