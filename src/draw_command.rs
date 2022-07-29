use crate::{shapes::Rect, color::Color};

pub enum DrawCommand {
    Rect(Rect, Color),
}
