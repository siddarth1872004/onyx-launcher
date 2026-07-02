//! Single-instance coordination built on native Win32 named objects (a
//! mutex plus an auto-reset event), replacing the older TCP-loopback hub.
//!
//! The lifecycle model is deliberately simple: **a process exists only while
//! its drawer is on screen.** When the drawer finishes hiding, the process
//! exits. That means there is never a hidden, resident background process
//! that a later launch has to reach through some fragile channel - reopening
//! the drawer is just "run the exe again", which the OS does reliably every
//! single time.
//!
//! Two things still need coordinating between launches:
//!
//! 1. Don't show two copies of the same drawer at once. A named **mutex**
//!    (unique per category) marks "this drawer is currently up".
//! 2. Clicking the taskbar pin again while the drawer is already up should
//!    close it. The second launch can't grab the mutex, so it pulses a named
//!    **event** the running instance is waiting on, telling it to hide (and
//!    therefore exit), then exits itself.
//!
//! Even if that event signal is ever missed, clicking the pin also steals
//! focus from the drawer, which triggers the same hide-and-exit path - so the
//! close behaviour has a built-in fallback and the reopen path never depends
//! on any of it.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Category names come from an exe's file stem, so they shouldn't contain
/// path separators - but a named kernel object must not contain a backslash
/// (that denotes a namespace), so sanitize defensively.
fn object_names(category: Option<&str>) -> (Vec<u16>, Vec<u16>) {
    let key = category.unwrap_or("default").replace(['\\', '/'], "_");
    (
        wide(&format!("Local\\OnyxLauncher.instance.{key}")),
        wide(&format!("Local\\OnyxLauncher.signal.{key}")),
    )
}

/// Holds the two named handles for the lifetime of the sole running instance.
/// Dropping it (i.e. the process exiting) releases the mutex so the next
/// launch can claim it. The event handle keeps the shared event object alive
/// so signalling launches have something to pulse.
pub struct InstanceGuard {
    mutex: HANDLE,
    event: HANDLE,
}

// The raw handles are just kernel object references; it's safe to hand the
// event handle's value to the listener thread.
unsafe impl Send for InstanceGuard {}

impl InstanceGuard {
    /// The auto-reset event's raw handle value, for the listener thread.
    fn event_value(&self) -> isize {
        self.event.0 as isize
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.mutex);
            let _ = CloseHandle(self.event);
        }
    }
}

/// Tries to become the sole instance for `category`.
///
/// - `Some(guard)` -> we own the drawer; keep the guard alive for the
///   process lifetime and start showing the drawer.
/// - `None` -> a drawer for this category is already up; we've pulsed it to
///   toggle (close) and the caller should exit immediately.
pub fn acquire_or_signal(category: Option<&str>) -> Option<InstanceGuard> {
    let (mutex_name, event_name) = object_names(category);
    unsafe {
        // Create the mutex first and capture *its* last-error immediately;
        // CreateEventW below would otherwise clobber GetLastError.
        let mutex = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())).ok()?;
        let already_running = GetLastError() == ERROR_ALREADY_EXISTS;

        // Auto-reset, initially non-signalled. Both instances name the same
        // event, so this returns a handle to the one shared object.
        let event = match CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseHandle(mutex);
                return None;
            }
        };

        if already_running {
            // A drawer is already up: tell it to toggle closed, then bow out.
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
            let _ = CloseHandle(mutex);
            return None;
        }

        Some(InstanceGuard { mutex, event })
    }
}

/// Spawns a thread that blocks on the toggle event and calls `wake` each time
/// another launch pulses it. The thread sleeps in the kernel between pulses,
/// so it costs nothing while idle.
pub fn spawn_signal_listener(guard: &InstanceGuard, wake: impl Fn() + Send + 'static) {
    let event_value = guard.event_value();
    std::thread::spawn(move || {
        let event = HANDLE(event_value as *mut _);
        loop {
            let result = unsafe { WaitForSingleObject(event, INFINITE) };
            if result != WAIT_OBJECT_0 {
                break;
            }
            wake();
        }
    });
}
