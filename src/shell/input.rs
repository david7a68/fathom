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
