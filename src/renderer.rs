mod error;
mod pipeline;
mod swapchain;

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

use crate::indexed_store::{Index, IndexedStore};

use self::{
    error::Error,
    swapchain::{Swapchain, FRAMES_IN_FLIGHT},
};

const VALIDATION_LAYER: *const i8 = b"VK_LAYER_KHRONOS_validation\0".as_ptr().cast();

const INSTANCE_EXTENSIONS: [*const i8; 2] = [
    b"VK_KHR_surface\0".as_ptr().cast(),
    #[cfg(target_os = "windows")]
    b"VK_KHR_win32_surface\0".as_ptr().cast(),
];

const DEVICE_EXTENSIONS: [*const i8; 1] = [b"VK_KHR_swapchain\0".as_ptr().cast()];

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

struct RenderState {
    command_buffers: [vk::CommandBuffer; FRAMES_IN_FLIGHT as usize],
    frame_buffers: Vec<vk::Framebuffer>,
}

#[derive(Clone, Copy, Debug)]
pub struct SwapchainHandle(Index);

pub struct Renderer {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,

    surface_api: ash::extensions::khr::Surface,

    #[cfg(target_os = "windows")]
    os_surface_api: ash::extensions::khr::Win32Surface,

    device: Option<Device>,
    swapchains: IndexedStore<(Swapchain, RenderState)>,
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
            swapchains: IndexedStore::new(),
            pipelines: HashMap::new(),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn create_swapchain(
        &mut self,
        hwnd: HWND,
        hinstance: HINSTANCE,
    ) -> Result<SwapchainHandle, Error> {
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
        let swapchain = Swapchain::new(device, surface, extent, &self.surface_api)?;
        let render_state = init_render_state(
            device,
            &mut self.pipelines,
            swapchain.format,
            &swapchain.image_views,
            extent,
        )?;

        let handle = self
            .swapchains
            .insert((swapchain, render_state))
            .map_err(|_| Error::TooManyObjects)?;
        Ok(SwapchainHandle(handle))
    }

    pub fn destroy_swapchain(&mut self, handle: SwapchainHandle) -> Result<(), Error> {
        if let Some((mut swapchain, state)) = self.swapchains.remove(handle.0) {
            let device = self.device.as_ref().unwrap();
            swapchain.destroy(device, &self.surface_api).unwrap();
            destroy_render_state(device, state);
        }
        Ok(())
    }

    pub fn begin_frame(&mut self, handle: SwapchainHandle) -> Result<(), Error> {
        let device = self.device.as_ref().unwrap();
        let (swapchain, _) = self.swapchains.get_mut(handle.0).unwrap();

        match swapchain.acquire_next_image(device) {
            Ok(_) => Ok(()),
            Err(Error::SwapchainOutOfDate) => {
                let (swapchain, old_render_state) = self.swapchains.get_mut(handle.0).unwrap();

                swapchain.resize(device, vk::Extent2D::default(), &self.surface_api)?;

                let mut new_render_state = init_render_state(
                    device,
                    &mut self.pipelines,
                    swapchain.format,
                    &swapchain.image_views,
                    swapchain.extent,
                )?;

                std::mem::swap(&mut new_render_state, old_render_state);
                destroy_render_state(device, new_render_state);

                swapchain.acquire_next_image(device)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn end_frame(&mut self, handle: SwapchainHandle) -> Result<(), Error> {
        let device = self.device.as_ref().unwrap();
        let (swapchain, render_state) = self.swapchains.get_mut(handle.0).unwrap();

        let (frame_index, frame_objects) = swapchain.frame_objects();

        let command_buffer = pipeline::record_draw(
            &device.device,
            self.pipelines.get(&swapchain.format).unwrap(),
            render_state.command_buffers[frame_index],
            render_state.frame_buffers[swapchain.current_image.unwrap() as usize],
            swapchain.extent,
        )?;

        unsafe {
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            device.device.queue_submit(
                device.graphics_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[command_buffer])
                    .wait_semaphores(&[frame_objects.acquire_semaphore])
                    .signal_semaphores(&[frame_objects.present_semaphore])
                    .wait_dst_stage_mask(&wait_stages)
                    .build()],
                frame_objects.fence,
            )?;
        }

        match swapchain.present(device) {
            Ok(_) => Ok(()),
            Err(Error::SwapchainOutOfDate) => {
                let (swapchain, old_render_state) = self.swapchains.get_mut(handle.0).unwrap();

                swapchain.resize(device, vk::Extent2D::default(), &self.surface_api)?;

                let mut new_render_state = init_render_state(
                    device,
                    &mut self.pipelines,
                    swapchain.format,
                    &swapchain.image_views,
                    swapchain.extent,
                )?;

                std::mem::swap(&mut new_render_state, old_render_state);
                destroy_render_state(device, new_render_state);

                swapchain.acquire_next_image(device)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            let vkdevice = &device.device;

            unsafe {
                vkdevice.device_wait_idle().unwrap();
            }

            assert!(
                self.swapchains.is_empty(),
                "all swapchains must be destroyed before the renderer is dropped"
            );

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
