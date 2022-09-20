mod canvas;
mod error;
mod memory;
mod pipeline;
mod swapchain;
mod vertex;

use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    os::raw::c_char,
};

use ash::vk;
use once_cell::sync::Lazy;
#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{HWND, RECT},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::GetClientRect,
};

use crate::{
    gfx::color::Color,
    indexed_object_pool::{newtype_index, IndexedObjectPool},
};

use self::{
    canvas::Canvas,
    memory::{Allocation, Memory},
    swapchain::{Swapchain, FRAMES_IN_FLIGHT},
};

pub use error::Error;
pub use vertex::Vertex;

const VALIDATION_LAYER: *const i8 = b"VK_LAYER_KHRONOS_validation\0".as_ptr().cast();

const INSTANCE_EXTENSIONS: [*const i8; 2] = [
    b"VK_KHR_surface\0".as_ptr().cast(),
    #[cfg(target_os = "windows")]
    b"VK_KHR_win32_surface\0".as_ptr().cast(),
];

const DEVICE_EXTENSIONS: [*const i8; 1] = [b"VK_KHR_swapchain\0".as_ptr().cast()];

pub(self) struct Vulkan {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,

    surface_api: ash::extensions::khr::Surface,

    #[cfg(target_os = "windows")]
    os_surface_api: ash::extensions::khr::Win32Surface,
}

static VULKAN: Lazy<Vulkan> = Lazy::new(|| {
    let entry = unsafe { ash::Entry::load() }
        .map_err(|_| Error::NoVulkanLibrary)
        .unwrap();

    let instance = {
        let app_info = vk::ApplicationInfo::builder().api_version(vk::make_api_version(0, 1, 1, 0));

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

        unsafe { entry.create_instance(&instance_ci, None) }.unwrap()
    };

    let surface_api = { ash::extensions::khr::Surface::new(&entry, &instance) };

    #[cfg(target_os = "windows")]
    let os_surface_api = { ash::extensions::khr::Win32Surface::new(&entry, &instance) };

    Vulkan {
        entry,
        instance,
        surface_api,
        os_surface_api,
    }
});

struct Device {
    device: ash::Device,
    gpu: vk::PhysicalDevice,

    swapchain_api: ash::extensions::khr::Swapchain,

    graphics_family: u32,
    present_family: u32,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    memory: Memory,
    command_pool: vk::CommandPool,
}

#[derive(Default)]
struct DeferredDestroy {
    buffers: Vec<vk::Buffer>,
    allocations: Vec<Allocation>,
}

impl DeferredDestroy {
    fn cleanup(&mut self, device: &mut Device) {
        for buffer in self.buffers.drain(..) {
            unsafe { device.device.destroy_buffer(buffer, None) };
        }

        for allocation in self.allocations.drain(..) {
            device.memory.deallocate(allocation);
        }
    }
}

struct RenderState {
    deferred_destroy: [DeferredDestroy; FRAMES_IN_FLIGHT as usize],
    frame_buffers: Vec<vk::Framebuffer>,
}

impl RenderState {
    fn new(
        device: &mut Device,
        pipelines: &mut HashMap<vk::Format, pipeline::Pipeline>,
        swapchain: &Swapchain,
    ) -> Result<Self, Error> {
        let vkdevice = &device.device;

        let pipeline = if let Some(pipeline) = pipelines.get(&swapchain.format) {
            pipeline
        } else {
            pipelines.insert(
                swapchain.format,
                pipeline::create(vkdevice, swapchain.format)?,
            );
            pipelines.get(&swapchain.format).unwrap()
        };

        let frame_buffers = {
            let mut frame_buffers = Vec::with_capacity(swapchain.image_views.len());

            for view in &swapchain.image_views {
                let framebuffer_ci = vk::FramebufferCreateInfo::builder()
                    .render_pass(pipeline.render_pass)
                    .attachments(std::slice::from_ref(view))
                    .width(swapchain.extent.width)
                    .height(swapchain.extent.height)
                    .layers(1);

                frame_buffers.push(unsafe { vkdevice.create_framebuffer(&framebuffer_ci, None) }?);
            }
            frame_buffers
        };

        Ok(Self {
            frame_buffers,
            deferred_destroy: Default::default(),
        })
    }

