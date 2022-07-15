mod error;
mod pipeline;

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

use self::error::Error;

const VALIDATION_LAYER: *const i8 = b"VK_LAYER_KHRONOS_validation\0".as_ptr().cast();

const INSTANCE_EXTENSIONS: [*const i8; 2] = [
    b"VK_KHR_surface\0".as_ptr().cast(),
    #[cfg(target_os = "windows")]
    b"VK_KHR_win32_surface\0".as_ptr().cast(),
];

const DEVICE_EXTENSIONS: [*const i8; 1] = [b"VK_KHR_swapchain\0".as_ptr().cast()];

const FRAMES_IN_FLIGHT: u32 = 2;
const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

struct Device {
    device: ash::Device,
    gpu: vk::PhysicalDevice,
    swapchain_api: ash::extensions::khr::Swapchain,

    graphics_family: u32,
    present_family: u32,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    command_pool: vk::CommandPool,
}

struct Swapchain {
    format: vk::Format,
    extent: vk::Extent2D,
    surface: vk::SurfaceKHR,
    image_views: Vec<vk::ImageView>,

    current_frame: u32,
    current_image: u32,

    frames_in_flight: FramesInFlight,
}

struct RenderState {
    command_buffers: [vk::CommandBuffer; FRAMES_IN_FLIGHT as usize],
    frame_buffers: Vec<vk::Framebuffer>,
}

struct FramesInFlight {
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
    swapchains: HashMap<vk::SwapchainKHR, (Swapchain, RenderState)>,
    pipelines: HashMap<vk::Format, pipeline::Pipeline>,
}

impl Renderer {
    pub fn new() -> Result<Self, Error> {
        let entry = unsafe { ash::Entry::load() }.map_err(|_| Error::NoVulkanLibrary)?;

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

            unsafe { entry.create_instance(&instance_ci, None) }?
        };

        let surface_api = { ash::extensions::khr::Surface::new(&entry, &instance) };

        #[cfg(target_os = "windows")]
        let os_surface_api = { ash::extensions::khr::Win32Surface::new(&entry, &instance) };

        Ok(Self {
            entry,
            instance,
            surface_api,
            os_surface_api,
            device: None,
            swapchains: HashMap::new(),
            pipelines: HashMap::new(),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn create_swapchain(
        &mut self,
        hwnd: HWND,
        hinstance: HINSTANCE,
    ) -> Result<vk::SwapchainKHR, Error> {
        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe {
            self.os_surface_api
                .create_win32_surface(&surface_ci, None)?
        };

        let extent = unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            vk::Extent2D {
                width: u32::try_from(rect.right).unwrap(),
                height: u32::try_from(rect.bottom).unwrap(),
            }
        };

        let device =
            self.device
                .get_or_insert(init_device(&self.instance, &self.surface_api, surface)?);
        let (handle, swapchain) = create_swapchain(device, surface, extent, &self.surface_api)?;
        let render_state = init_render_state(
            device,
            &mut self.pipelines,
            swapchain.format,
            &swapchain.image_views,
            extent,
        )?;

        self.swapchains.insert(handle, (swapchain, render_state));
        Ok(handle)
    }

    pub fn destroy_swapchain(&mut self, handle: vk::SwapchainKHR) -> Result<(), Error> {
        if let Some((swapchain, state)) = self.swapchains.remove(&handle) {
            let device = self.device.as_ref().unwrap();
            destroy_swapchain(device, &self.surface_api, handle, swapchain)?;
            destroy_render_state(device, state);
        }
        Ok(())
    }

    pub fn begin_frame(&mut self, handle: vk::SwapchainKHR) -> Result<(), Error> {
        let device = self.device.as_ref().unwrap();
        let (swapchain, _) = self.swapchains.get_mut(&handle).unwrap();
        acquire_next_image(device, handle, swapchain)
    }

    pub fn end_frame(&mut self, handle: vk::SwapchainKHR) -> Result<(), Error> {
        let device = self.device.as_ref().unwrap();
        let (swapchain, render_state) = self.swapchains.get_mut(&handle).unwrap();

        let frame_idx = swapchain.current_frame as usize % DESIRED_SWAPCHAIN_LENGTH as usize;
        let fif = &swapchain.frames_in_flight;

        let command_buffer = pipeline::record_draw(
            &device.device,
            self.pipelines.get(&swapchain.format).unwrap(),
            render_state.command_buffers[frame_idx],
            render_state.frame_buffers[frame_idx],
            swapchain.extent,
        )?;

        unsafe {
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            device.device.queue_submit(
                device.graphics_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[command_buffer])
                    .wait_semaphores(&[fif.acquire_semaphores[frame_idx]])
                    .signal_semaphores(&[fif.present_semaphores[frame_idx]])
                    .wait_dst_stage_mask(&wait_stages)
                    .build()],
                fif.fences[frame_idx],
            )?;
        }

        present(device, handle, swapchain)?;

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let vkdevice = &device.device;

            unsafe {
                vkdevice.device_wait_idle().unwrap();
            }

