use crate::{geometry::Rect, color::Color};

pub enum DrawCommand {
    Rect(Rect, Color),
}
