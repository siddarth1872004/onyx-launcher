#![windows_subsystem = "windows"]

use onyx_launcher::app::{DrawerApp, UserEvent};
use onyx_launcher::config;
use onyx_launcher::single_instance::{self, InstanceGuard};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

struct Handler {
    // Kept alive for the whole process: dropping it (on exit) releases the
    // single-instance mutex so the next launch can show the drawer.
    _guard: Option<InstanceGuard>,
    category: Option<String>,
    app: Option<DrawerApp>,
}

impl ApplicationHandler<UserEvent> for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_none() {
            self.app = Some(DrawerApp::new(event_loop, self.category.take()));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        if let Some(app) = &mut self.app {
            app.window_event(event_loop, window_id, event);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: UserEvent) {
        // A second launch pulsed our toggle event: hide (and therefore exit).
        if let Some(app) = &mut self.app {
            app.begin_close(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.tick(event_loop);
        }
    }
}

fn main() {
    let category = config::category_name();

    // If a drawer for this category is already up, `acquire_or_signal` has
    // already told it to toggle closed - so we just exit. Otherwise we own
    // the drawer and hold the guard for the process lifetime.
    let Some(guard) = single_instance::acquire_or_signal(category.as_deref()) else {
        return;
    };

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    // Wake the event loop whenever another launch pulses the toggle event.
    let proxy = event_loop.create_proxy();
    single_instance::spawn_signal_listener(&guard, move || {
        let _ = proxy.send_event(UserEvent::Toggle);
    });

    let mut handler = Handler {
        _guard: Some(guard),
        category,
        app: None,
    };
    let _ = event_loop.run_app(&mut handler);
}