    fn destroy_with(&mut self, device: &mut Device) {
        unsafe {
            for framebuffer in self.frame_buffers.drain(..) {
                device.device.destroy_framebuffer(framebuffer, None);
            }

            for deferred in &mut self.deferred_destroy {
                deferred.cleanup(device);
            }
        }
    }

    fn update(
        &mut self,
        device: &Device,
        pipelines: &mut HashMap<vk::Format, pipeline::Pipeline>,
        swapchain: &Swapchain,
    ) -> Result<(), Error> {
        let pipeline = if let Some(pipeline) = pipelines.get(&swapchain.format) {
            pipeline
        } else {
            pipelines.insert(
                swapchain.format,
                pipeline::create(&device.device, swapchain.format)?,
            );
            pipelines.get(&swapchain.format).unwrap()
        };

        for framebuffer in self.frame_buffers.drain(..) {
            unsafe { device.device.destroy_framebuffer(framebuffer, None) }
        }

        for view in &swapchain.image_views {
            let framebuffer_ci = vk::FramebufferCreateInfo::builder()
                .render_pass(pipeline.render_pass)
                .attachments(std::slice::from_ref(view))
                .width(swapchain.extent.width)
                .height(swapchain.extent.height)
                .layers(1);

            self.frame_buffers
                .push(unsafe { device.device.create_framebuffer(&framebuffer_ci, None) }?);
        }

        Ok(())
    }
}

newtype_index!(SwapchainHandle, (Swapchain, RenderState));

pub struct Renderer {
    device: Option<Device>,
    swapchains: IndexedObjectPool<(Swapchain, RenderState)>,
    pipelines: HashMap<vk::Format, pipeline::Pipeline>,
}

