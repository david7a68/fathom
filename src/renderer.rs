use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    os::raw::c_char,
};

use ash::vk;
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, RECT},
    UI::WindowsAndMessaging::GetClientRect,
};

const VALIDATION_LAYER: *const i8 = b"VK_LAYER_KHRONOS_validation\0".as_ptr().cast();

const INSTANCE_EXTENSIONS: [*const i8; 2] = [
    b"VK_KHR_surface\0".as_ptr().cast(),
    #[cfg(target_os = "windows")]
    b"VK_KHR_win32_surface\0".as_ptr().cast(),
];

const DEVICE_EXTENSIONS: [*const i8; 1] = [b"VK_KHR_swapchain\0".as_ptr().cast()];

const FRAMES_IN_FLIGHT: u32 = 2;
const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

const SHADER_MAIN: *const i8 = b"main\0".as_ptr().cast();
const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

struct Device {
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
    swapchain_api: ash::extensions::khr::Swapchain,

    graphics_family: u32,
    present_family: u32,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    command_pool: vk::CommandPool,
}

struct Pipeline {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
}

struct Swapchain {
    format: vk::Format,
    extent: vk::Extent2D,
    surface: vk::SurfaceKHR,
    image_views: Vec<vk::ImageView>,
    frame_buffers: Vec<vk::Framebuffer>,

    current_frame: u32,
    current_image: u32,

    frames_in_flight: FramesInFlight,
}

struct FramesInFlight {
    command_buffers: [vk::CommandBuffer; DESIRED_SWAPCHAIN_LENGTH as usize],
    /// Semaphores indicating when the swapchain image can be rendered to.
    acquire_semaphores: [vk::Semaphore; DESIRED_SWAPCHAIN_LENGTH as usize],
    /// Semaphores indicating when the image is ready to be presented.
    present_semaphores: [vk::Semaphore; DESIRED_SWAPCHAIN_LENGTH as usize],
    /// Fences indicating when the command buffer is finished.
    fences: [vk::Fence; DESIRED_SWAPCHAIN_LENGTH as usize],
}

pub struct Renderer {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,

    surface_api: ash::extensions::khr::Surface,

    #[cfg(target_os = "windows")]
    os_surface_api: ash::extensions::khr::Win32Surface,

    device: Option<Device>,
    swapchains: HashMap<vk::SwapchainKHR, Swapchain>,
    pipelines: HashMap<vk::Format, Pipeline>,
}

