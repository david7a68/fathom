use super::{geometry::{Rect, Extent}, color::Color};

pub trait Canvas {
    fn extent(&self) -> Extent;

    fn fill_rect(&mut self, rect: Rect, color: Color);
}
