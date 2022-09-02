use crate::gfx::geometry::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
#[must_use]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    #[must_use]
    pub fn is_left(&self) -> bool {
        *self == MouseButton::Left
    }

    #[must_use]
    pub fn is_right(&self) -> bool {
        *self == MouseButton::Right
    }

    #[must_use]
    pub fn is_middle(&self) -> bool {
        *self == MouseButton::Middle
    }
}

/// The state of a button such as a mouse button or keyboard key.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(u8)]
#[must_use]
pub enum ButtonState {
    #[default]
    Released,
    Pressed,
}

impl ButtonState {
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        *self == ButtonState::Pressed
    }

    #[must_use]
    pub fn is_released(&self) -> bool {
        *self == ButtonState::Released
    }
}

#[derive(Clone, Copy, Default)]
pub enum Event {
    #[default]
    None,
    CursorMove {
        position: Point,
    },
    MouseButton {
        button: MouseButton,
        state: ButtonState,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tick(u32);

#[derive(Default)]
pub struct Input {
    tick: Tick,
    cursor_position: (Point, Tick),
    mouse_buttons: [(ButtonState, Tick); 3],
    event: Event,
}

impl Input {
    /// Call this every frame to update input change tracking.
    pub fn tick(&mut self) {
        self.tick.0 += 1;
        self.event = Event::None;
    }

    #[must_use]
    pub fn event(&self) -> Event {
        self.event
    }

    /// Returns true if the cursor was updated since the last call to `tick()`
    /// (usually called every frame).
    #[must_use]
    pub fn was_cursor_updated(&self) -> bool {
        self.cursor_position.1 == self.tick
    }

    pub fn cursor_position(&self) -> Point {
        self.cursor_position.0
    }

    pub fn update_cursor_position(&mut self, position: Point) {
        self.cursor_position = (position, self.tick);
        self.event = Event::CursorMove { position };
    }

    /// Returns true if the button was updated since the last call to `tick()`
    /// (usually called every frame).
    #[must_use]
    pub fn was_mouse_button_updated(&self, button: MouseButton) -> bool {
        self.mouse_buttons[button as usize].1 == self.tick
    }

    pub fn mouse_button_state(&self, button: MouseButton) -> ButtonState {
        self.mouse_buttons[button as usize].0
    }

    pub fn update_mouse_button(&mut self, button: MouseButton, state: ButtonState) {
        self.mouse_buttons[button as usize] = (state, self.tick);
        self.event = Event::MouseButton { button, state };
    }
}
