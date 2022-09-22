use crate::gfx::geometry::Extent;

use super::event::Event;

#[cfg(target_os = "windows")]
#[path = "shl2/win32.rs"]
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
        *self = Self::Wait
    }

    pub fn exit(&mut self) {
        *self = Self::Exit
    }
}

/// An operating system shell provides the facilities needed to run user
/// programs. We assume that there is only ever one shell that is active at a
/// time, and set it as a global value.
///
/// This struct provides a uniform interface for those facilities needed by
/// Fathom.
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

    fn hwnd(&self, window: WindowId) -> windows::Win32::Foundation::HWND {
        self.inner.hwnd(window)
    }
}

pub trait Shell {
    fn create_window(&self, config: &WindowConfig) -> Result<WindowId, Error>;

    fn destroy_window(&self, window: WindowId);

    fn show_window(&self, window: WindowId);

    fn hide_window(&self, window: WindowId);

    #[cfg(target_os = "windows")]
    fn hwnd(&self, window: WindowId) -> windows::Win32::Foundation::HWND;
}
