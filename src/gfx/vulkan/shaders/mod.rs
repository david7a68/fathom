mod fill;

pub use fill::Fill;

use ash::vk;

use crate::gfx::{geometry::Point, Vertex};

use super::api::Vulkan;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScaleTranslate {
    scale: [f32; 2],
    translate: [f32; 2],
}

pub const VERTEX_BINDING_DESCRIPTION: vk::VertexInputBindingDescription =
    vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<Vertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    };

pub const VERTEX_ATTRIBUTE_DESCRIPTIONS: [vk::VertexInputAttributeDescription; 3] = [
    // ivec2 position
    vk::VertexInputAttributeDescription {
        location: 0,
        binding: 0,
        format: vk::Format::R16G16_SINT,
        offset: std::mem::size_of::<Point>() as u32,
    },
    // vec4 color
    vk::VertexInputAttributeDescription {
        location: 1,
        binding: 0,
        format: vk::Format::R32G32B32A32_SFLOAT,
        offset: std::mem::size_of::<Point>() as u32 * 2,
    },
    // ivec2 uv
    vk::VertexInputAttributeDescription {
        location: 2,
        binding: 0,
        format: vk::Format::R16G16_SINT,
        offset: 0,
    },
];

pub struct DefaultRenderPass {
    pub handle: vk::RenderPass,
}

impl DefaultRenderPass {
    pub fn new(api: &Vulkan, format: vk::Format) -> Self {
        let attachment_descriptions = [vk::AttachmentDescription {
            flags: vk::AttachmentDescriptionFlags::empty(),
            format,
            samples: vk::SampleCountFlags::TYPE_1,
            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::STORE,
            stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
            stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
        }];

        let subpass_descriptions = [vk::SubpassDescription::builder()
            .color_attachments(&[vk::AttachmentReference {
                attachment: 0,
                layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            }])
            .build()];

        let subpass_dependencies = [vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            src_access_mask: vk::AccessFlags::NONE,
            dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            dependency_flags: vk::DependencyFlags::empty(),
        }];

        let render_pass_ci = vk::RenderPassCreateInfo::builder()
            .attachments(&attachment_descriptions)
            .subpasses(&subpass_descriptions)
            .dependencies(&subpass_dependencies);

        let handle = unsafe { api.device.create_render_pass(&render_pass_ci, None) }.unwrap();

        Self { handle }
    }

    pub fn create_framebuffer(
        &self,
        api: &Vulkan,
        extent: vk::Extent2D,
        color_attachment: vk::ImageView,
    ) -> vk::Framebuffer {
        let create_info = vk::FramebufferCreateInfo {
            render_pass: self.handle,
            attachment_count: 1,
            p_attachments: &color_attachment,
            width: extent.width,
            height: extent.height,
            layers: 1,
            ..Default::default()
        };

        unsafe { api.device.create_framebuffer(&create_info, None) }.unwrap()
    }
}
