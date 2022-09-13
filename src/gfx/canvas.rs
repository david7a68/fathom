use super::{
    color::Color,
    geometry::{Extent, Rect},
};

pub trait Canvas {
    fn extent(&self) -> Extent;

    fn fill_rect(&mut self, rect: Rect, color: Color);
}
