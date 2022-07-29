use ash::vk;

use crate::{color::Color, point::Point};

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
            format: vk::Format::R32G32_SFLOAT,
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