impl Renderer {
    pub fn new() -> Self {
        let entry = unsafe { ash::Entry::load().unwrap() };

        let instance = {
            let app_info =
                vk::ApplicationInfo::builder().api_version(vk::make_api_version(0, 1, 1, 0));

            let mut instance_layers = vec![];

            #[cfg(debug_assertions)]
            {
                let has_layers = has_required_names(
                    &entry.enumerate_instance_layer_properties().unwrap(),
                    |l| &l.layer_name,
                    &[VALIDATION_LAYER],
                );

                if has_layers[0] {
                    instance_layers.push(VALIDATION_LAYER);
                }
            }

            let extensions = INSTANCE_EXTENSIONS;

            {
                let has_required = has_required_names(
                    &entry.enumerate_instance_extension_properties(None).unwrap(),
                    |e| &e.extension_name,
                    &INSTANCE_EXTENSIONS,
                );

                for (index, result) in has_required.iter().enumerate() {
                    assert!(
                        result,
                        "required Vulkan extension not found: {:?}",
                        unsafe { CStr::from_ptr(extensions[index]) }
                    );
                }
            };

            let instance_ci = vk::InstanceCreateInfo::builder()
                .application_info(&app_info)
                .enabled_layer_names(&instance_layers)
                .enabled_extension_names(&extensions);

            unsafe { entry.create_instance(&instance_ci, None).unwrap() }
        };

        let surface_api = { ash::extensions::khr::Surface::new(&entry, &instance) };

        #[cfg(target_os = "windows")]
        let os_surface_api = { ash::extensions::khr::Win32Surface::new(&entry, &instance) };

        Self {
            entry,
            instance,
            surface_api,
            os_surface_api,
            device: None,
            swapchains: HashMap::new(),
            pipelines: HashMap::new(),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn create_swapchain(&mut self, hwnd: HWND, hinstance: HINSTANCE) -> vk::SwapchainKHR {
        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe {
            self.os_surface_api
                .create_win32_surface(&surface_ci, None)
                .unwrap()
        };

        let extent = unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            vk::Extent2D {
                width: u32::try_from(rect.right).unwrap(),
                height: u32::try_from(rect.bottom).unwrap(),
            }
        };

        self.actually_create_swapchain(surface, extent)
    }

    pub fn destroy_swapchain(&mut self, swapchain: vk::SwapchainKHR) {
        if let Some(data) = self.swapchains.remove(&swapchain) {
            self.destroy_swapchain_data(swapchain, data);
        }
    }

    fn destroy_swapchain_data(&self, handle: vk::SwapchainKHR, mut data: Swapchain) {
        let device = self.device.as_ref().unwrap();
        let fif = data.frames_in_flight;

        unsafe {
            device
                .device
                .wait_for_fences(&fif.fences, true, u64::MAX)
                .unwrap();

            for view in data.image_views.drain(..) {
                device.device.destroy_image_view(view, None);
            }

            for framebuffer in data.frame_buffers.drain(..) {
                device.device.destroy_framebuffer(framebuffer, None);
            }

            device
                .device
                .free_command_buffers(device.command_pool, &fif.command_buffers);

            for i in 0..FRAMES_IN_FLIGHT as usize {
                device
                    .device
                    .destroy_semaphore(fif.acquire_semaphores[i], None);
                device
                    .device
                    .destroy_semaphore(fif.present_semaphores[i], None);
                device.device.destroy_fence(fif.fences[i], None);
            }

            device.swapchain_api.destroy_swapchain(handle, None);
            self.surface_api.destroy_surface(data.surface, None);
        }
    }

    pub fn begin_frame(&mut self, swapchain: vk::SwapchainKHR) {
        let device = self.device.as_ref().unwrap();
        let swapchain_data = self.swapchains.get_mut(&swapchain).unwrap();

        let frame_idx = swapchain_data.current_frame as usize % DESIRED_SWAPCHAIN_LENGTH as usize;

        let fif = &mut swapchain_data.frames_in_flight;

        unsafe {
            device
                .device
                .wait_for_fences(&[fif.fences[frame_idx]], true, u64::MAX)
                .unwrap();
            device
                .device
                .reset_fences(&[fif.fences[frame_idx]])
                .unwrap();
        }

        let (index, _needs_resize) = unsafe {
            device
                .swapchain_api
                .acquire_next_image(
                    swapchain,
                    u64::MAX,
                    fif.acquire_semaphores[frame_idx],
                    vk::Fence::null(),
                )
                .unwrap()
        };

        swapchain_data.current_image = index;
    }

    pub fn end_frame(&mut self, swapchain: vk::SwapchainKHR) {
        let device = self.device.as_ref().unwrap();
        let swapchain_data = self.swapchains.get_mut(&swapchain).unwrap();
        let pipeline = self.pipelines.get(&swapchain_data.format).unwrap();

        let frame_idx = swapchain_data.current_frame as usize % DESIRED_SWAPCHAIN_LENGTH as usize;

        let fif = &mut swapchain_data.frames_in_flight;

        let command_buffer = fif.command_buffers[frame_idx];

        let command_buffer_bi = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            device
                .device
                .begin_command_buffer(command_buffer, &command_buffer_bi)
                .unwrap();
        }

        let clear_colors = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0],
            },
        }];

        let render_pass_bi = vk::RenderPassBeginInfo::builder()
            .render_pass(pipeline.render_pass)
            .framebuffer(swapchain_data.frame_buffers[frame_idx])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: swapchain_data.extent,
            })
            .clear_values(&clear_colors);

        unsafe {
            device.device.cmd_begin_render_pass(
                command_buffer,
                &render_pass_bi,
                vk::SubpassContents::INLINE,
            );

            device.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                pipeline.pipeline,
            );
        }

        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: swapchain_data.extent.width as f32,
            height: swapchain_data.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };

        unsafe {
            device
                .device
                .cmd_set_viewport(command_buffer, 0, &[viewport]);
        }

        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swapchain_data.extent,
        };

        unsafe {
            device.device.cmd_set_scissor(command_buffer, 0, &[scissor]);
        }

        unsafe {
            device.device.cmd_draw(command_buffer, 3, 1, 0, 0);
            device.device.cmd_end_render_pass(command_buffer);
            device.device.end_command_buffer(command_buffer).unwrap();
        }

        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];

        let submit_info = [vk::SubmitInfo::builder()
            .command_buffers(&[command_buffer])
            .wait_semaphores(&[fif.acquire_semaphores[frame_idx]])
            .signal_semaphores(&[fif.present_semaphores[frame_idx]])
            .wait_dst_stage_mask(&wait_stages)
            .build()];

        unsafe {
            device
                .device
                .queue_submit(device.graphics_queue, &submit_info, fif.fences[frame_idx])
                .unwrap();
        }

        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(&[fif.present_semaphores[frame_idx]])
            .swapchains(&[swapchain])
            .image_indices(&[swapchain_data.current_image])
            .build();

        unsafe {
            device
                .swapchain_api
                .queue_present(device.present_queue, &present_info)
                .unwrap();
        }

        swapchain_data.current_frame += 1;
    }

    fn initialize(&self, surface: vk::SurfaceKHR) -> Device {
        let selected_device = {
            let mut selected_device = None;

            for physical_device in unsafe { self.instance.enumerate_physical_devices().unwrap() } {
                let mut found_present_family = false;
                let mut found_graphics_family = false;
                let mut present_family = 0;
                let mut graphics_family = 0;

                let queue_families = unsafe {
                    self.instance
                        .get_physical_device_queue_family_properties(physical_device)
                };

                for (index, queue_family) in queue_families.iter().enumerate().rev() {
                    let index = index.try_into().unwrap();

                    if unsafe {
                        self.surface_api
                            .get_physical_device_surface_support(physical_device, index, surface)
                            .unwrap()
                    } {
                        found_present_family = true;
                        present_family = index;
                    };

                    if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                        found_graphics_family = true;
                        graphics_family = index;
                    }

                    if found_graphics_family && found_present_family {
                        break;
                    }
                }

                if !found_present_family || !found_graphics_family {
                    continue;
                }

                let supports_swapchain = has_required_names(
                    &unsafe {
                        self.instance
                            .enumerate_device_extension_properties(physical_device)
                            .unwrap()
                    },
                    |e| &e.extension_name,
                    &DEVICE_EXTENSIONS,
                )[0];

                if !supports_swapchain {
                    continue;
                }

                selected_device = Some((physical_device, graphics_family, present_family));
                break;
            }

            selected_device
        };

        let (physical_device, graphics_family, present_family) =
            if let Some((physical_device, present_family, graphics_family)) = selected_device {
                (physical_device, graphics_family, present_family)
            } else {
                panic!("no suitable device found");
            };

        let device = {
            let queue_priority = 1.0;

            let queue_ci = [
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(graphics_family)
                    .queue_priorities(&[queue_priority])
                    .build(),
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(present_family)
                    .queue_priorities(&[queue_priority])
                    .build(),
            ];

            let num_queues = 1 + usize::from(present_family != graphics_family);
            let device_ci = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queue_ci[0..num_queues])
                .enabled_extension_names(&DEVICE_EXTENSIONS);

            unsafe {
                self.instance
                    .create_device(physical_device, &device_ci, None)
                    .unwrap()
            }
        };

        let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
        let present_queue = unsafe { device.get_device_queue(present_family, 0) };

        let swapchain_api = { ash::extensions::khr::Swapchain::new(&self.instance, &device) };

        let command_pool = {
            let pool_ci = vk::CommandPoolCreateInfo::builder()
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                .queue_family_index(graphics_family);

            unsafe { device.create_command_pool(&pool_ci, None).unwrap() }
        };

        Device {
            device,
            physical_device,
            swapchain_api,
            graphics_family,
            present_family,
            graphics_queue,
            present_queue,
            command_pool,
        }
    }

    fn create_pipeline(device: &ash::Device, swapchain_format: vk::Format) -> Pipeline {
        let layout = {
            let pipeline_layout_ci = vk::PipelineLayoutCreateInfo::builder();

            unsafe {
                device
                    .create_pipeline_layout(&pipeline_layout_ci, None)
                    .unwrap()
            }
        };

        let render_pass = {
            let render_pass_attachments = [vk::AttachmentDescription::builder()
                .format(swapchain_format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .build()];

            let attachment_references = [vk::AttachmentReference::builder()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .build()];

            let dependencies = [vk::SubpassDependency::builder()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::NONE)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .build()];

            let subpass_ci = [vk::SubpassDescription::builder()
                .color_attachments(&attachment_references)
                .build()];

            let render_pass_ci = vk::RenderPassCreateInfo::builder()
                .attachments(&render_pass_attachments)
                .subpasses(&subpass_ci)
                .dependencies(&dependencies);

            unsafe { device.create_render_pass(&render_pass_ci, None).unwrap() }
        };

        let pipeline = {
            let vertex_shader = {
                debug_assert_eq!(UI_VERT_SHADER_SPV.len() % 4, 0);
                let shader_words = unsafe {
                    std::slice::from_raw_parts(
                        UI_VERT_SHADER_SPV.as_ptr().cast(),
                        UI_VERT_SHADER_SPV.len() / 4,
                    )
                };

                let module_ci = vk::ShaderModuleCreateInfo::builder().code(shader_words);

                unsafe { device.create_shader_module(&module_ci, None).unwrap() }
            };

            let fragment_shader = {
                debug_assert_eq!(UI_FRAG_SHADER_SPV.len() % 4, 0);
                let shader_words = unsafe {
                    std::slice::from_raw_parts(
                        UI_FRAG_SHADER_SPV.as_ptr().cast(),
                        UI_FRAG_SHADER_SPV.len() / 4,
                    )
                };

                let module_ci = vk::ShaderModuleCreateInfo::builder().code(shader_words);

                unsafe { device.create_shader_module(&module_ci, None).unwrap() }
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

            let vertex_input_ci = vk::PipelineVertexInputStateCreateInfo::builder();

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

            let framebuffer_blend_ci = [vk::PipelineColorBlendAttachmentState::builder()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(false)
                .build()];

            let global_blend_ci = vk::PipelineColorBlendStateCreateInfo::builder()
                .logic_op_enable(false)
                .attachments(&framebuffer_blend_ci);

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

            let pipeline = unsafe {
                device
                    .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_ci], None)
                    .unwrap()[0]
            };

            unsafe {
                device.destroy_shader_module(vertex_shader, None);
                device.destroy_shader_module(fragment_shader, None);
            }

            pipeline
        };

        Pipeline {
            pipeline,
            layout,
            render_pass,
        }
    }

    fn actually_create_swapchain(
        &mut self,
        surface: vk::SurfaceKHR,
        extent: vk::Extent2D,
    ) -> vk::SwapchainKHR {
        let device = self.device.get_or_insert(self.initialize(surface));

        let format = {
            let formats = unsafe {
                self.surface_api
                    .get_physical_device_surface_formats(device.physical_device, surface)
                    .unwrap()
            };

            assert!(!formats.is_empty());

            formats
                .iter()
                .find_map(|f| (f.format == vk::Format::B8G8R8A8_SRGB).then_some(*f))
                .unwrap_or(formats[0])
        };

        let (extent, transform, swapchain_length) = {
            let capabilities = unsafe {
                self.surface_api
                    .get_physical_device_surface_capabilities(device.physical_device, surface)
                    .unwrap()
            };

            let extent = if capabilities.current_extent.width == u32::MAX {
                vk::Extent2D {
                    width: extent.width.clamp(
                        capabilities.min_image_extent.width,
                        capabilities.max_image_extent.width,
                    ),
                    height: extent.height.clamp(
                        capabilities.min_image_extent.height,
                        capabilities.max_image_extent.height,
                    ),
                }
            } else {
                capabilities.current_extent
            };

            assert!(
                extent.width > 0 && extent.height > 0,
                "a swapchain must always have nonzero extent"
            );

            let length = if capabilities.max_image_count == 0
                || capabilities.min_image_count <= DESIRED_SWAPCHAIN_LENGTH
            {
                DESIRED_SWAPCHAIN_LENGTH
            } else {
                capabilities.min_image_count
            };

            (extent, capabilities.current_transform, length)
        };

        let concurrent_family_indices = &[device.graphics_family, device.present_family];
        let swapchain_ci = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(swapchain_length)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .queue_family_indices(concurrent_family_indices)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);

        let swapchain_ci = if device.graphics_family == device.present_family {
            swapchain_ci.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        } else {
            swapchain_ci.image_sharing_mode(vk::SharingMode::CONCURRENT)
        };

        let swapchain = unsafe {
            device
                .swapchain_api
                .create_swapchain(&swapchain_ci, None)
                .unwrap()
        };

        let image_views = {
            let images = unsafe {
                device
                    .swapchain_api
                    .get_swapchain_images(swapchain)
                    .unwrap()
            };

            let mut views = Vec::with_capacity(images.len());

            for image in &images {
                let view_ci = vk::ImageViewCreateInfo::builder()
                    .image(*image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });

                let view = unsafe { device.device.create_image_view(&view_ci, None).unwrap() };
                views.push(view);
            }
            views
        };

        let pipeline = if let Some(pipeline) = self.pipelines.get(&format.format) {
            pipeline
        } else {
            self.pipelines.insert(
                format.format,
                Self::create_pipeline(&device.device, format.format),
            );
            self.pipelines.get(&format.format).unwrap()
        };

        let frame_buffers = {
            let mut frame_buffers = Vec::with_capacity(image_views.len());

            for view in &image_views {
                let attachments = [*view];

                let framebuffer_ci = vk::FramebufferCreateInfo::builder()
                    .render_pass(pipeline.render_pass)
                    .attachments(&attachments)
                    .width(extent.width)
                    .height(extent.height)
                    .layers(1);

                let framebuffer = unsafe {
                    device
                        .device
                        .create_framebuffer(&framebuffer_ci, None)
                        .unwrap()
                };
                frame_buffers.push(framebuffer);
            }
            frame_buffers
        };

        let frames_in_flight = {
            let command_buffers = {
                let mut array = [vk::CommandBuffer::null(); FRAMES_IN_FLIGHT as usize];

                let command_buffer_ai = vk::CommandBufferAllocateInfo::builder()
                    .command_pool(device.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(FRAMES_IN_FLIGHT)
                    .build();

                // Note(straivers): This is implemented using the function pointer
                // directly in order to avoid allocating a `Vec` since we know the
                // size of the array statically.

                let vk_result = unsafe {
                    (device.device.fp_v1_0().allocate_command_buffers)(
                        device.device.handle(),
                        &command_buffer_ai,
                        array.as_mut_ptr(),
                    )
                };

                if vk_result != vk::Result::SUCCESS {
                    panic!("failed to allocate command buffers");
                }

                array
            };

            let semaphore_ci = vk::SemaphoreCreateInfo::builder();
            let fence_ci = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

            let mut acquire_semaphores = [vk::Semaphore::null(); FRAMES_IN_FLIGHT as usize];
            let mut present_semaphores = [vk::Semaphore::null(); FRAMES_IN_FLIGHT as usize];
            let mut fences = [vk::Fence::null(); FRAMES_IN_FLIGHT as usize];

            for i in 0..FRAMES_IN_FLIGHT as usize {
                unsafe {
                    acquire_semaphores[i] =
                        device.device.create_semaphore(&semaphore_ci, None).unwrap();
                    present_semaphores[i] =
                        device.device.create_semaphore(&semaphore_ci, None).unwrap();
                    fences[i] = device.device.create_fence(&fence_ci, None).unwrap();
                }
            }

            FramesInFlight {
                command_buffers,
                acquire_semaphores,
                present_semaphores,
                fences,
            }
        };

        self.swapchains.insert(
            swapchain,
            Swapchain {
                surface,
                extent,
                format: format.format,
                image_views,
                frame_buffers,
                current_frame: 0,
                current_image: 0,
                frames_in_flight,
            },
        );
        swapchain
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            unsafe {
                device.device.device_wait_idle().unwrap();
            }

            for (handle, data) in std::mem::take(&mut self.swapchains) {
                self.destroy_swapchain_data(handle, data);
            }

            for (_, pipeline) in std::mem::take(&mut self.pipelines) {
                unsafe {
                    device.device.destroy_pipeline(pipeline.pipeline, None);
                    device
                        .device
                        .destroy_render_pass(pipeline.render_pass, None);
                    device.device.destroy_pipeline_layout(pipeline.layout, None);
                }
            }

            unsafe {
                device
                    .device
                    .destroy_command_pool(device.command_pool, None);
                device.device.destroy_device(None);
            }
        }

        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

fn has_required_names<T, F: Fn(&T) -> &[c_char], const N: usize>(
    items: &[T],
    to_name: F,
    names: &[*const c_char; N],
) -> [bool; N] {
    let mut item_set = HashSet::new();

    for name in items.iter().map(to_name) {
        item_set.insert(unsafe { CStr::from_ptr(name.as_ptr()) });
    }

    let mut results = [false; N];
    for i in 0..names.len() {
        results[i] = item_set.contains(unsafe { CStr::from_ptr(names[i]) });
    }

    results
}
