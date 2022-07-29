use std::ffi::CStr;

use ash::vk;

use crate::color::Color;

use super::{error::Error, vertex::Vertex};

const SHADER_MAIN: *const i8 = b"main\0".as_ptr().cast();
const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

#[repr(C)]
pub struct PushConstants {
    pub scale: [f32; 2],
    pub translate: [f32; 2],
}

pub struct Pipeline {
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub render_pass: vk::RenderPass,
}

pub fn create(device: &ash::Device, swapchain_format: vk::Format) -> Result<Pipeline, Error> {
    let layout = {
        let push_constant_range = [vk::PushConstantRange::builder()
            .offset(0)
            .size(
                std::mem::size_of::<PushConstants>()
                    .try_into()
                    .expect("push constants exceed 2^32 bytes; what happened?"),
            )
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .build()];

        let pipeline_layout_ci =
            vk::PipelineLayoutCreateInfo::builder().push_constant_ranges(&push_constant_range);

        unsafe { device.create_pipeline_layout(&pipeline_layout_ci, None)? }
    };

    let render_pass = {
        let attachment_descriptions = [vk::AttachmentDescription {
            flags: vk::AttachmentDescriptionFlags::empty(),
            format: swapchain_format,
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

        unsafe { device.create_render_pass(&render_pass_ci, None) }?
    };

    let pipeline = {
        let vertex_shader = unsafe {
            device.create_shader_module(
                &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                    UI_VERT_SHADER_SPV.as_ptr().cast(),
                    UI_VERT_SHADER_SPV.len() / 4,
                )),
                None,
            )?
        };

        let fragment_shader = unsafe {
            device.create_shader_module(
                &vk::ShaderModuleCreateInfo::builder().code(std::slice::from_raw_parts(
                    UI_FRAG_SHADER_SPV.as_ptr().cast(),
                    UI_FRAG_SHADER_SPV.len() / 4,
                )),
                None,
            )?
        };

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

        let binding_descriptions = &[Vertex::BINDING_DESCRIPTION];
        let attribute_descriptions = &Vertex::ATTRIBUTE_DESCRIPTIONS;
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

        let pipeline = match unsafe {
            device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_ci], None)
        } {
            Ok(pipelines) => pipelines[0],
            Err((_, err)) => {
                return Err(Error::Vulkan(err));
            }
        };

        unsafe {
            device.destroy_shader_module(vertex_shader, None);
            device.destroy_shader_module(fragment_shader, None);
        }

        pipeline
    };

    Ok(Pipeline {
        pipeline,
        layout,
        render_pass,
    })
}

pub fn record_draw(
    vkdevice: &ash::Device,
    pipeline: &Pipeline,
    command_buffer: vk::CommandBuffer,
    frame_buffer: vk::Framebuffer,
    viewport: vk::Extent2D,
    clear_color: Color,
    vertex_buffer: vk::Buffer,
    index_buffer: vk::Buffer,
    num_indices: u16,
) -> Result<vk::CommandBuffer, Error> {
    unsafe {
        vkdevice.begin_command_buffer(
            command_buffer,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;

        vkdevice.cmd_begin_render_pass(
            command_buffer,
            &vk::RenderPassBeginInfo::builder()
                .render_pass(pipeline.render_pass)
                .framebuffer(frame_buffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: viewport,
                })
                .clear_values(&[vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: clear_color.to_array(),
                    },
                }]),
            vk::SubpassContents::INLINE,
        );

        vkdevice.cmd_bind_pipeline(
            command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            pipeline.pipeline,
        );

        vkdevice.cmd_set_viewport(
            command_buffer,
            0,
            std::slice::from_ref(&vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: viewport.width as f32,
                height: viewport.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }),
        );

        vkdevice.cmd_set_scissor(
            command_buffer,
            0,
            std::slice::from_ref(&vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: viewport,
            }),
        );

        vkdevice.cmd_bind_vertex_buffers(command_buffer, 0, &[vertex_buffer], &[0]);
        vkdevice.cmd_bind_index_buffer(command_buffer, index_buffer, 0, vk::IndexType::UINT16);

        // normalize the vertices to [0, 1]

        let scale = [
            (2.0 / viewport.width as f32),
            (2.0 / viewport.height as f32),
        ];

        let mut scale_bytes = [0; 8];
        scale_bytes[0..4].copy_from_slice(&scale[0].to_ne_bytes());
        scale_bytes[4..8].copy_from_slice(&scale[1].to_ne_bytes());

        vkdevice.cmd_push_constants(
            command_buffer,
            pipeline.layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            &scale_bytes,
        );

        let translate: [f32; 2] = [-1.0, -1.0];

        let mut translate_bytes = [0; 8];
        translate_bytes[0..4].copy_from_slice(&translate[0].to_ne_bytes());
        translate_bytes[4..8].copy_from_slice(&translate[1].to_ne_bytes());

        vkdevice.cmd_push_constants(
            command_buffer,
            pipeline.layout,
            vk::ShaderStageFlags::VERTEX,
            scale_bytes
                .len()
                .try_into()
                .expect("scale push constaznt took up more than 2^32 bytes; overflow error?"),
            &translate_bytes,
        );

        vkdevice.cmd_draw_indexed(command_buffer, num_indices.into(), 1, 0, 0, 0);

        vkdevice.cmd_end_render_pass(command_buffer);
        vkdevice.end_command_buffer(command_buffer)?;
    }

    Ok(command_buffer)
}
