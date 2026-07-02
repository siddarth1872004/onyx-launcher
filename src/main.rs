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

fn main() {
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
