use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Clone, Copy, Debug)]
pub struct DrawerLayout {
    /// Final resting position (flush against the top edge of the taskbar).
    pub shown_x: i32,
    pub shown_y: i32,
    /// Off-screen starting position the drawer slides up from.
    pub hidden_x: i32,
    pub hidden_y: i32,
}

/// Computes where the drawer window should sit: bottom-center of the work
/// area of whichever monitor the cursor is currently on (i.e. resting on top
/// of the taskbar of the screen the user is actually looking at - not always
/// the primary monitor, which matters on multi-monitor setups), and the
/// off-screen position it animates from.
pub fn compute_layout(window_w: i32, window_h: i32) -> DrawerLayout {
    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut cursor);
        let hmonitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work: RECT = if GetMonitorInfoW(hmonitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        };

        let work_w = work.right - work.left;
        let shown_x = work.left + (work_w - window_w) / 2;
        let shown_y = work.bottom - window_h;

        DrawerLayout {
            shown_x,
            shown_y,
            hidden_x: shown_x,
            hidden_y: work.bottom + 4,
        }
    }
}
