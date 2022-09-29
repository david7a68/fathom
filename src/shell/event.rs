use crate::gfx::geometry::{Extent, Point};

use super::WindowId;

/// Events that can be received from the OS event loop.
///
/// Note that each mouse button/state combination is included as a unique event.
/// This is intentional, and has the benefit of reducing a branch for every
/// mouse button event since there is no need to match on the button. In this,
/// we trade a minor aesthetic inconvenience for a minor efficiency improvement.
#[derive(Clone, Copy, Debug, Default)]
#[repr(u8)]
pub enum Event {
    #[default]
    None,
    Window {
        window_id: WindowId,
        event: Window,
    },
    /// Indicates that all repaint requests for the current loop iteration have
    /// been completed. Handle this message to perform any shared post-rendering
    /// operations.
    RepaintComplete,
}

/// Window-specific events that can be received from the OS event loop.
///
/// Note that each mouse button/state combination is included as a unique event.
/// This is intentional, and has the benefit of reducing a branch for every
/// mouse button event since there is no need to match on the button. In this,
/// we trade a minor aesthetic inconvenience for a minor efficiency improvement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Window {
    Init { inner_extent: Extent },
    CloseRequested,
    Destroyed,
    Resized { inner_extent: Extent },
    CursorMoved { position: Point },
    Repaint,
    LeftMouseButtonPressed,
    LeftMouseButtonReleased,
    RightMouseButtonPressed,
    RightMouseButtonReleased,
    MiddleMouseButtonPressed,
    MiddleMouseButtonReleased,
}
