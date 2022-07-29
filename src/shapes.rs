use crate::{color::Color, point::Point, renderer::Vertex};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub top: f32,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
}

impl Rect {
    pub fn draw(&self, color: Color, vertex_buffer: &mut Vec<Vertex>, index_buffer: &mut Vec<u16>) {
        vertex_buffer.push(Vertex {
            point: Point {
                x: self.left,
                y: self.top,
            },
            color,
        });
        vertex_buffer.push(Vertex {
            point: Point {
                x: self.right,
                y: self.top,
            },
            color,
        });
        vertex_buffer.push(Vertex {
            point: Point {
                x: self.right,
                y: self.bottom,
            },
            color,
        });
        vertex_buffer.push(Vertex {
            point: Point {
                x: self.left,
                y: self.bottom,
            },
            color,
        });
        index_buffer.extend_from_slice(&[0, 1, 2, 2, 3, 0]);
    }
}
