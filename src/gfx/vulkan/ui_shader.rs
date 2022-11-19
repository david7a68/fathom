use ash::vk;
use smallvec::SmallVec;

use crate::{
    gfx::{self, color::Color, geometry::Point, Command, DrawCommandList, Vertex, MAX_IMAGES},
    handle_pool::Handle,
};

use super::{
    api::{next_multiple_of, Vulkan},
    MemoryUsage,
};

const SHADER_MAIN: *const i8 = b"main\0".as_ptr().cast();
const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

pub const BINDING_DESCRIPTION: vk::VertexInputBindingDescription =
    vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<Vertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    };

pub const ATTRIBUTE_DESCRIPTIONS: [vk::VertexInputAttributeDescription; 3] = [
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

#[repr(C)]
#[derive(Clone, Copy)]
struct VertexPushConstants {
    scale: [f32; 2],
    translate: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FragmentPushConstants {
    use_texture: vk::Bool32,
}

/// Utility struct for holding a pipeline and render pass.
pub struct UiShader {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
}

impl UiShader {
    #[allow(clippy::too_many_lines)]
    pub fn new(api: &Vulkan, format: vk::Format) -> Result<Self, vk::Result> {
        let layout = {
            let push_constant_range = [
                vk::PushConstantRange::builder()
                    .offset(0)
                    .size(
                        std::mem::size_of::<VertexPushConstants>()
                            .try_into()
                            .expect("push constants exceed 2^32 bytes; what happened?"),
                    )
                    .stage_flags(vk::ShaderStageFlags::VERTEX)
                    .build(),
                vk::PushConstantRange::builder()
                    .offset(std::mem::size_of::<VertexPushConstants>() as u32)
                    .size(
                        std::mem::size_of::<FragmentPushConstants>()
                            .try_into()
                            .expect("push constants exceed 2^32 bytes; what happened?"),
                    )
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                    .build(),
            ];

            let pipeline_layout_ci =
                vk::PipelineLayoutCreateInfo::builder().push_constant_ranges(&push_constant_range);

            unsafe {
                api.device
                    .create_pipeline_layout(&pipeline_layout_ci, None)?
            }
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

            let shader_main = unsafe { std::ffi::CStr::from_ptr(SHADER_MAIN) };
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

            let binding_descriptions = &[BINDING_DESCRIPTION];
            let attribute_descriptions = &ATTRIBUTE_DESCRIPTIONS;
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

        Ok(Self {
            pipeline,
            layout,
            render_pass,
        })
    }

    pub fn destroy(self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_pipeline_layout(self.layout, None);
        }
    }

    pub fn begin(
        &self,
        api: &Vulkan,
        target: vk::Framebuffer,
        viewport: vk::Extent2D,
        command_buffer: vk::CommandBuffer,
    ) {
        unsafe {
            api.device.cmd_begin_render_pass(
                command_buffer,
                &vk::RenderPassBeginInfo::builder()
                    .render_pass(self.render_pass)
                    .framebuffer(target)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: viewport,
                    })
                    .clear_values(&[vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: Color::BLACK.to_array(),
                        },
                    }]),
                vk::SubpassContents::INLINE,
            );

            api.device.cmd_set_viewport(
                command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width as f32,
                    height: viewport.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );

            api.device.cmd_set_scissor(
                command_buffer,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: viewport,
                }],
            );
        }
    }

    pub fn end(&self, api: &Vulkan, command_buffer: vk::CommandBuffer) {
        unsafe { api.device.cmd_end_render_pass(command_buffer) };
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
                &std::mem::transmute::<_, [u8; std::mem::size_of::<VertexPushConstants>()]>(
                    VertexPushConstants {
                        scale: [2.0 / viewport.width as f32, 2.0 / viewport.height as f32],
                        translate: [-1.0, -1.0],
                    },
                ),
            );

            api.device.cmd_push_constants(
                command_buffer,
                self.layout,
                vk::ShaderStageFlags::FRAGMENT,
                std::mem::size_of::<VertexPushConstants>() as u32,
                &std::mem::transmute::<_, [u8; 4]>(FragmentPushConstants {
                    use_texture: vk::FALSE,
                }),
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

    pub fn draw_textured(
        &self,
        api: &Vulkan,
        first_index: u16,
        num_indices: u16,
        viewport: vk::Extent2D,
        texture: &vk::DescriptorImageInfo,
        descriptor: vk::DescriptorSet,
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
                &std::mem::transmute::<_, [u8; std::mem::size_of::<VertexPushConstants>()]>(
                    VertexPushConstants {
                        scale: [2.0 / viewport.width as f32, 2.0 / viewport.height as f32],
                        translate: [-1.0, -1.0],
                    },
                ),
            );

            api.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet {
                    dst_set: descriptor,
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    p_image_info: texture,
                    ..Default::default()
                }],
                &[],
            );

            api.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[descriptor],
                &[0],
            );

            api.device.cmd_push_constants(
                command_buffer,
                self.layout,
                vk::ShaderStageFlags::FRAGMENT,
                std::mem::size_of::<VertexPushConstants>() as u32,
                &std::mem::transmute::<_, [u8; 4]>(FragmentPushConstants {
                    use_texture: vk::TRUE,
                }),
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

    pub fn create_framebuffer(
        &self,
        api: &Vulkan,
        color_attachment: vk::ImageView,
        extent: vk::Extent2D,
    ) -> Result<vk::Framebuffer, vk::Result> {
        let create_info = vk::FramebufferCreateInfo {
            render_pass: self.render_pass,
            attachment_count: 1,
            p_attachments: &color_attachment,
            width: extent.width,
            height: extent.height,
            layers: 1,
            ..Default::default()
        };

        unsafe { api.device.create_framebuffer(&create_info, None) }
    }
}

