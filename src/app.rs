use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HGLOBAL, HWND, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetWindowLongPtrW, SetWindowLongPtrW,
    TrackPopupMenu, GWL_EXSTYLE, MF_STRING, TPM_NONOTIFY, TPM_RETURNCMD, WS_EX_LAYERED,
};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::{BackdropType, CornerPreference, WindowExtWindows};
use winit::window::{Window, WindowLevel};

use crate::config::Config;
use crate::gdiplus::{Renderer, TextAlign};
use crate::geometry::{self, DrawerLayout};
use crate::icon;
use crate::ipc;

/// Wakes the event loop from another thread (the IPC listener) without
/// needing any GPU/egui machinery.
pub enum UserEvent {
    Wake,
}

const ANIM_MS: f32 = 260.0;
const HOVER_ANIM_MS: f32 = 120.0;

// Logical (96-DPI) pixel sizes; actual on-screen sizes are these times
// `scale`, so the drawer looks the same physical size on any display scaling.
const LOGICAL_WINDOW_W: f32 = 760.0;
const LOGICAL_WINDOW_H: f32 = 460.0;
const LOGICAL_TILE_W: f32 = 84.0;
const LOGICAL_TILE_H: f32 = 92.0;
const LOGICAL_GAP: f32 = 10.0;
const LOGICAL_PADDING: f32 = 20.0;
const LOGICAL_REMOVE_BADGE: f32 = 18.0;
const LOGICAL_SEARCH_W: f32 = 360.0;
const LOGICAL_SEARCH_H: f32 = 36.0;
const LOGICAL_ICON: f32 = 36.0;
const LOGICAL_GRID_TOP: f32 = 96.0;
const LOGICAL_CORNER_RADIUS: f32 = 14.0;
const LOGICAL_SCROLLBAR_W: f32 = 4.0;

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn ease_out_smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * std::f32::consts::FRAC_PI_2).sin()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// All layout metrics pre-scaled once at startup for the display's DPI.
struct Metrics {
    scale: f32,
    window_w: f32,
    window_h: f32,
    tile_w: f32,
    tile_h: f32,
    gap: f32,
    padding: f32,
    remove_badge: f32,
    search_w: f32,
    search_h: f32,
    icon: f32,
    grid_top: f32,
    corner_radius: f32,
    scrollbar_w: f32,
}

impl Metrics {
    fn compute(scale: f32) -> Self {
        Self {
            scale,
            window_w: LOGICAL_WINDOW_W * scale,
            window_h: LOGICAL_WINDOW_H * scale,
            tile_w: LOGICAL_TILE_W * scale,
            tile_h: LOGICAL_TILE_H * scale,
            gap: LOGICAL_GAP * scale,
            padding: LOGICAL_PADDING * scale,
            remove_badge: LOGICAL_REMOVE_BADGE * scale,
            search_w: LOGICAL_SEARCH_W * scale,
            search_h: LOGICAL_SEARCH_H * scale,
            icon: LOGICAL_ICON * scale,
            grid_top: LOGICAL_GRID_TOP * scale,
            corner_radius: LOGICAL_CORNER_RADIUS * scale,
            scrollbar_w: LOGICAL_SCROLLBAR_W * scale,
        }
    }

    fn font(&self, logical_px: f32) -> f32 {
        logical_px * self.scale
    }

    fn columns(&self) -> usize {
        (((self.window_w - 2.0 * self.padding + self.gap) / (self.tile_w + self.gap))
            .floor()
            .max(1.0)) as usize
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Hovered {
    None,
    App(usize),
    AppRemove(usize),
    Add,
}

enum DrawerState {
    Hidden,
    SlidingUp { start: Instant },
    Shown,
    SlidingDown { start: Instant },
}

struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

pub struct DrawerApp {
    window: Arc<Window>,
    hwnd: HWND,
    renderer: Renderer,
    metrics: Metrics,
    config: Config,
    active_category: Option<String>,
    icons: HashMap<String, (Vec<u8>, u32, u32)>,
    search: String,
    state: DrawerState,
    layout: DrawerLayout,
    pos: (i32, i32),
    // Kept alive so the hotkey stays registered; unregistered automatically on drop.
    #[allow(dead_code)]
    hotkey_manager: GlobalHotKeyManager,
    hotkey_id: u32,
    request_rx: Receiver<Option<String>>,
    hovered: Hovered,
    mouse: (f32, f32),
    focused: bool,
    ctrl_held: bool,
    scroll: f32,
    hover_alpha: HashMap<String, f32>,
    last_tick: Instant,
    frame_interval: std::time::Duration,
}

fn hwnd_of(window: &Window) -> HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().expect("window handle").as_raw() {
        RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut _),
        _ => unreachable!("windows-only build"),
    }
}

