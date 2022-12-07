use ash::vk;

use crate::gfx::vulkan::{
    api::{VkResult, Vulkan},
    as_cchar_slice,
    geometry::UiGeometryBuffer,
};

use super::{ScaleTranslate, VERTEX_ATTRIBUTE_DESCRIPTIONS, VERTEX_BINDING_DESCRIPTION};

pub struct Fill {
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
}

impl Fill {
    const SHADER_MAIN: *const i8 = as_cchar_slice(b"main\0").as_ptr();
    const VERTEX_SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fill.vert.spv"));
    const FRAGMENT_SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fill.frag.spv"));

    pub fn new(api: &Vulkan, render_pass: vk::RenderPass) -> VkResult<Self> {
        let layout = {
            let ranges = [vk::PushConstantRange::builder()
                .offset(0)
                .size(std::mem::size_of::<ScaleTranslate>() as u32)
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .build()];

            let ci = vk::PipelineLayoutCreateInfo::builder().push_constant_ranges(&ranges);

            unsafe { api.device.create_pipeline_layout(&ci, None) }?
        };

        let pipeline = {
            let vertex_shader = unsafe {
                api.device.create_shader_module(
                    &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                        Self::VERTEX_SHADER.as_ptr().cast(),
                        Self::VERTEX_SHADER.len() / 4,
                    )),
                    None,
                )?
            };

            let fragment_shader = unsafe {
                api.device.create_shader_module(
                    &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                        Self::FRAGMENT_SHADER.as_ptr().cast(),
                        Self::FRAGMENT_SHADER.len() / 4,
                    )),
                    None,
                )?
            };

            let shader_main = unsafe { std::ffi::CStr::from_ptr(Self::SHADER_MAIN) };
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

            let binding_descriptions = &[VERTEX_BINDING_DESCRIPTION];
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

            let pipeline_ci = vk::GraphicsPipelineCreateInfo::builder()
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

            let pipeline = {
                let mut pipeline = vk::Pipeline::null();
                unsafe {
                    // Call the function pointer directly to avoid allocating a
                    // 1-element Vec
                    (api.device.fp_v1_0().create_graphics_pipelines)(
                        api.device.handle(),
                        api.pipeline_cache,
                        1,
                        &pipeline_ci,
                        std::ptr::null(),
                        &mut pipeline,
                    )
                }
                .result()?;
                pipeline
            };

            unsafe {
                api.device.destroy_shader_module(vertex_shader, None);
                api.device.destroy_shader_module(fragment_shader, None);
            }

            pipeline
        };

        Ok(Self { pipeline, layout })
    }

    pub fn destroy(self, api: &Vulkan) {
        unsafe {
            api.device.destroy_pipeline(self.pipeline, None);
            api.device.destroy_pipeline_layout(self.layout, None);
        }
    }

    pub fn draw_indexed(
        &self,
        api: &Vulkan,
        first_index: u16,
        num_indices: u16,
        viewport: vk::Extent2D,
        geometry: &UiGeometryBuffer,
        command_buffer: vk::CommandBuffer,
    ) {
        unsafe {
            api.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            api.device
                .cmd_bind_vertex_buffers(command_buffer, 0, &[geometry.handle], &[0]);

            api.device.cmd_bind_index_buffer(
                command_buffer,
                geometry.handle,
                geometry.index_offset,
                vk::IndexType::UINT16,
            );

            api.device.cmd_push_constants(
                command_buffer,
                self.layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                &std::mem::transmute::<_, [u8; std::mem::size_of::<ScaleTranslate>()]>(
                    ScaleTranslate {
                        scale: [2.0 / viewport.width as f32, 2.0 / viewport.height as f32],
                        translate: [-1.0, -1.0],
                    },
                ),
            );

            api.device.cmd_draw_indexed(
                command_buffer,
                u32::from(num_indices),
                1,
                u32::from(first_index),
                0,
                0,
            );
        }
    }
}
