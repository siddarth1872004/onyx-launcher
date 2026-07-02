use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
};
use windows::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON};

/// Standard icon sizes to bake in, covering taskbar, Start menu tiles, and
/// Explorer's large-icon view.
const SIZES: [u32; 4] = [16, 32, 48, 256];

/// Builds a multi-resolution ICO byte buffer from an arbitrary source image
/// (PNG/JPEG/BMP/ICO/...). If the source is already a valid ICO, it's passed
/// through unchanged.
pub fn build_ico(image_bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    if let Ok(existing) = ico::IconDir::read(std::io::Cursor::new(image_bytes)) {
        if !existing.entries().is_empty() {
            let mut out = Vec::new();
            existing.write(&mut out)?;
            return Ok(out);
        }
    }

    let source = image::load_from_memory(image_bytes)?.to_rgba8();
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in SIZES {
        let resized = image::imageops::resize(
            &source,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        );
        let image = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&image)?);
    }
    let mut out = Vec::new();
    icon_dir.write(&mut out)?;
    Ok(out)
}

struct IcoEntry {
    width: u8,
    height: u8,
    color_count: u8,
    planes: u16,
    bit_count: u16,
    data: Vec<u8>,
}

fn parse_ico(bytes: &[u8]) -> anyhow::Result<Vec<IcoEntry>> {
    anyhow::ensure!(bytes.len() >= 6, "ICO data too short");
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let off = 6 + i * 16;
        anyhow::ensure!(bytes.len() >= off + 16, "ICO directory entry truncated");
        let width = bytes[off];
        let height = bytes[off + 1];
        let color_count = bytes[off + 2];
        let planes = u16::from_le_bytes([bytes[off + 4], bytes[off + 5]]);
        let bit_count = u16::from_le_bytes([bytes[off + 6], bytes[off + 7]]);
        let bytes_in_res = u32::from_le_bytes([
            bytes[off + 8],
            bytes[off + 9],
            bytes[off + 10],
            bytes[off + 11],
        ]) as usize;
        let image_offset = u32::from_le_bytes([
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
            bytes[off + 15],
        ]) as usize;
        anyhow::ensure!(
            bytes.len() >= image_offset + bytes_in_res,
            "ICO image data truncated"
        );
        entries.push(IcoEntry {
            width,
            height,
            color_count,
            planes,
            bit_count,
            data: bytes[image_offset..image_offset + bytes_in_res].to_vec(),
        });
    }
    Ok(entries)
}

fn build_group_icon(entries: &[IcoEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + entries.len() * 14);
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (i, entry) in entries.iter().enumerate() {
        out.push(entry.width);
        out.push(entry.height);
        out.push(entry.color_count);
        out.push(0); // reserved
        out.extend_from_slice(&entry.planes.to_le_bytes());
        out.extend_from_slice(&entry.bit_count.to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&((i + 1) as u16).to_le_bytes()); // resource ID
    }
    out
}

/// Replaces (or adds) the icon resource of the .exe at `exe_path` with the
/// icon described by `ico_bytes` (a standard multi-image .ico file).
pub fn patch_exe_icon(exe_path: &Path, ico_bytes: &[u8]) -> anyhow::Result<()> {
    let entries = parse_ico(ico_bytes)?;
    anyhow::ensure!(!entries.is_empty(), "no images found in icon data");
    let group_icon = build_group_icon(&entries);

    let wide: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = BeginUpdateResourceW(PCWSTR(wide.as_ptr()), false)?;

        for (i, entry) in entries.iter().enumerate() {
            let id = PCWSTR(((i + 1) as u16) as *const u16);
            UpdateResourceW(
                handle,
                RT_ICON,
                id,
                0,
                Some(entry.data.as_ptr() as *const core::ffi::c_void),
                entry.data.len() as u32,
            )?;
        }

        UpdateResourceW(
            handle,
            RT_GROUP_ICON,
            PCWSTR(1u16 as *const u16),
            0,
            Some(group_icon.as_ptr() as *const core::ffi::c_void),
            group_icon.len() as u32,
        )?;

        EndUpdateResourceW(handle, false)?;
    }

    Ok(())
}