            for (handle, (swapchain, render_state)) in std::mem::take(&mut self.swapchains) {
                destroy_swapchain(&device, &self.surface_api, handle, swapchain).unwrap();
                destroy_render_state(&device, render_state);
            }

            for (_, pipeline) in std::mem::take(&mut self.pipelines) {
                unsafe {
                    vkdevice.destroy_pipeline(pipeline.pipeline, None);
                    vkdevice.destroy_render_pass(pipeline.render_pass, None);
                    vkdevice.destroy_pipeline_layout(pipeline.layout, None);
                }
            }

            unsafe {
                vkdevice.destroy_command_pool(device.command_pool, None);
                vkdevice.destroy_device(None);
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

fn init_device(
    instance: &ash::Instance,
    surface_api: &ash::extensions::khr::Surface,
    surface: vk::SurfaceKHR,
) -> Result<Device, Error> {
    let selected_device = {
        let mut selected_device = None;

        for gpu in unsafe { instance.enumerate_physical_devices().unwrap() } {
            let mut found_present_family = false;
            let mut found_graphics_family = false;
            let mut present_family = 0;
            let mut graphics_family = 0;

            let queue_families =
                unsafe { instance.get_physical_device_queue_family_properties(gpu) };

            for (index, queue_family) in queue_families.iter().enumerate().rev() {
                let index = index.try_into().unwrap();

                if unsafe { surface_api.get_physical_device_surface_support(gpu, index, surface)? }
                {
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
                &unsafe { instance.enumerate_device_extension_properties(gpu)? },
                |e| &e.extension_name,
                &DEVICE_EXTENSIONS,
            )[0];

            if !supports_swapchain {
                continue;
            }

            selected_device = Some((gpu, graphics_family, present_family));
            break;
        }

        selected_device
    };

    let (gpu, graphics_family, present_family) =
        if let Some((physical_device, present_family, graphics_family)) = selected_device {
            (physical_device, graphics_family, present_family)
        } else {
            return Err(Error::NoSuitableGpu);
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

        unsafe { instance.create_device(gpu, &device_ci, None)? }
    };

    let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
    let present_queue = unsafe { device.get_device_queue(present_family, 0) };

    let swapchain_api = { ash::extensions::khr::Swapchain::new(instance, &device) };

    let command_pool = {
        let pool_ci = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(graphics_family);

        unsafe { device.create_command_pool(&pool_ci, None)? }
    };

    Ok(Device {
        device,
        gpu,
        swapchain_api,
        graphics_family,
        present_family,
        graphics_queue,
        present_queue,
        command_pool,
    })
}

fn create_swapchain(
    device: &Device,
    surface: vk::SurfaceKHR,
    extent: vk::Extent2D,
    surface_api: &ash::extensions::khr::Surface,
) -> Result<(vk::SwapchainKHR, Swapchain), Error> {
    let vkdevice = &device.device;

    let format = {
        let formats =
            unsafe { surface_api.get_physical_device_surface_formats(device.gpu, surface)? };
        formats
            .iter()
            .find_map(|f| (f.format == vk::Format::B8G8R8A8_SRGB).then_some(*f))
            .unwrap_or(formats[0])
    };

    let capabilities =
        unsafe { surface_api.get_physical_device_surface_capabilities(device.gpu, surface)? };

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

    let min_images = if capabilities.max_image_count == 0
        || capabilities.min_image_count <= DESIRED_SWAPCHAIN_LENGTH
    {
        DESIRED_SWAPCHAIN_LENGTH
    } else {
        capabilities.min_image_count
    };

    let concurrent_family_indices = &[device.graphics_family, device.present_family];
    let swapchain_ci = vk::SwapchainCreateInfoKHR::builder()
        .surface(surface)
        .min_image_count(min_images)
        .image_format(format.format)
        .image_color_space(format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .queue_family_indices(concurrent_family_indices)
        .pre_transform(capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(vk::PresentModeKHR::FIFO)
        .clipped(true);

    let swapchain_ci = if device.graphics_family == device.present_family {
        swapchain_ci.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
    } else {
        swapchain_ci.image_sharing_mode(vk::SharingMode::CONCURRENT)
    };

    let handle = unsafe { device.swapchain_api.create_swapchain(&swapchain_ci, None)? };

    let image_views = {
        let images = unsafe { device.swapchain_api.get_swapchain_images(handle)? };
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

            views.push(unsafe { vkdevice.create_image_view(&view_ci, None) }?);
        }
        views
    };

    let frames_in_flight = {
        let semaphore_ci = vk::SemaphoreCreateInfo::builder();
        let fence_ci = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

        let mut acquire_semaphores = [vk::Semaphore::null(); FRAMES_IN_FLIGHT as usize];
        let mut present_semaphores = [vk::Semaphore::null(); FRAMES_IN_FLIGHT as usize];
        let mut fences = [vk::Fence::null(); FRAMES_IN_FLIGHT as usize];

        for i in 0..FRAMES_IN_FLIGHT as usize {
            unsafe {
                acquire_semaphores[i] = vkdevice.create_semaphore(&semaphore_ci, None)?;
                present_semaphores[i] = vkdevice.create_semaphore(&semaphore_ci, None)?;
                fences[i] = vkdevice.create_fence(&fence_ci, None)?;
            }
        }

        FramesInFlight {
            acquire_semaphores,
            present_semaphores,
            fences,
        }
    };

    Ok((
        handle,
        Swapchain {
            format: format.format,
            extent,
            surface,
            image_views,
            frames_in_flight,
            current_frame: 0,
            current_image: 0,
        },
    ))
}

fn destroy_swapchain(
    device: &Device,
    surface_api: &ash::extensions::khr::Surface,
    handle: vk::SwapchainKHR,
    mut data: Swapchain,
) -> Result<(), Error> {
    let vkdevice = &device.device;
    let fif = data.frames_in_flight;

    unsafe {
        vkdevice.wait_for_fences(&fif.fences, true, u64::MAX)?;

        for view in data.image_views.drain(..) {
            vkdevice.destroy_image_view(view, None);
        }

        for i in 0..FRAMES_IN_FLIGHT as usize {
            vkdevice.destroy_semaphore(fif.acquire_semaphores[i], None);
            vkdevice.destroy_semaphore(fif.present_semaphores[i], None);
            vkdevice.destroy_fence(fif.fences[i], None);
        }

        device.swapchain_api.destroy_swapchain(handle, None);
        surface_api.destroy_surface(data.surface, None);
    }

    Ok(())
}

fn acquire_next_image(
    device: &Device,
    handle: vk::SwapchainKHR,
    swapchain: &mut Swapchain,
) -> Result<(), Error> {
    let frame_idx = swapchain.current_frame as usize % DESIRED_SWAPCHAIN_LENGTH as usize;

    unsafe {
        let fence = swapchain.frames_in_flight.fences[frame_idx];
        device.device.wait_for_fences(&[fence], true, u64::MAX)?;
        device.device.reset_fences(&[fence])?;
    }

    let (index, _needs_resize) = unsafe {
        device.swapchain_api.acquire_next_image(
            handle,
            u64::MAX,
            swapchain.frames_in_flight.acquire_semaphores[frame_idx],
            vk::Fence::null(),
        )?
    };

    swapchain.current_image = index;
    Ok(())
}

fn present(
    device: &Device,
    handle: vk::SwapchainKHR,
    swapchain: &mut Swapchain,
) -> Result<(), Error> {
    let frame_idx = swapchain.current_frame as usize % DESIRED_SWAPCHAIN_LENGTH as usize;

    unsafe {
        device.swapchain_api.queue_present(
            device.present_queue,
            &vk::PresentInfoKHR::builder()
                .wait_semaphores(&[swapchain.frames_in_flight.present_semaphores[frame_idx]])
                .swapchains(&[handle])
                .image_indices(&[swapchain.current_image]),
        )?;
    }

    swapchain.current_frame += 1;

    Ok(())
}

fn init_render_state(
    device: &Device,
    pipelines: &mut HashMap<vk::Format, pipeline::Pipeline>,
    swapchain_format: vk::Format,
    image_views: &[vk::ImageView],
    image_extent: vk::Extent2D,
) -> Result<RenderState, Error> {
    let vkdevice = &device.device;
    let command_buffers = {
        let mut array = [vk::CommandBuffer::null(); FRAMES_IN_FLIGHT as usize];

        let command_buffer_ai = vk::CommandBufferAllocateInfo::builder()
            .command_pool(device.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT)
            .build();

        let vk_result = unsafe {
            (vkdevice.fp_v1_0().allocate_command_buffers)(
                vkdevice.handle(),
                &command_buffer_ai,
                array.as_mut_ptr(),
            )
        };

        if vk_result != vk::Result::SUCCESS {
            return Err(Error::Vulkan(vk_result));
        }

        array
    };

    let pipeline = if let Some(pipeline) = pipelines.get(&swapchain_format) {
        pipeline
    } else {
        pipelines.insert(
            swapchain_format,
            pipeline::create(vkdevice, swapchain_format)?,
        );
        pipelines.get(&swapchain_format).unwrap()
    };

    let frame_buffers = {
        let mut frame_buffers = Vec::with_capacity(image_views.len());

        for view in image_views {
            let framebuffer_ci = vk::FramebufferCreateInfo::builder()
                .render_pass(pipeline.render_pass)
                .attachments(std::slice::from_ref(&*view))
                .width(image_extent.width)
                .height(image_extent.height)
                .layers(1);

            frame_buffers.push(unsafe { vkdevice.create_framebuffer(&framebuffer_ci, None) }?);
        }
        frame_buffers
    };

    Ok(RenderState {
        command_buffers,
        frame_buffers,
    })
}

fn destroy_render_state(device: &Device, mut state: RenderState) {
    let vkdevice = &device.device;
    unsafe {
        vkdevice.free_command_buffers(device.command_pool, &state.command_buffers);

        for framebuffer in state.frame_buffers.drain(..) {
            vkdevice.destroy_framebuffer(framebuffer, None);
        }
    }
}
