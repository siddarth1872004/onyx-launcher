use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;

/// Single fixed loopback port for the whole app, regardless of how many
/// categories exist. Only ever one process is resident: whichever
/// onyx-launcher-family exe launches first binds this port and becomes the
/// shared hub; every other launch (default or any category) just sends its
/// category name to the hub and exits immediately. This keeps memory/CPU
/// flat no matter how many categories are pinned to the taskbar.
const HUB_PORT: u16 = 47821;

/// Tries to become the single resident hub process. On success, returns a
/// listener that should be handed to [`spawn_listener`]. If the hub is
/// already running, this sends it `category` (empty string for the default,
/// uncategorized drawer) so it can show/switch to that category, then
/// returns `None` so the caller can exit immediately.
pub fn claim_or_wake(category: Option<&str>) -> Option<TcpListener> {
    match TcpListener::bind(("127.0.0.1", HUB_PORT)) {
        Ok(listener) => Some(listener),
        Err(_) => {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", HUB_PORT)) {
                let _ = stream.write_all(category.unwrap_or("").as_bytes());
            }
            None
        }
    }
}

/// Spawns a background thread that blocks on `accept()` and forwards the
/// requested category (`None` for the default drawer) through `tx` every
/// time another launch pings the hub. The thread is fully event-driven
/// (blocked in the kernel between wakeups), so it costs nothing while idle.
pub fn spawn_listener(
    listener: TcpListener,
    tx: Sender<Option<String>>,
    wake: impl Fn() + Send + 'static,
) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 256];
            let n = stream.read(&mut buf).unwrap_or(0);
            let name = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            let category = if name.is_empty() { None } else { Some(name) };
            let _ = tx.send(category);
            wake();
        }
    });
}
