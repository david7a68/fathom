use std::ffi::CStr;

use ash::vk;

use crate::gfx::{
    backend::{Error, Vertex},
    geometry::Point,
};

use super::api::VulkanApi;

const SHADER_MAIN: *const i8 = b"main\0".as_ptr().cast();
const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

pub const VERTEX_BINDING_DESCRIPTIONS: [vk::VertexInputBindingDescription; 1] =
    [vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<Vertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }];

pub const VERTEX_ATTRIBUTE_DESCRIPTIONS: [vk::VertexInputAttributeDescription; 2] = [
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

pub struct SimpleShaderFactory {
    vertex_shader: vk::ShaderModule,
    fragment_shader: vk::ShaderModule,
}

impl SimpleShaderFactory {
    pub fn new(api: &VulkanApi) -> Result<Self, Error> {
        let vertex_shader = unsafe {
            api.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                    UI_VERT_SHADER_SPV.as_ptr().cast(),
                    UI_VERT_SHADER_SPV.len() / 4,
                )),
                None,
            )?
        };

        let fragment_shader = unsafe {
            api.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                    UI_FRAG_SHADER_SPV.as_ptr().cast(),
                    UI_FRAG_SHADER_SPV.len() / 4,
                )),
                None,
            )?
        };

        Ok(Self {
            vertex_shader,
            fragment_shader,
        })
    }

    pub fn destroy(self, api: &VulkanApi) {
        unsafe {
            api.device.destroy_shader_module(self.vertex_shader, None);
            api.device.destroy_shader_module(self.fragment_shader, None);
        }
    }

    pub fn create_shader(
        &self,
        format: vk::Format,
        api: &VulkanApi,
    ) -> Result<SimpleShader, Error> {
        SimpleShader::new(format, self.vertex_shader, self.fragment_shader, api)
    }
}

pub struct SimpleShader {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub render_pass: vk::RenderPass,
}

impl SimpleShader {
    pub fn new(
        format: vk::Format,
        vertex_shader: vk::ShaderModule,
        fragment_shader: vk::ShaderModule,
        api: &VulkanApi,
    ) -> Result<Self, Error> {
        let layout = unsafe {
            api.device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)?
        };

        let render_pass = {
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

            unsafe { api.device.create_render_pass(&render_pass_ci, None) }?
        };

        let pipeline = {
            let shader_main = unsafe { CStr::from_ptr(SHADER_MAIN) };
            let shader_stage_ci = [
                vk::PipelineShaderStageCreateInfo::builder()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vertex_shader)
                    .name(shader_main)
                    .build(),
                vk::PipelineShaderStageCreateInfo::builder()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(fragment_shader)
                    .name(shader_main)
                    .build(),
            ];

            let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];

            let dynamic_state_ci =
                vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dynamic_states);

            let binding_descriptions = &VERTEX_BINDING_DESCRIPTIONS;
            let attribute_descriptions = &VERTEX_ATTRIBUTE_DESCRIPTIONS;
            let vertex_input_ci = vk::PipelineVertexInputStateCreateInfo::builder()
                .vertex_attribute_descriptions(attribute_descriptions)
                .vertex_binding_descriptions(binding_descriptions);

            let input_assembly_ci = vk::PipelineInputAssemblyStateCreateInfo::builder()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

            let viewport_state_ci = vk::PipelineViewportStateCreateInfo::builder()
                .viewport_count(1)
                .scissor_count(1);

            let rasterization_ci = vk::PipelineRasterizationStateCreateInfo::builder()
                .depth_clamp_enable(false)
                .rasterizer_discard_enable(false)
                .polygon_mode(vk::PolygonMode::FILL)
                .line_width(1.0)
                .cull_mode(vk::CullModeFlags::BACK)
                .front_face(vk::FrontFace::CLOCKWISE)
                .depth_bias_enable(false);

            let multisample_ci = vk::PipelineMultisampleStateCreateInfo::builder()
                .sample_shading_enable(false)
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);

            let framebuffer_blend_ci = vk::PipelineColorBlendAttachmentState::builder()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(false)
                .build();

            let global_blend_ci = vk::PipelineColorBlendStateCreateInfo::builder()
                .logic_op_enable(false)
                .attachments(std::slice::from_ref(&framebuffer_blend_ci));

            let create_info = vk::GraphicsPipelineCreateInfo::builder()
                .stages(&shader_stage_ci)
                .vertex_input_state(&vertex_input_ci)
                .input_assembly_state(&input_assembly_ci)
                .viewport_state(&viewport_state_ci)
                .rasterization_state(&rasterization_ci)
                .multisample_state(&multisample_ci)
                .color_blend_state(&global_blend_ci)
                .dynamic_state(&dynamic_state_ci)
                .layout(layout)
                .render_pass(render_pass)
                .subpass(0)
                .build();

            match unsafe {
                api.device
                    .create_graphics_pipelines(api.pipeline_cache, &[create_info], None)
            } {
                Ok(pipelines) => pipelines[0],
                Err((_, err)) => {
                    return Err(Error::VulkanInternal { error_code: err });
                }
            }
        };

        Ok(Self {
            pipeline,
            pipeline_layout: layout,
            render_pass,
        })
    }

    pub fn destroy(self, api: &VulkanApi) {
        unsafe {
            api.device.destroy_pipeline(self.pipeline, None);
            api.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            api.device.destroy_render_pass(self.render_pass, None);
        }
    }
}
