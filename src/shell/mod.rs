pub mod event;

use crate::gfx::geometry::Extent;

use event::Event;

#[cfg(target_os = "windows")]
#[path = "win32.rs"]
mod platform;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the operation is not failed because the shell is shutting down")]
    ShuttingDown,
}

pub struct WindowConfig<'a> {
    pub title: &'a str,
    pub extent: Option<Extent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(platform::WindowId);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventLoopControl {
    Poll,
    Wait,
    /// Performs a clean exit of the event loop once the callback returns. Any
    /// windows created within the callback will be destroyed (the
    /// Event::Destroy message will be sent to each window) and window creation
    /// will be disabled.
    ///
    /// The program will then quit, ostensibly as a platform compatibility. It
    /// doesn't really matter yet since we only support Windows right now, but
    /// it's a small feature that costs almost nothing to implement now and
    /// could prevent much pain in the future.
    Exit,
}

impl EventLoopControl {
    pub fn poll(&mut self) {
        *self = Self::Poll;
    }

    pub fn wait(&mut self) {
        *self = Self::Wait;
    }

    pub fn exit(&mut self) {
        *self = Self::Exit;
    }
}

/// An operating system shell provides the facilities needed to run user
/// programs. We assume that there is only ever one shell that is active at a
/// time, and set it as a global value.
///
/// This struct provides a uniform interface for those facilities needed by
/// Fathom.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub struct OsShell {
    inner: platform::OsShell,
}

impl OsShell {
    pub fn initialize() -> Self {
        Self {
            inner: platform::OsShell::initialize(),
        }
    }

    pub fn run_event_loop<F>(&self, callback: F)
    where
        F: 'static + FnMut(Event, &dyn Shell, &mut EventLoopControl),
    {
        self.inner.run_event_loop(callback)
    }
}

impl Shell for OsShell {
    fn create_window(&self, config: &WindowConfig) -> Result<WindowId, Error> {
        self.inner.create_window(config)
    }

    fn destroy_window(&self, window: WindowId) {
        self.inner.destroy_window(window);
    }

    fn show_window(&self, window: WindowId) {
        self.inner.show_window(window);
    }

    fn hide_window(&self, window: WindowId) {
        self.inner.hide_window(window);
    }

    #[cfg(target_os = "windows")]
    fn hwnd(&self, window: WindowId) -> windows::Win32::Foundation::HWND {
        self.inner.hwnd(window)
    }
}

pub trait Shell {
    /// Creates a new window for the given configuration.
    ///
    /// ## Errors
    ///
    /// Window creation may fail if the shell is currently shutting down.
    fn create_window(&self, config: &WindowConfig) -> Result<WindowId, Error>;

    /// Schedules the window for destruction. A `WindowEvent::Destroyed` event
    /// will be sent to the event handler after the window is no longer visible
    /// but before its associated resources are destroyed.
    fn destroy_window(&self, window: WindowId);

    /// Makes the window visible.
    fn show_window(&self, window: WindowId);

    /// Makes the window invisible.
    fn hide_window(&self, window: WindowId);

    /// Retrieves the `HWND` for the window.
    #[cfg(target_os = "windows")]
    fn hwnd(&self, window: WindowId) -> windows::Win32::Foundation::HWND;
}
