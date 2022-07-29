use crate::{color::Color, geometry::Rect};

pub enum DrawCommand {
    Rect(Rect, Color),
}