impl DrawerApp {
    pub fn new(
        event_loop: &ActiveEventLoop,
        listener: TcpListener,
        category: Option<String>,
        proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    ) -> Self {
        let primary_monitor = event_loop.primary_monitor();
        let scale = primary_monitor
            .as_ref()
            .map(|m| m.scale_factor() as f32)
            .unwrap_or(1.0);
        let metrics = Metrics::compute(scale);

        // Pace animation frames to the real display refresh rate instead of
        // busy-polling as fast as possible, which reads as jittery.
        let refresh_millihertz = primary_monitor
            .as_ref()
            .and_then(|m| m.refresh_rate_millihertz())
            .unwrap_or(60_000);
        let frame_interval =
            std::time::Duration::from_secs_f64(1000.0 / refresh_millihertz as f64);

        let window_w = metrics.window_w.round() as i32;
        let window_h = metrics.window_h.round() as i32;
        let layout = geometry::compute_layout(window_w, window_h);

        let title = match &category {
            Some(name) => format!("Onyx Launcher - {name}"),
            None => "Onyx Launcher".to_string(),
        };

        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::PhysicalSize::new(window_w as u32, window_h as u32))
            .with_position(PhysicalPosition::new(layout.hidden_x, layout.hidden_y))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop);

        let window = Arc::new(event_loop.create_window(attrs).expect("failed to create window"));
        let hwnd = hwnd_of(&window);

        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as isize);
        }

        window.set_system_backdrop(BackdropType::TransientWindow);
        window.set_corner_preference(CornerPreference::Round);
        window.set_skip_taskbar(true);
        // DWM draws a thin accent-colored 1px border around rounded-corner
        // windows by default; we don't want that outline, just the rounding.
        window.set_border_color(None);

        let (tx, rx) = std::sync::mpsc::channel();
        ipc::spawn_listener(listener, tx, move || {
            let _ = proxy.send_event(UserEvent::Wake);
        });

        let hotkey_manager =
            GlobalHotKeyManager::new().expect("failed to initialize global hotkey manager");
        let hotkey = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
        let hotkey_id = hotkey.id();
        let _ = hotkey_manager.register(hotkey);

        let mut app = Self {
            window,
            hwnd,
            renderer: Renderer::new(window_w, window_h),
            metrics,
            config: Config::load(category.as_deref()),
            active_category: category.clone(),
            icons: HashMap::new(),
            search: String::new(),
            state: DrawerState::Hidden,
            layout,
            pos: (layout.hidden_x, layout.hidden_y),
            hotkey_manager,
            hotkey_id,
            request_rx: rx,
            hovered: Hovered::None,
            mouse: (0.0, 0.0),
            focused: false,
            ctrl_held: false,
            scroll: 0.0,
            hover_alpha: HashMap::new(),
            last_tick: Instant::now(),
            frame_interval,
        };
        app.request(category, event_loop);
        app
    }

    fn set_pos(&mut self, x: i32, y: i32) {
        self.pos = (x, y);
        self.window.set_outer_position(PhysicalPosition::new(x, y));
    }

    /// Schedules the next animation tick at the display's true refresh
    /// interval, rather than busy-polling as fast as the OS will allow
    /// (which produces uneven, jittery frame delivery).
    fn schedule_next_frame(&self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            Instant::now() + self.frame_interval,
        ));
    }

    fn toggle_visibility(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        match self.state {
            DrawerState::Hidden => {
                self.set_pos(self.layout.hidden_x, self.layout.hidden_y);
                self.state = DrawerState::SlidingUp { start: now };
                self.render();
                self.window.set_visible(true);
            }
            DrawerState::Shown | DrawerState::SlidingUp { .. } => {
                self.state = DrawerState::SlidingDown { start: now };
            }
            DrawerState::SlidingDown { .. } => {
                self.state = DrawerState::SlidingUp { start: now };
            }
        }
        self.schedule_next_frame(event_loop);
        self.render();
    }

    fn switch_category(&mut self, category: Option<String>) {
        self.config = Config::load(category.as_deref());
        self.icons.clear();
        self.search.clear();
        self.hovered = Hovered::None;
        self.hover_alpha.clear();
        self.scroll = 0.0;
        let title = match &category {
            Some(name) => format!("Onyx Launcher - {name}"),
            None => "Onyx Launcher".to_string(),
        };
        self.window.set_title(&title);
        self.active_category = category;
    }

    fn request(&mut self, category: Option<String>, event_loop: &ActiveEventLoop) {
        if category == self.active_category {
            self.toggle_visibility(event_loop);
            return;
        }
        self.switch_category(category);
        match self.state {
            DrawerState::Hidden => self.toggle_visibility(event_loop),
            DrawerState::SlidingDown { .. } => {
                self.state = DrawerState::SlidingUp { start: Instant::now() };
                self.schedule_next_frame(event_loop);
            }
            DrawerState::Shown | DrawerState::SlidingUp { .. } => {}
        }
        self.render();
    }

    fn get_icon(&mut self, path: &str) -> Option<(Vec<u8>, u32, u32)> {
        if let Some(icon) = self.icons.get(path) {
            return Some(icon.clone());
        }
        let icon = icon::extract_icon_rgba(path, 48)?;
        self.icons.insert(path.to_string(), icon.clone());
        Some(icon)
    }

    fn tile_rect(&self, index: usize) -> Rect {
        let m = &self.metrics;
        let columns = m.columns();
        let col = index % columns;
        let row = index / columns;
        Rect {
            x: m.padding + col as f32 * (m.tile_w + m.gap),
            y: m.grid_top + row as f32 * (m.tile_h + m.gap) - self.scroll,
            w: m.tile_w,
            h: m.tile_h,
        }
    }

    fn content_area(&self) -> Rect {
        let m = &self.metrics;
        Rect {
            x: 0.0,
            y: m.grid_top,
            w: m.window_w,
            h: m.window_h - m.grid_top - m.padding * 0.5,
        }
    }

    fn content_height(&self, tile_count: usize) -> f32 {
        let m = &self.metrics;
        let columns = m.columns();
        let rows = tile_count.div_ceil(columns).max(1);
        rows as f32 * (m.tile_h + m.gap) - m.gap
    }

    fn max_scroll(&self, tile_count: usize) -> f32 {
        (self.content_height(tile_count) - self.content_area().h).max(0.0)
    }

    fn clamp_scroll(&mut self, tile_count: usize) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(tile_count));
    }

    fn filtered_apps(&self) -> Vec<crate::config::AppEntry> {
        let search_lower = self.search.to_lowercase();
        self.config
            .apps
            .iter()
            .filter(|a| search_lower.is_empty() || a.name.to_lowercase().contains(&search_lower))
            .cloned()
            .collect()
    }

    fn hit_test(&self, x: f32, y: f32) -> Hovered {
        let content = self.content_area();
        if !content.contains(x, y) {
            return Hovered::None;
        }
        let apps = self.filtered_apps();
        for (i, _) in apps.iter().enumerate() {
            let r = self.tile_rect(i);
            if r.contains(x, y) {
                let badge = Rect {
                    x: r.x + r.w - self.metrics.remove_badge,
                    y: r.y,
                    w: self.metrics.remove_badge,
                    h: self.metrics.remove_badge,
                };
                return if badge.contains(x, y) {
                    Hovered::AppRemove(i)
                } else {
                    Hovered::App(i)
                };
            }
        }
        let add_rect = self.tile_rect(apps.len());
        if add_rect.contains(x, y) {
            return Hovered::Add;
        }
        Hovered::None
    }

    fn show_remove_menu(&self) -> bool {
        unsafe {
            let menu = CreatePopupMenu().expect("popup menu");
            let label: Vec<u16> = "Remove\0".encode_utf16().collect();
            let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(label.as_ptr()));
            let mut pt = POINT {
                x: self.mouse.0 as i32,
                y: self.mouse.1 as i32,
            };
            let _ = ClientToScreen(self.hwnd, &mut pt);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                pt.x,
                pt.y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
            cmd.as_bool()
        }
    }

    fn paste_from_clipboard(&mut self) {
        unsafe {
            if OpenClipboard(Some(self.hwnd)).is_err() {
                return;
            }
            if let Ok(handle) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
                let hglobal = HGLOBAL(handle.0);
                let ptr = GlobalLock(hglobal) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    let text = String::from_utf16_lossy(slice);
                    for c in text.chars().filter(|c| !c.is_control()) {
                        self.search.push(c);
                    }
                    let _ = GlobalUnlock(hglobal);
                }
            }
            let _ = CloseClipboard();
        }
    }

    /// Eases every tile's hover-highlight value toward its target and reports
    /// whether anything is still mid-transition (so the caller knows whether
    /// to keep polling for more animation frames).
    fn tick_hover(&mut self, dt_ms: f32) -> bool {
        let target_key: Option<String> = match self.hovered {
            Hovered::App(i) | Hovered::AppRemove(i) => {
                self.filtered_apps().get(i).map(|a| a.path.clone())
            }
            Hovered::Add => Some("__add__".to_string()),
            Hovered::None => None,
        };

        let step = (dt_ms / HOVER_ANIM_MS).clamp(0.0, 1.0);
        let mut still_animating = false;

        if let Some(key) = &target_key {
            let v = self.hover_alpha.entry(key.clone()).or_insert(0.0);
            *v = lerp(*v, 1.0, step);
            if (*v - 1.0).abs() > 0.003 {
                still_animating = true;
            }
        }
        self.hover_alpha.retain(|k, v| {
            if Some(k.as_str()) != target_key.as_deref() {
                *v = lerp(*v, 0.0, step);
                if *v > 0.003 {
                    still_animating = true;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        });

        still_animating
    }

    fn hover_value(&self, key: &str) -> f32 {
        self.hover_alpha.get(key).copied().unwrap_or(0.0)
    }

    fn render(&mut self) {
        self.renderer.surface.clear();
        let m_window_w = self.metrics.window_w;
        let m_window_h = self.metrics.window_h;

        self.renderer.surface.fill_rounded_rect(
            0.0,
            0.0,
            m_window_w,
            m_window_h,
            self.metrics.corner_radius,
            argb(244, 2, 2, 2),
        );
        self.renderer.surface.fill_rect(
            self.metrics.padding * 0.7,
            0.0,
            m_window_w - self.metrics.padding * 1.4,
            self.metrics.scale.max(1.0),
            argb(30, 255, 255, 255),
        );

        // Search pill.
        let search_x = (m_window_w - self.metrics.search_w) / 2.0;
        let search_y = self.metrics.padding;
        self.renderer.surface.fill_rounded_rect(
            search_x,
            search_y,
            self.metrics.search_w,
            self.metrics.search_h,
            self.metrics.search_h / 2.0,
            argb(16, 255, 255, 255),
        );
        let display_text = if self.search.is_empty() {
            "Search apps...".to_string()
        } else {
            format!("{}\u{2502}", self.search)
        };
        let text_color = if self.search.is_empty() {
            argb(140, 255, 255, 255)
        } else {
            argb(255, 255, 255, 255)
        };
        self.renderer.surface.draw_text(
            &display_text,
            (
                search_x + self.metrics.padding * 0.8,
                search_y,
                self.metrics.search_w - self.metrics.padding * 1.6,
                self.metrics.search_h,
            ),
            self.metrics.font(14.0),
            text_color,
            TextAlign::Near,
        );

        let apps = self.filtered_apps();
        self.clamp_scroll(apps.len() + 1);

        let content = self.content_area();
        self.renderer
            .surface
            .set_clip_rect(content.x, content.y, content.w, content.h);

        for (i, app_entry) in apps.iter().enumerate() {
            let r = self.tile_rect(i);
            if r.y + r.h < content.y || r.y > content.y + content.h {
                continue;
            }
            let hover_t = self.hover_value(&app_entry.path);
            let fill_alpha = lerp(10.0, 28.0, hover_t) as u8;
            self.renderer.surface.fill_rounded_rect(
                r.x,
                r.y,
                r.w,
                r.h,
                self.metrics.corner_radius,
                argb(fill_alpha, 255, 255, 255),
            );

            if let Some((rgba, iw, ih)) = self.get_icon(&app_entry.path) {
                let icon_size = self.metrics.icon;
                self.renderer.surface.draw_rgba_image(
                    (
                        r.x + (r.w - icon_size) / 2.0,
                        r.y + 8.0 * self.metrics.scale,
                        icon_size,
                        icon_size,
                    ),
                    &rgba,
                    iw,
                    ih,
                );
            }

            self.renderer.surface.draw_text(
                &truncate(&app_entry.name, 12),
                (r.x, r.y + r.h - 26.0 * self.metrics.scale, r.w, 16.0 * self.metrics.scale),
                self.metrics.font(11.0),
                argb(220, 255, 255, 255),
                TextAlign::Center,
            );

            if hover_t > 0.05 {
                self.renderer.surface.draw_text(
                    "\u{2715}",
                    (
                        r.x + r.w - self.metrics.remove_badge,
                        r.y,
                        self.metrics.remove_badge,
                        self.metrics.remove_badge,
                    ),
                    self.metrics.font(10.0),
                    argb((200.0 * hover_t) as u8, 255, 255, 255),
                    TextAlign::Center,
                );
            }
        }

        let add_rect = self.tile_rect(apps.len());
        let add_hover_t = self.hover_value("__add__");
        self.renderer.surface.fill_rounded_rect(
            add_rect.x,
            add_rect.y,
            add_rect.w,
            add_rect.h,
            self.metrics.corner_radius,
            argb(lerp(10.0, 28.0, add_hover_t) as u8, 255, 255, 255),
        );
        self.renderer.surface.draw_text(
            "+",
            (add_rect.x, add_rect.y + 20.0 * self.metrics.scale, add_rect.w, 32.0 * self.metrics.scale),
            self.metrics.font(22.0),
            argb(200, 255, 255, 255),
            TextAlign::Center,
        );
        self.renderer.surface.draw_text(
            "Add app",
            (
                add_rect.x,
                add_rect.y + add_rect.h - 26.0 * self.metrics.scale,
                add_rect.w,
                16.0 * self.metrics.scale,
            ),
            self.metrics.font(11.0),
            argb(220, 255, 255, 255),
            TextAlign::Center,
        );

        // Scrollbar, only drawn when content actually overflows.
        let max_scroll = self.max_scroll(apps.len() + 1);
        if max_scroll > 1.0 {
            let content_h = self.content_height(apps.len() + 1);
            let track_h = content.h;
            let thumb_h = (track_h * track_h / content_h).max(24.0 * self.metrics.scale);
            let thumb_y =
                content.y + (self.scroll / max_scroll) * (track_h - thumb_h).max(0.0);
            self.renderer.surface.fill_rounded_rect(
                m_window_w - self.metrics.padding * 0.4 - self.metrics.scrollbar_w,
                thumb_y,
                self.metrics.scrollbar_w,
                thumb_h,
                self.metrics.scrollbar_w / 2.0,
                argb(60, 255, 255, 255),
            );
        }

        self.renderer.surface.reset_clip();
        self.renderer.surface.present(self.hwnd, self.pos.0, self.pos.1);
    }

    pub fn poll_requests(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(category) = self.request_rx.try_recv() {
            self.request(category, event_loop);
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.hotkey_id && event.state == HotKeyState::Pressed {
                self.toggle_visibility(event_loop);
            }
        }

        let now = Instant::now();
        let dt_ms = now.duration_since(self.last_tick).as_secs_f32() * 1000.0;
        self.last_tick = now;

        let mut needs_more_frames = false;

        match self.state {
            DrawerState::SlidingUp { start } => {
                let t = ease_out_smooth(now.duration_since(start).as_secs_f32() * 1000.0 / ANIM_MS);
                let y = lerp(self.layout.hidden_y as f32, self.layout.shown_y as f32, t) as i32;
                self.set_pos(self.layout.shown_x, y);
                if t >= 1.0 {
                    self.state = DrawerState::Shown;
                    self.window.focus_window();
                } else {
                    needs_more_frames = true;
                }
            }
            DrawerState::SlidingDown { start } => {
                let t = ease_out_smooth(now.duration_since(start).as_secs_f32() * 1000.0 / ANIM_MS);
                let y = lerp(self.layout.shown_y as f32, self.layout.hidden_y as f32, t) as i32;
                self.set_pos(self.layout.shown_x, y);
                if t >= 1.0 {
                    self.state = DrawerState::Hidden;
                    self.window.set_visible(false);
                } else {
                    needs_more_frames = true;
                }
            }
            _ => {}
        }

        if !matches!(self.state, DrawerState::Hidden) {
            needs_more_frames |= self.tick_hover(dt_ms);
            self.render();
        }

        if needs_more_frames {
            self.schedule_next_frame(event_loop);
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
    }

    pub fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused && matches!(self.state, DrawerState::Shown) {
                    self.state = DrawerState::SlidingDown { start: Instant::now() };
                    self.schedule_next_frame(event_loop);
                    self.render();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = (position.x as f32, position.y as f32);
                let hovered = self.hit_test(self.mouse.0, self.mouse.1);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    self.schedule_next_frame(event_loop);
                    self.render();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 48.0 * self.metrics.scale,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.scroll -= lines;
                let count = self.filtered_apps().len() + 1;
                self.clamp_scroll(count);
                self.render();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.ctrl_held = mods.state().control_key();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.handle_left_click(event_loop),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.handle_right_click(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.handle_key(event);
                self.render();
            }
            _ => {}
        }
    }

    fn handle_left_click(&mut self, event_loop: &ActiveEventLoop) {
        match self.hovered {
            Hovered::App(i) => {
                if let Some(app_entry) = self.filtered_apps().get(i) {
                    let _ = std::process::Command::new(&app_entry.path).spawn();
                }
                self.toggle_visibility(event_loop);
            }
            Hovered::AppRemove(i) => {
                if let Some(app_entry) = self.filtered_apps().get(i) {
                    let path = app_entry.path.clone();
                    self.config.remove_app(&path);
                    self.icons.remove(&path);
                    self.hover_alpha.remove(&path);
                    self.hovered = Hovered::None;
                    self.render();
                }
            }
            Hovered::Add => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Executable", &["exe"])
                    .pick_file()
                {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "App".to_string());
                    let path_str = path.to_string_lossy().to_string();
                    self.config.add_app(name, path_str);
                }
                self.render();
            }
            Hovered::None => {}
        }
    }

    fn handle_right_click(&mut self) {
        if let Hovered::App(i) | Hovered::AppRemove(i) = self.hovered {
            if self.show_remove_menu() {
                if let Some(app_entry) = self.filtered_apps().get(i) {
                    let path = app_entry.path.clone();
                    self.config.remove_app(&path);
                    self.icons.remove(&path);
                    self.hover_alpha.remove(&path);
                }
            }
            self.render();
        }
    }

    fn handle_key(&mut self, event: winit::event::KeyEvent) {
        if self.ctrl_held {
            if let Key::Character(c) = &event.logical_key {
                if c.eq_ignore_ascii_case("v") {
                    self.paste_from_clipboard();
                    return;
                }
            }
        }
        match event.logical_key {
            Key::Named(NamedKey::Backspace) => {
                self.search.pop();
            }
            Key::Named(NamedKey::Escape) => {
                self.search.clear();
            }
            _ => {
                if let Some(text) = event.text {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        self.search.push(c);
                    }
                }
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}
