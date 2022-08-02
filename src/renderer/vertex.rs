use ash::vk;

use crate::{color::Color, draw_command::DrawCommand, geometry::Point};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub point: Point,
    pub color: Color,
}

impl Vertex {
    pub const BINDING_DESCRIPTION: vk::VertexInputBindingDescription =
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        };

    pub const ATTRIBUTE_DESCRIPTIONS: [vk::VertexInputAttributeDescription; 2] = [
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R16G16_SINT,
            offset: 0,
        },
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: std::mem::size_of::<Point>() as u32,
        },
    ];
}

pub fn commands_to_vertices(
    commands: &[DrawCommand],
    vertex_buffer: &mut Vec<Vertex>,
    index_buffer: &mut Vec<u16>,
) {
    for command in commands {
        match command {
            DrawCommand::Rect(rect, color) => {
                let offset = vertex_buffer.len() as u16;

                vertex_buffer.push(Vertex {
                    point: rect.top_left(),
                    color: *color,
                });
                vertex_buffer.push(Vertex {
                    point: rect.top_right(),
                    color: *color,
                });
                vertex_buffer.push(Vertex {
                    point: rect.bottom_right(),
                    color: *color,
                });
                vertex_buffer.push(Vertex {
                    point: rect.bottom_left(),
                    color: *color,
                });

                index_buffer.extend_from_slice(&[
                    offset,
                    offset + 1,
                    offset + 2,
                    offset + 2,
                    offset + 3,
                    offset,
                ]);
            }
        }
    }
}
