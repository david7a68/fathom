//! # Textures and their Storage
//!
//! ## Assumptions/Requirements
//!
//! - all textures stored in RGBA_F16_LINEAR format for simplicity
//! - must accept all formats used by PixelBuffer (and convert)
//! - must permit updating of subtextures in bulk
//!
//! ## Using Textures
//!
//! A [`Texture`] is a 2D image that has been uploaded to the GPU and can be
//! used for rendering. This uploading process requires some state and so is
//! done through the [`Staging`] manager. Notably, we enforce the same
//! mutability rules on textures as Rust does on memory. That is to say, memory
//! may either be modified by one user, or read by many. Here, this is
//! accomplished through the use of a timeline semaphore to track the number of
//! concurrent reads, as well as an optional timeline semaphore to track writes.
//!
//! The use of semaphores allows us to describe dependencies such that a write
//! may be scheduled to occur-after reads have completed, or reads to
//! occur-after a write has completed.

use std::io::Write;

use arrayvec::ArrayVec;
use ash::vk;
use smallvec::SmallVec;

use crate::gfx::{
    geometry::{Extent, Offset},
    pixel_buffer::{Layout, PixelBufferView},
};

use super::{
    api::{MemoryUsage, VkResult, Vulkan},
    as_cchar_slice,
};

const STORAGE_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

pub struct Texture {
    image: vk::Image,
    image_view: vk::ImageView,
    image_layout: vk::ImageLayout,
    memory: vk::DeviceMemory,
    /// A timeline semaphore used to track read operations. If
    /// `read_semaphore==read_count`, the texture is not currently being read
    /// and can be used for write operations.
    pub read_semaphore: vk::Semaphore,
    /// A count of the number of read operations executed on this texture.
    pub read_count: u64,
    pub write_state: Option<WriteState>,
}

impl Texture {
    pub fn new(api: &Vulkan, extent: Extent) -> VkResult<Self> {
        let image = {
            let create_info = vk::ImageCreateInfo {
                flags: vk::ImageCreateFlags::empty(),
                image_type: vk::ImageType::TYPE_2D,
                format: STORAGE_FORMAT,
                extent: vk::Extent3D {
                    width: extent.width.0 as u32,
                    height: extent.height.0 as u32,
                    depth: 1,
                },
                mip_levels: 1,
                array_layers: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                tiling: vk::ImageTiling::OPTIMAL,
                usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::STORAGE,
                initial_layout: vk::ImageLayout::UNDEFINED,
                ..Default::default()
            };

            unsafe { api.device.create_image(&create_info, None) }?
        };

        let memory = {
            let requirements = unsafe { api.device.get_image_memory_requirements(image) };
            api.allocate_memory(MemoryUsage::Static, requirements)?
        };

        unsafe { api.device.bind_image_memory(image, memory, 0) }?;

        let image_view = api.create_image_view(image, STORAGE_FORMAT)?;
        let read_semaphore = api.create_semaphore(true)?;

        Ok(Self {
            image,
            image_view,
            image_layout: vk::ImageLayout::UNDEFINED,
            memory,
            read_semaphore,
            read_count: 0,
            write_state: None,
        })
    }

    pub fn is_idle(&self, api: &Vulkan) -> VkResult<bool> {
        let write_idle = if let Some(write_state) = &self.write_state {
            unsafe {
                api.device
                    .get_semaphore_counter_value(write_state.semaphore)
            }? == write_state.counter
        } else {
            true
        };

        let read_count = unsafe { api.device.get_semaphore_counter_value(self.read_semaphore) }?;
        Ok(write_idle && read_count == self.read_count)
    }

    pub fn wait_idle(&self, api: &Vulkan) -> VkResult<()> {
        let (write_semaphore, write_value) = self
            .write_state
            .as_ref()
            .map_or((vk::Semaphore::null(), 0), |s| (s.semaphore, s.counter));

        let semaphores = [self.read_semaphore, write_semaphore];
        let values = [self.read_count, write_value];

        unsafe {
            api.device.wait_semaphores(
                &vk::SemaphoreWaitInfo {
                    semaphore_count: 1 + u32::from(self.write_state.is_some()),
                    p_semaphores: semaphores.as_ptr(),
                    p_values: values.as_ptr(),
                    ..Default::default()
                },
                u64::MAX,
            )
        }
    }

