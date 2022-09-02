use crate::gfx::{color::Color, geometry::Rect};

#[derive(Debug)]
pub enum DrawCommand {
    Rect(Rect, Color),
}
