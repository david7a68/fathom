use super::{
    color::Color,
    geometry::{Extent, Rect},
};

pub enum Paint {
    Fill { color: Color },
}

pub trait Canvas {
    fn extent(&self) -> Extent;

    fn draw_rect(&mut self, rect: Rect, paint: &Paint);
}
