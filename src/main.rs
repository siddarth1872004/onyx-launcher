#![windows_subsystem = "windows"]

use std::net::TcpListener;

use onyx_launcher::app::{DrawerApp, UserEvent};
use onyx_launcher::{config, ipc};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

struct Handler {
    listener: Option<TcpListener>,
    category: Option<String>,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    app: Option<DrawerApp>,
}

impl ApplicationHandler<UserEvent> for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_none() {
            let listener = self.listener.take().expect("listener already consumed");
            self.app = Some(DrawerApp::new(
                event_loop,
                listener,
                self.category.take(),
                self.proxy.clone(),
            ));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        if let Some(app) = &mut self.app {
            app.window_event(event_loop, window_id, event);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: UserEvent) {
        if let Some(app) = &mut self.app {
            app.poll_requests(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.poll_requests(event_loop);
        }
    }
}

/// Windows derives an implicit "AppUserModelID" for an unregistered exe from
/// its file path, and the shell uses that ID to decide whether a taskbar
/// pin click should activate an already-running instance instead of
/// re-launching the target. Since our resident hub deliberately has no
/// taskbar-tracked window to activate (see `set_skip_taskbar`), that
/// shell-side "it's already running, nothing to activate" conclusion can
/// make the pin click silently do nothing instead of falling back to
/// actually running the exe again. Giving every process launch its own
/// unique explicit AppUserModelID means the shell never considers a new
/// launch a match for whatever's already running, so it always re-executes
/// the pinned target - which is what we want, since our own IPC handshake
/// (not Explorer's window activation) is what's responsible for showing the
/// drawer.
fn set_unique_app_id() {
    let aumid: Vec<u16> = format!("OnyxLauncher.{}\0", std::process::id())
        .encode_utf16()
        .collect();
    unsafe {
        let _ = windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
            windows::core::PCWSTR(aumid.as_ptr()),
        );
    }
}

fn main() {
    set_unique_app_id();

    let category = config::category_name();

    let Some(listener) = ipc::claim_or_wake(category.as_deref()) else {
        // Another instance is already running; it has been woken up.
        // Exit immediately.
        return;
    };

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();

    let mut handler = Handler {
        listener: Some(listener),
        category,
        proxy,
        app: None,
    };
    let _ = event_loop.run_app(&mut handler);
}