impl Renderer {
    pub fn new() -> Result<Self, Error> {
        Lazy::force(&VULKAN);

        Ok(Self {
            device: None,
            swapchains: IndexedObjectPool::new(),
            pipelines: HashMap::new(),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn create_swapchain(&mut self, hwnd: HWND) -> Result<SwapchainHandle, Error> {
        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe {
            VULKAN
                .os_surface_api
                .create_win32_surface(&surface_ci, None)?
        };

        let extent = unsafe {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            vk::Extent2D {
                width: u32::try_from(rect.right).unwrap(),
                height: u32::try_from(rect.bottom).unwrap(),
            }
        };

        self.create_swapchain_impl(surface, extent)
    }

    fn create_swapchain_impl(
        &mut self,
        surface: vk::SurfaceKHR,
        extent: vk::Extent2D,
    ) -> Result<SwapchainHandle, Error> {
        let device = if let Some(device) = &mut self.device {
            device
        } else {
            self.device = Some(init_device(surface)?);
            self.device.as_mut().unwrap()
        };

        let swapchain = Swapchain::new(device, surface, extent)?;
        let render_state = RenderState::new(device, &mut self.pipelines, &swapchain)?;

        let handle = self
            .swapchains
            .insert((swapchain, render_state))
            .map_err(|_| Error::TooManyObjects)?;
        Ok(handle.into())
    }

    pub fn destroy_swapchain(&mut self, handle: SwapchainHandle) -> Result<(), Error> {
        if let Some((mut swapchain, mut state)) = self.swapchains.remove(handle) {
            let device = self.device.as_mut().unwrap();
            swapchain.destroy_with(device)?;
            state.destroy_with(device);
        }
        Ok(())
    }

    pub fn begin_frame(&mut self, handle: SwapchainHandle) -> Result<Canvas, Error> {
        let device = self.device.as_mut().unwrap();
        let (swapchain, render_state) = self.swapchains.get_mut(handle).unwrap();

        match swapchain.acquire_next_image(device) {
            Ok(_) => Ok(()),
            Err(Error::SwapchainOutOfDate) => {
                swapchain.resize(device, vk::Extent2D::default())?;
                render_state.update(device, &mut self.pipelines, swapchain)?;

                swapchain.acquire_next_image(device)?;
                Ok(())
            }
            Err(e) => Err(e),
        }?;

        // TODO(straivers): calling frame_id() is kinda ugly
        render_state.deferred_destroy[swapchain.frame_id()].cleanup(device);

        Canvas::new(
            device,
            swapchain.extent,
            handle,
            render_state.frame_buffers[swapchain.current_image.unwrap() as usize],
        )
    }

    pub fn submit(&mut self, canvas: Canvas) -> Result<(), Error> {
        let device = self.device.as_mut().unwrap();
        let (swapchain, render_state) = self.swapchains.get_mut(canvas.swapchain).unwrap();
        let (frame_id, frame_objects) = swapchain.frame_objects();

        let pipeline = if let Some(pipeline) = self.pipelines.get(&swapchain.format) {
            pipeline
        } else {
            self.pipelines.insert(
                swapchain.format,
                pipeline::create(&device.device, swapchain.format)?,
            );
            self.pipelines.get(&swapchain.format).unwrap()
        };

        let command_buffer = {
            let command_buffer_ci = vk::CommandBufferAllocateInfo {
                command_pool: device.command_pool,
                level: vk::CommandBufferLevel::PRIMARY,
                command_buffer_count: 1,
                ..Default::default()
            };

            let mut handle = [vk::CommandBuffer::null()];
            let vk_result = unsafe {
                (device.device.fp_v1_0().allocate_command_buffers)(
                    device.device.handle(),
                    &command_buffer_ci,
                    handle.as_mut_ptr(),
                )
            };

            if vk_result != vk::Result::SUCCESS {
                return Err(Error::Vulkan(vk_result));
            }

            handle[0]
        };

        pipeline::record_draw(
            &device.device,
            pipeline,
            command_buffer,
            canvas.frame_buffer,
            canvas.extent,
            Color::BLACK,
            canvas.vertex_buffer,
            canvas.index_buffer,
            canvas.num_indices() as u16,
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

        canvas.finish(device, &mut render_state.deferred_destroy[frame_id])?;

        match swapchain.present(device) {
            Ok(_) => Ok(()),
            Err(Error::SwapchainOutOfDate) => {
                swapchain.resize(device, vk::Extent2D::default())?;
                render_state.update(device, &mut self.pipelines, swapchain)?;
                swapchain.acquire_next_image(device)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(mut device) = self.device.take() {
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
                device.memory.destroy(vkdevice);
                vkdevice.destroy_device(None);
            }
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

fn init_device(surface: vk::SurfaceKHR) -> Result<Device, Error> {
    let selected_device = {
        let mut selected_device = None;

        for gpu in unsafe { VULKAN.instance.enumerate_physical_devices().unwrap() } {
            let mut found_present_family = false;
            let mut found_graphics_family = false;
            let mut present_family = 0;
            let mut graphics_family = 0;

            let queue_families = unsafe {
                VULKAN
                    .instance
                    .get_physical_device_queue_family_properties(gpu)
            };

            for (index, queue_family) in queue_families.iter().enumerate().rev() {
                let index = index.try_into().unwrap();

                if unsafe {
                    VULKAN
                        .surface_api
                        .get_physical_device_surface_support(gpu, index, surface)?
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
                &unsafe { VULKAN.instance.enumerate_device_extension_properties(gpu)? },
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

        unsafe { VULKAN.instance.create_device(gpu, &device_ci, None)? }
    };

    let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
    let present_queue = unsafe { device.get_device_queue(present_family, 0) };

    let swapchain_api = { ash::extensions::khr::Swapchain::new(&VULKAN.instance, &device) };

    let memory_properties = unsafe { VULKAN.instance.get_physical_device_memory_properties(gpu) };

    let command_pool = {
        let pool_ci = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(graphics_family);

        unsafe { device.create_command_pool(&pool_ci, None)? }
    };

    let memory = Memory::new(memory_properties);

    Ok(Device {
        device,
        gpu,
        swapchain_api,
        graphics_family,
        present_family,
        graphics_queue,
        present_queue,
        memory,
        command_pool,
    })
}
