#[cfg(target_os = "windows")]
#[path = "./win32.rs"]
mod platform;

use crate::geometry::{Extent, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// The state of a button such as a mouse button or keyboard key.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ButtonState {
    #[default]
    Released,
    Pressed,
}

pub struct WindowConfig<'a> {
    pub title: &'a str,
    pub extent: Option<Extent>,
}

pub use platform::WindowHandle;

/// This trait defines the interface for a window event handler. It is
/// effectively a set of callbacks with associated state.
///
/// This approach was chosen (2022-07-28) because it is the most flexible and
/// offers good isolation between event handlers and the OS' windowing system
/// whilst supporting a convenient way to associate state with each window. It
/// also offers the ability to be polymorphic over the implementation of the
/// event handler, greatly simplifying the event loop.
///
/// However, there are a couple tradeoffs to this approach:
///
///  - There is a likely cache miss every frame, since the event handler has
///    likely been displaced by the time a new frame is needed by
///    application-specific and rendering code. It should only be one miss,
///    however, since input event handlers are likely to be relatively small
///    relative to `on_redraw()`.
///  - Allocating the trait object involves a heap allocation with its
///    associated costs in time and space (fragmentation).
///  - The amount of code required for a codebase with only one implementor of
///    WindowEventHandler is likely similar to just using that implementor
///    directly (at the cost of increased coupling).
pub trait WindowEventHandler {
    /// Handles initialization of any user state associated with the window.
    ///
    /// This method is called when the window is first created and isn't
    /// visible.
    ///
    /// Return `EventReply::Continue` to continue creating the window, and
    /// `EventReply::DestroyWindow` to abort. Returning an error will cause the
    /// window to be immediately destroyed.
    fn on_create(&mut self, window_handle: WindowHandle, control: &mut dyn Proxy);

    /// Handles user-originating close requests, such as by clicking the 'X'
    /// button on the window's titlebar or by pressing 'Alt+F4'.
    ///
    /// Return `EventReply::Continue` to ignore the request, or
    /// `EventReply::DestroyWindow` to destroy the window. Returning an error
    /// will cause the window to be immediately destroyed.
    fn on_close(&mut self, control: &mut dyn Proxy);

    /// Redraws the window's contents and presents it to the screen. This should
    /// be called once per frame.
    ///
    /// Return `EventReply::Continue` to continue processing events (and keep
    /// the window open), or `EventReply::DestroyWindow` to destroy the window
    /// after the function returns.
    fn on_redraw(&mut self, control: &mut dyn Proxy, window_size: Extent);

    /// Processes cursor movement accross the window. This may be called even
    /// when the window is out of focus.
    ///
    /// Return `EventReply::Continue` to continue processing events (and keep
    /// the window open), or `EventReply::DestroyWindow` to destroy the window
    /// after the function returns.
    fn on_mouse_move(&mut self, control: &mut dyn Proxy, new_position: Point);

    /// Processes a mouse button press.
    ///
    /// Return `EventReply::Continue` to continue processing events (and keep
    /// the window open), or `EventReply::DestroyWindow` to destroy the window
    /// after the function returns.
    fn on_mouse_button(&mut self, control: &mut dyn Proxy, button: MouseButton, state: ButtonState);
}

/// Expresses the interface for controlling window lifetimes outside of the
/// event handler. This is used to permit a new window to be created whilst
/// within an event handler.
pub trait Proxy {
    /// Creates a new window with the given event handler and associated state.
    fn create_window(&self, config: &WindowConfig, window: Box<dyn WindowEventHandler>);

    fn destroy_window(&self, window: WindowHandle);
}

/// The event loop is responsible for querying window events from the OS and
/// passing them to their corresponding `WindowEventHandler` implementations. It
/// also handles creating new windows.
pub struct EventLoop(platform::EventLoop);

impl EventLoop {
    /// Initializes the event loop.
    pub fn new() -> Self {
        Self(platform::EventLoop::new())
    }

    pub fn create_window(&self, config: &WindowConfig, window: Box<dyn WindowEventHandler>) {
        self.0.create_window(config, window);
    }

    /// Runs the event loop until there are no windows open.
    pub fn run(&mut self) {
        self.0.run();
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}