    pub fn destroy(self, api: &Vulkan) {
        assert!(
            self.is_idle(api).unwrap(),
            "must not destory an image that is in use"
        );
        assert_eq!(self.write_state, None);

        unsafe {
            api.device.destroy_image_view(self.image_view, None);
            api.device.destroy_image(self.image, None);
            api.device.free_memory(self.memory, None);
            api.device.destroy_semaphore(self.read_semaphore, None);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WriteState {
    /// tracks the value that the semaphore must reach
    pub counter: u64,
    /// used to determine if/when the write is complete
    pub semaphore: vk::Semaphore,
    /// descriptors (uniforms) used for the write, one per region
    pub descriptors: SmallVec<[Descriptor; 2]>,
    /// command buffer holding commands for this write, can be reset once
    /// `semaphore==counter`
    pub command_buffer: vk::CommandBuffer,
}

impl WriteState {
    pub fn is_complete(&self, api: &Vulkan) -> VkResult<bool> {
        unsafe {
            api.device.wait_semaphores(
                &vk::SemaphoreWaitInfo {
                    semaphore_count: 1,
                    p_semaphores: &self.semaphore,
                    p_values: &self.counter,
                    ..Default::default()
                },
                0,
            )
        }
        .map(|_| true)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub handle: vk::DescriptorSet,
    pub extent_buffer_offset: vk::DeviceSize,
    pub target_sampler: vk::Sampler,
}

#[repr(C)]
struct CopyUniforms {
    pub source_extent: Extent,
    pub target_offset: Offset,
}

pub struct Staging {
    rgb_pipeline: vk::Pipeline,
    rgb_pipeline_layout: vk::PipelineLayout,
    // rgba_pipeline: vk::Pipeline,
    // rgba_pipeline_layout: vk::PipelineLayout,
    command_pool: vk::CommandPool,

    sampler: vk::Sampler,

    extent_buffer: vk::Buffer,
    extent_memory: vk::DeviceMemory,
    extent_memory_ptr: *mut std::ffi::c_void,

    descriptor_pool: vk::DescriptorPool,
    descriptor_layout: vk::DescriptorSetLayout,

    io_pool: SmallVec<[WriteState; 16]>,
    descriptors: ArrayVec<Descriptor, { Self::MAX_DESCRIPTORS as usize }>,
}

impl Staging {
    const MAX_CONCURRENT_IO: u32 = 128;
    const MAX_DESCRIPTORS: u32 = Self::MAX_CONCURRENT_IO * 4;

    const RGB_UINT_SHADER: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/image_upload_uint.spv"));

    pub fn new(api: &Vulkan) -> VkResult<Self> {
        let descriptor_layout = {
            let bindings = [
                vk::DescriptorSetLayoutBinding {
                    binding: 0,
                    descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                },
                vk::DescriptorSetLayoutBinding {
                    binding: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                },
                vk::DescriptorSetLayoutBinding {
                    binding: 2,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                },
            ];

            let create_info = vk::DescriptorSetLayoutCreateInfo {
                binding_count: bindings.len() as u32,
                p_bindings: bindings.as_ptr(),
                ..Default::default()
            };

            unsafe { api.device.create_descriptor_set_layout(&create_info, None) }?
        };

        let descriptor_pool = {
            let pool_size = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: Self::MAX_DESCRIPTORS,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: Self::MAX_DESCRIPTORS,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: Self::MAX_DESCRIPTORS,
                },
            ];

            let create_info = vk::DescriptorPoolCreateInfo {
                flags: vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND,
                max_sets: Self::MAX_DESCRIPTORS,
                pool_size_count: pool_size.len() as u32,
                p_pool_sizes: pool_size.as_ptr(),
                ..Default::default()
            };

            unsafe { api.device.create_descriptor_pool(&create_info, None) }?
        };

        let command_pool = {
            let create_info = vk::CommandPoolCreateInfo {
                flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
                queue_family_index: api.physical_device.graphics_queue_family,
                ..Default::default()
            };

            unsafe { api.device.create_command_pool(&create_info, None) }?
        };

        // Created empy. To minimize allocations & waste, only allocate as
        // needed.
        let io_pool = SmallVec::new();

        let descriptors = {
            let layouts = [descriptor_layout; Self::MAX_DESCRIPTORS as usize];
            let create_info = vk::DescriptorSetAllocateInfo {
                descriptor_pool,
                descriptor_set_count: Self::MAX_DESCRIPTORS,
                p_set_layouts: layouts.as_ptr(),
                ..Default::default()
            };

            let mut sets = [Default::default(); Self::MAX_DESCRIPTORS as usize];
            unsafe {
                (api.device.fp_v1_0().allocate_descriptor_sets)(
                    api.device.handle(),
                    &create_info,
                    sets.as_mut_ptr(),
                )
                .result()?;
            }

            ArrayVec::<_, { Self::MAX_DESCRIPTORS as usize }>::from_iter(
                sets.iter().enumerate().map(|(i, set)| Descriptor {
                    handle: *set,
                    target_sampler: vk::Sampler::null(),
                    extent_buffer_offset: (i * std::mem::size_of::<CopyUniforms>())
                        as vk::DeviceSize,
                }),
            )
        };

        let (extent_buffer, extent_memory) = api.allocate_buffer(
            MemoryUsage::Dynamic,
            (std::mem::size_of::<CopyUniforms>() as u32 * Self::MAX_DESCRIPTORS).into(),
            vk::BufferUsageFlags::UNIFORM_BUFFER,
        )?;

        let extent_memory_ptr = unsafe {
            api.device
                .bind_buffer_memory(extent_buffer, extent_memory, 0)?;

            api.device.map_memory(
                extent_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?
        };

        let (rgb_pipeline, rgb_pipeline_layout) =
            Self::create_rgb8_pipeline(api, descriptor_layout)?;

        let sampler = {
            let create_info = vk::SamplerCreateInfo {
                mag_filter: vk::Filter::LINEAR,
                min_filter: vk::Filter::LINEAR,
                mipmap_mode: vk::SamplerMipmapMode::NEAREST,
                address_mode_u: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                address_mode_v: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                address_mode_w: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                mip_lod_bias: 0.0,
                anisotropy_enable: vk::FALSE,
                max_anisotropy: 0.0,
                compare_enable: vk::FALSE,
                compare_op: vk::CompareOp::NEVER,
                min_lod: 0.0,
                max_lod: 0.0,
                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                unnormalized_coordinates: vk::TRUE,
                ..Default::default()
            };

            unsafe { api.device.create_sampler(&create_info, None) }?
        };

        Ok(Self {
            rgb_pipeline,
            rgb_pipeline_layout,
            command_pool,
            sampler,
            extent_buffer,
            extent_memory,
            extent_memory_ptr,
            descriptor_pool,
            descriptor_layout,
            io_pool,
            descriptors,
        })
    }

    pub fn destroy(&mut self, api: &Vulkan) {
        unsafe {
            api.device.destroy_pipeline(self.rgb_pipeline, None);
            api.device
                .destroy_pipeline_layout(self.rgb_pipeline_layout, None);
            api.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            api.device
                .destroy_descriptor_set_layout(self.descriptor_layout, None);
            api.device.destroy_buffer(self.extent_buffer, None);
            api.device.free_memory(self.extent_memory, None);
            self.io_pool.clear();
        }
    }

    pub fn finish(&mut self, mut state: WriteState) {
        self.descriptors.extend(state.descriptors.drain(..));
        self.io_pool.push(state);
    }

    pub fn copy_pixels(
        &mut self,
        api: &Vulkan,
        src: PixelBufferView,
        dst: &mut Texture,
        ops: &[crate::gfx::ImageCopy],
    ) -> VkResult<()> {
        let mut pixels_to_copy = 0;
        for op in ops {
            pixels_to_copy += op.src_rect.extent().area();
        }

        let bytes_to_copy = (pixels_to_copy * src.layout().bytes_per_pixel()) as vk::DeviceSize;
        let (buffer, memory) = api.allocate_buffer(
            MemoryUsage::Once,
            bytes_to_copy,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let mut bytes_written = 0;
        let map = unsafe {
            api.device
                .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
        }?
        .cast::<u8>();

        for op in ops {
            for bytes in src.subrect(op.src_rect).bytes() {
                unsafe { std::slice::from_raw_parts_mut(map.add(bytes_written), bytes.len()) }
                    .copy_from_slice(bytes);
                bytes_written += bytes.len();
                assert!(bytes_written <= bytes_to_copy as usize);
            }
        }

        unsafe {
            api.device
                .flush_mapped_memory_ranges(&[vk::MappedMemoryRange {
                    memory,
                    offset: 0,
                    size: vk::WHOLE_SIZE,
                    ..Default::default()
                }])?;

            api.device.unmap_memory(memory);
        }

        let mut io_state = self.io_pool.pop().expect("out of descriptors!");
        io_state.descriptors.reserve(ops.len());

        assert!(
            self.descriptors.len() >= ops.len(),
            "out of staging descriptors!"
        );

        unsafe {
            api.device.begin_command_buffer(
                io_state.command_buffer,
                &vk::CommandBufferBeginInfo {
                    flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                    ..Default::default()
                },
            )?;

            let pipeline = match src.layout() {
                Layout::RGB8 => self.rgb_pipeline,
                Layout::RGBA8 => todo!(),
            };

            api.device.cmd_bind_pipeline(
                io_state.command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline,
            );

            api.device.cmd_pipeline_barrier(
                io_state.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::BY_REGION,
                &[],
                &[],
                &[vk::ImageMemoryBarrier {
                    src_access_mask: vk::AccessFlags::SHADER_READ,
                    dst_access_mask: vk::AccessFlags::SHADER_WRITE,
                    old_layout: dst.image_layout,
                    new_layout: vk::ImageLayout::GENERAL,
                    src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    image: dst.image,
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    ..Default::default()
                }],
            );

            let mut bytes_copied = 0;
            for op in ops {
                let bytes_to_copy = op.src_rect.extent().area() as vk::DeviceSize;
                let descriptor = self.descriptors.pop().unwrap();

                let uniforms = vk::DescriptorBufferInfo {
                    buffer: self.extent_buffer,
                    offset: descriptor.extent_buffer_offset,
                    range: std::mem::size_of::<CopyUniforms>() as vk::DeviceSize,
                };

                std::slice::from_raw_parts_mut(
                    self.extent_memory_ptr
                        .add(descriptor.extent_buffer_offset as usize)
                        .cast(),
                    std::mem::size_of::<CopyUniforms>(),
                )
                .write_all(&std::mem::transmute::<
                    CopyUniforms,
                    [u8; std::mem::size_of::<CopyUniforms>()],
                >(CopyUniforms {
                    source_extent: op.src_rect.extent(),
                    target_offset: op.dst_location,
                }))
                .unwrap();

                api.device
                    .flush_mapped_memory_ranges(&[vk::MappedMemoryRange {
                        memory,
                        offset: descriptor.extent_buffer_offset,
                        size: std::mem::size_of::<CopyUniforms>() as vk::DeviceSize,
                        ..Default::default()
                    }])?;

                let source = vk::DescriptorBufferInfo {
                    buffer,
                    offset: bytes_copied,
                    range: bytes_to_copy,
                };

                let target = vk::DescriptorImageInfo {
                    sampler: self.sampler,
                    image_view: dst.image_view,
                    image_layout: vk::ImageLayout::GENERAL,
                };

                api.device.update_descriptor_sets(
                    &[
                        vk::WriteDescriptorSet {
                            dst_set: descriptor.handle,
                            dst_binding: 0,
                            dst_array_element: 0,
                            descriptor_count: 1,
                            descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                            p_buffer_info: &uniforms,
                            ..Default::default()
                        },
                        vk::WriteDescriptorSet {
                            dst_set: descriptor.handle,
                            dst_binding: 1,
                            dst_array_element: 0,
                            descriptor_count: 1,
                            descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                            p_buffer_info: &source,
                            ..Default::default()
                        },
                        vk::WriteDescriptorSet {
                            dst_set: descriptor.handle,
                            dst_binding: 1,
                            dst_array_element: 0,
                            descriptor_count: 1,
                            descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                            p_image_info: &target,
                            ..Default::default()
                        },
                    ],
                    &[],
                );

                api.device.cmd_bind_descriptor_sets(
                    io_state.command_buffer,
                    vk::PipelineBindPoint::COMPUTE,
                    self.rgb_pipeline_layout,
                    0,
                    &[descriptor.handle],
                    &[0],
                );

                bytes_copied += bytes_to_copy;

                let work_group_x =
                    (op.src_rect.width().0 as u32 / 32) + u32::from(op.src_rect.width() % 32 > 0);

                let work_group_y =
                    (op.src_rect.height().0 as u32 / 32) + u32::from(op.src_rect.height() % 32 > 0);

                api.device
                    .cmd_dispatch(io_state.command_buffer, work_group_x, work_group_y, 1);

                io_state.descriptors.push(descriptor);
            }

            api.device.cmd_pipeline_barrier(
                io_state.command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::BY_REGION,
                &[],
                &[],
                &[vk::ImageMemoryBarrier {
                    src_access_mask: vk::AccessFlags::SHADER_WRITE,
                    dst_access_mask: vk::AccessFlags::SHADER_READ,
                    old_layout: vk::ImageLayout::GENERAL,
                    new_layout: vk::ImageLayout::READ_ONLY_OPTIMAL,
                    src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    image: dst.image,
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    ..Default::default()
                }],
            );

            api.device.end_command_buffer(io_state.command_buffer)?;
        }

        let mut wait_values = ArrayVec::<_, 2>::new();
        let mut wait_semaphores = ArrayVec::<_, 2>::new();

        wait_values.push(dst.read_count);
        wait_semaphores.push(dst.read_semaphore);

        let old = dst.write_state.take();

        if let Some(write) = old {
            wait_values.push(write.counter);
            wait_semaphores.push(write.semaphore);
        }

        io_state.counter += 1;
        let timeline_info = vk::TimelineSemaphoreSubmitInfo {
            wait_semaphore_value_count: wait_values.len() as u32,
            p_wait_semaphore_values: wait_values.as_ptr(),
            signal_semaphore_value_count: 1,
            p_signal_semaphore_values: &io_state.counter,
            ..Default::default()
        };

        let submit = vk::SubmitInfo {
            p_next: &timeline_info as *const _ as *const _,
            wait_semaphore_count: wait_values.len() as u32,
            p_wait_semaphores: wait_semaphores.as_ptr(),
            p_signal_semaphores: &io_state.semaphore,
            p_wait_dst_stage_mask: &vk::PipelineStageFlags::COMPUTE_SHADER,
            command_buffer_count: 1,
            p_command_buffers: &io_state.command_buffer,
            ..Default::default()
        };

        dst.image_layout = vk::ImageLayout::READ_ONLY_OPTIMAL;
        dst.write_state = Some(io_state);

        unsafe {
            api.device
                .queue_submit(api.graphics_queue, &[submit], vk::Fence::null())
        }?;

        Ok(())
    }

    fn create_rgb8_pipeline(
        api: &Vulkan,
        descriptor_layout: vk::DescriptorSetLayout,
    ) -> VkResult<(vk::Pipeline, vk::PipelineLayout)> {
        let layout = {
            let create_info = vk::PipelineLayoutCreateInfo {
                set_layout_count: 1,
                p_set_layouts: &descriptor_layout,
                ..Default::default()
            };

            unsafe { api.device.create_pipeline_layout(&create_info, None) }?
        };

        let shader = vk::ShaderModuleCreateInfo {
            code_size: Self::RGB_UINT_SHADER.len(),
            p_code: Self::RGB_UINT_SHADER.as_ptr().cast(),
            ..Default::default()
        };

        let specialization_constants: [u32; 2] = [
            3,   // num_channels
            255, // channel_range_max
        ];

        let entries = [
            vk::SpecializationMapEntry {
                constant_id: 0,
                offset: 0,
                size: std::mem::size_of::<u32>(),
            },
            vk::SpecializationMapEntry {
                constant_id: 1,
                offset: std::mem::size_of::<u32>() as u32,
                size: std::mem::size_of::<u32>(),
            },
        ];

        let specialization = vk::SpecializationInfo {
            map_entry_count: 2,
            p_map_entries: entries.as_ptr(),
            data_size: std::mem::size_of_val(&entries),
            p_data: specialization_constants.as_ptr().cast(),
        };

        let stage = vk::PipelineShaderStageCreateInfo {
            p_next: &shader as *const _ as *const _,
            stage: vk::ShaderStageFlags::COMPUTE,
            module: vk::ShaderModule::null(),
            p_name: as_cchar_slice(b"main\0").as_ptr(),
            p_specialization_info: &specialization,
            ..Default::default()
        };

        let create_info = vk::ComputePipelineCreateInfo {
            stage,
            layout,
            ..Default::default()
        };

        let mut pipeline = vk::Pipeline::null();
        unsafe {
            (api.device.fp_v1_0().create_compute_pipelines)(
                api.device.handle(),
                api.pipeline_cache,
                1,
                &create_info,
                std::ptr::null(),
                &mut pipeline,
            )
        }
        .result()?;

        Ok((pipeline, layout))
    }

    fn alloc_write_state(&mut self, api: &Vulkan) -> VkResult<WriteState> {
        if let Some(state) = self.io_pool.pop() {
            assert!(state.descriptors.is_empty());
            Ok(state)
        } else {
            let semaphore = api.create_semaphore(true)?;
            let command_buffer = api
                .allocate_command_buffer(self.command_pool)
                .map_err(|e| {
                    unsafe { api.device.destroy_semaphore(semaphore, None) };
                    e
                })?;

            Ok(WriteState {
                counter: 0,
                semaphore,
                descriptors: SmallVec::new(),
                command_buffer,
            })
        }
    }
}