/// Utility struct for a `VkBuffer` suitable for vertices and indices.
pub struct UiGeometryBuffer {
    handle: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
    // first_vertex is assumed to be 0
    index_offset: vk::DeviceSize,
}

impl UiGeometryBuffer {
    const NUM_INIT_VERTICES: vk::DeviceSize = 1024 * 4;
    const NUM_INIT_INDICES: vk::DeviceSize = 1024 * 6;

    /// Allocates a new buffer suitable for 1024 rects (4096 vertices and 6144
    /// indices).
    pub fn new(api: &Vulkan) -> Result<Self, vk::Result> {
        let index_offset = Self::index_offset(api, Self::NUM_INIT_VERTICES);
        let buffer_size = index_offset + Self::index_size(Self::NUM_INIT_INDICES);

        let (handle, memory) = api.allocate_buffer(
            MemoryUsage::Dynamic,
            buffer_size,
            vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
        )?;

        Ok(Self {
            handle,
            memory,
            size: buffer_size,
            index_offset,
        })
    }

    /// Destroys the buffer and frees its memory from the GPU.
    pub fn destroy(self, api: &Vulkan) {
        unsafe {
            api.device.destroy_buffer(self.handle, None);
            api.device.free_memory(self.memory, None);
        }
    }

    /// Copies the vertices and indices into the GPU buffer, resizing as needed
    /// to fit the data.
    ///
    /// This copy _does not_ shrink the buffer, however, as there is no real
    /// usecase for it yet.
    pub(super) fn copy(
        &mut self,
        api: &Vulkan,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> Result<(), vk::Result> {
        let index_offset = Self::index_offset(api, vertices.len() as vk::DeviceSize);
        let required_size = index_offset + Self::index_size(indices.len() as vk::DeviceSize);

        if required_size > self.size {
            unsafe {
                api.device.destroy_buffer(self.handle, None);
                api.device.free_memory(self.memory, None);
            }

            let (handle, memory) = api.allocate_buffer(
                MemoryUsage::Dynamic,
                required_size,
                vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
            )?;

            self.handle = handle;
            self.memory = memory;
        }

        // This may change even if the buffer size doesn't.
        self.index_offset = index_offset;

        unsafe {
            let ptr = api.device.map_memory(
                self.memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?;

            std::slice::from_raw_parts_mut(ptr.cast(), vertices.len()).copy_from_slice(vertices);

            std::slice::from_raw_parts_mut(ptr.add(index_offset as usize).cast(), indices.len())
                .copy_from_slice(indices);

            api.device.unmap_memory(self.memory);
        }

        Ok(())
    }

    /// Calculates the offset offset into a buffer with `n_vertices`.
    fn index_offset(api: &Vulkan, n_vertices: vk::DeviceSize) -> vk::DeviceSize {
        let vertex_bytes = std::mem::size_of::<Vertex>() as vk::DeviceSize * n_vertices;
        next_multiple_of(
            vertex_bytes,
            api.physical_device.properties.limits.non_coherent_atom_size,
        )
    }

    /// Calculates the size of the index buffer.
    fn index_size(n_indices: vk::DeviceSize) -> vk::DeviceSize {
        std::mem::size_of::<u16>() as vk::DeviceSize * n_indices
    }
}
