use crate::{point::Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub top: f32,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
}

impl Rect {
    pub fn contains(&self, point: Point) -> bool {
        self.left <= point.x
            && point.x < self.right
            && self.top <= point.y
            && point.y <= self.bottom
    }
}
