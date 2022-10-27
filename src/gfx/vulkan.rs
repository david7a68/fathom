use std::{cell::RefCell, ffi::c_char};

use ash::vk;
use smallvec::SmallVec;

use crate::handle_pool::{Handle, HandlePool};

use super::{
    color::Color,
    geometry::{Extent, Point, Rect},
    pixel_buffer::{ColorSpace, Layout, PixelBuffer},
    DrawCommandList, Error, GfxDevice, Vertex, MAX_SWAPCHAINS,
};

const fn as_cchar_slice(slice: &[u8]) -> &[c_char] {
    unsafe { std::mem::transmute(slice) }
}

const SHADER_MAIN: *const i8 = b"main\0".as_ptr().cast();
const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

const VALIDATION_LAYER: &[c_char] = as_cchar_slice(b"VK_LAYER_KHRONOS_validation\0");

const REQUIRED_INSTANCE_LAYERS: &[&[c_char]] = &[];

const REQUIRED_INSTANCE_EXTENSIONS: &[&[c_char]] = &[
    as_cchar_slice(b"VK_KHR_surface\0"),
    #[cfg(target_os = "windows")]
    as_cchar_slice(b"VK_KHR_win32_surface\0"),
];

const OPTIONAL_INSTANCE_EXTENSIONS: &[&[c_char]] =
    &[as_cchar_slice(b"VK_EXT_swapchjain_colorspace\0")];

const REQUIRED_DEVICE_EXTENSIONS: &[&[c_char]] = &[as_cchar_slice(b"VK_KHR_swapchain\0")];

const OPTIONAL_DEVICE_EXTENSIONS: &[&[c_char]] = &[];

const FRAMES_IN_FLIGHT: usize = 2;
const PREFERRED_SWAPCHAIN_LENGTH: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    scale: [f32; 2],
    translate: [f32; 2],
}

union PushConstantBytes {
    constants: PushConstants,
    bytes: [u8; std::mem::size_of::<PushConstants>()],
}

#[derive(Debug)]
struct PhysicalDevice {
    handle: vk::PhysicalDevice,
    properties: vk::PhysicalDeviceProperties,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    graphics_queue_family: u32,
    transfer_queue_family: u32,
    present_queue_family: u32,
}

pub struct Vulkan {
    #[allow(unused)]
    entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,

    physical_device: PhysicalDevice,

    pipeline_cache: vk::PipelineCache,
    graphics_queue: vk::Queue,
    transfer_queue: vk::Queue,
    present_queue: vk::Queue,

    surface_khr: ash::extensions::khr::Surface,
    swapchain_khr: ash::extensions::khr::Swapchain,

    #[cfg(target_os = "windows")]
    win32_surface_khr: ash::extensions::khr::Win32Surface,

    windows: RefCell<HandlePool<WindowData, super::Swapchain, MAX_SWAPCHAINS>>,
    render_targets: RefCell<HandlePool<RenderTarget, super::RenderTarget, 128>>,
}

impl Vulkan {
    pub fn new(with_debug: bool) -> Result<Self, Error> {
        let entry = unsafe { ash::Entry::load() }
            .map_err(|_| Error::BackendNotFound)
            .unwrap();

        let instance = {
            let instance_layers = {
                let mut optional = SmallVec::<[&[c_char]; 1]>::new();
                if with_debug {
                    optional.push(VALIDATION_LAYER);
                }

                has_names(
                    &entry.enumerate_instance_layer_properties()?,
                    |layer| &layer.layer_name,
                    REQUIRED_INSTANCE_LAYERS,
                    &optional,
                )
                .ok_or(Error::VulkanInternal {
                    error_code: vk::Result::ERROR_INITIALIZATION_FAILED,
                })?
            };

            let instance_extensions = has_names(
                &entry.enumerate_instance_extension_properties(None)?,
                |extension| &extension.extension_name,
                REQUIRED_INSTANCE_EXTENSIONS,
                OPTIONAL_INSTANCE_EXTENSIONS,
            )
            .ok_or(Error::VulkanInternal {
                error_code: vk::Result::ERROR_INITIALIZATION_FAILED,
            })?;

            let app_info = vk::ApplicationInfo {
                api_version: vk::make_api_version(0, 1, 1, 0),
                ..Default::default()
            };

            let create_info = vk::InstanceCreateInfo {
                p_application_info: &app_info,
                enabled_layer_count: instance_layers.len() as u32,
                pp_enabled_layer_names: instance_layers.as_ptr(),
                enabled_extension_count: instance_extensions.len() as u32,
                pp_enabled_extension_names: instance_extensions.as_ptr(),
                ..Default::default()
            };

            unsafe { entry.create_instance(&create_info, None) }?
        };

        let surface_khr = ash::extensions::khr::Surface::new(&entry, &instance);

        #[cfg(target_os = "windows")]
        let win32_surface_khr = ash::extensions::khr::Win32Surface::new(&entry, &instance);

        let (gpu, device_extensions) = select_gpu(&instance, |gpu, queue| unsafe {
            #[cfg(target_os = "windows")]
            win32_surface_khr.get_physical_device_win32_presentation_support(gpu, queue)
        })?;

        let device = {
            let queue_priority = 1.0;
            let mut queues = SmallVec::<[vk::DeviceQueueCreateInfo; 3]>::new();

            queues.push(
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(gpu.graphics_queue_family)
                    .queue_priorities(&[queue_priority])
                    .build(),
            );

            if gpu.graphics_queue_family != gpu.transfer_queue_family {
                queues.push(
                    vk::DeviceQueueCreateInfo::builder()
                        .queue_family_index(gpu.transfer_queue_family)
                        .queue_priorities(&[queue_priority])
                        .build(),
                );
            }

            if gpu.graphics_queue_family != gpu.present_queue_family {
                queues.push(
                    vk::DeviceQueueCreateInfo::builder()
                        .queue_family_index(gpu.present_queue_family)
                        .queue_priorities(&[queue_priority])
                        .build(),
                );
            }

            let create_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queues)
                .enabled_extension_names(&device_extensions);

            unsafe { instance.create_device(gpu.handle, &create_info, None) }?
        };

        let pipeline_cache = {
            let create_info = vk::PipelineCacheCreateInfo::default();
            unsafe { device.create_pipeline_cache(&create_info, None) }?
        };

        let graphics_queue = unsafe { device.get_device_queue(gpu.graphics_queue_family, 0) };
        let transfer_queue = unsafe { device.get_device_queue(gpu.transfer_queue_family, 0) };
        let present_queue = unsafe { device.get_device_queue(gpu.present_queue_family, 0) };

        let swapchain_khr = ash::extensions::khr::Swapchain::new(&instance, &device);

        Ok(Self {
            entry,
            instance,
            device,
            physical_device: gpu,
            pipeline_cache,
            graphics_queue,
            transfer_queue,
            present_queue,
            surface_khr,
            swapchain_khr,
            win32_surface_khr,
            windows: RefCell::new(HandlePool::preallocate()),
            render_targets: RefCell::new(HandlePool::preallocate()),
        })
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        unsafe {
            self.device
                .destroy_pipeline_cache(self.pipeline_cache, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

impl GfxDevice for Vulkan {
    fn create_swapchain(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Result<Handle<super::Swapchain>, Error> {
        let window = WindowData::new(self, hwnd)?;
        Ok(self.windows.borrow_mut().insert(window)?)
    }

    fn resize_swapchain(
        &self,
        handle: Handle<super::Swapchain>,
        extent: Extent,
    ) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle).ok_or(Error::InvalidHandle)?;
        unsafe { self.device.device_wait_idle() }?;
        window.resize(self, extent.into())?;
        Ok(())
    }

    fn destroy_swapchain(&self, handle: Handle<super::Swapchain>) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.remove(handle).ok_or(Error::InvalidHandle)?;
        unsafe { self.device.device_wait_idle() }?;
        window.destroy(self);
        Ok(())
    }

    fn get_next_swapchain_image(
        &self,
        handle: Handle<super::Swapchain>,
    ) -> Result<Handle<super::RenderTarget>, Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle).ok_or(Error::InvalidHandle)?;
        window.get_next_image(self)?;

        let mut render_targets = self.render_targets.borrow_mut();
        let handle = render_targets.insert(RenderTarget::Swapchain(handle, window.frame_id))?;
        Ok(handle)
    }

    fn present_swapchain_images(&self, handles: &[Handle<super::Swapchain>]) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        for handle in handles {
            let window = windows.get_mut(*handle).ok_or(Error::InvalidHandle)?;
            window.present(self)?;
        }
        Ok(())
    }

    fn create_image(
        &self,
        layout: Layout,
        color_space: ColorSpace,
    ) -> Result<Handle<super::Image>, Error> {
        todo!()
    }

    fn upload_image(&self, pixels: &PixelBuffer) -> Result<Handle<super::Image>, Error> {
        todo!()
    }

    fn delete_image(&self, handle: Handle<super::Image>) -> Result<(), Error> {
        todo!()
    }

    fn get_image_pixels(&self, handle: Handle<super::Image>) -> Result<PixelBuffer, Error> {
        todo!()
    }

    fn destroy_render_target(&self, handle: Handle<super::RenderTarget>) -> Result<(), Error> {
        let mut targets = self.render_targets.borrow_mut();
        let render_target = targets.remove(handle).ok_or(Error::InvalidHandle)?;

        match render_target {
            RenderTarget::Swapchain(..) => {
                // We don't need to do anything here. It doesn't matter if the
                // swapchain still exists or not, since we're not going to do
                // anything with it anymore.
            }
        }

        Ok(())
    }

    fn draw(
        &self,
        handle: Handle<super::RenderTarget>,
        commands: &DrawCommandList,
    ) -> Result<(), Error> {
        let mut targets = self.render_targets.borrow_mut();
        let render_target = targets.get(handle).ok_or(Error::InvalidHandle)?;

        match render_target {
            RenderTarget::Swapchain(window_handle, frame_id) => {
                let mut windows = self.windows.borrow_mut();

                // Check if the window still exists.
                if let Some(window) = windows.get_mut(*window_handle) {
                    // Check if the render target is pointing to the current image
                    if *frame_id == window.frame_id {
                        // The only way to get a swapchain image is to get a
                        // handle to it, and we've checked that the handle was
                        // acquired for the current frame.
                        let image_index = window.current_image.expect("internal logic error");
                        let frame = &mut window.frames[image_index as usize];
                        let sync = &window.frame_sync[window.frame_id as usize % FRAMES_IN_FLIGHT];

                        unsafe {
                            self.device
                                .wait_for_fences(&[frame.fence], true, u64::MAX)?;
                            self.device.reset_fences(&[frame.fence])?;

                            self.device.reset_command_pool(
                                frame.command_pool,
                                vk::CommandPoolResetFlags::empty(),
                            )?;
                        }

                        write_command_buffer(
                            self,
                            &window.shader,
                            commands,
                            frame.framebuffer,
                            window.swapchain.extent,
                            &mut frame.geometry,
                            frame.command_buffer,
                        )?;

                        unsafe {
                            self.device.queue_submit(
                                self.graphics_queue,
                                &[vk::SubmitInfo::builder()
                                    .command_buffers(&[frame.command_buffer])
                                    .wait_semaphores(&[sync.acquire_semaphore])
                                    .signal_semaphores(&[sync.present_semaphore])
                                    .wait_dst_stage_mask(&[
                                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                    ])
                                    .build()],
                                frame.fence,
                            )
                        }?;

                        return Ok(());
                    }
                }

                targets.remove(handle).unwrap();
                // May be more accurate to have a RenderTargetOutOfDate or
                // ResourceOutOfDate error?
                Err(Error::InvalidHandle)
            }
        }
    }
}

/// Literally, a thing that can be rendered to.
enum RenderTarget {
    Swapchain(Handle<super::Swapchain>, u64),
}

/// Utility struct that contains all the ancillary information needed to render
/// a frame. Each window has `FRAME_IN_FLIGHT` `Frame`s that are used
/// alternately to allow a previously submitted frame to complete on the GPU.
struct Frame {
    image: vk::Image,
    image_view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,

    geometry: GeometryBuffer,

    /// The fence is used to determine when the GPU is done rendering this
    /// frame. Once rendering is done, the command pool can be reset, and the
    /// buffer reused.
    ///
    /// NOTE: It is not sufficient to check that all the fences are signalled
    /// before resizing a window! Check either that the graphics queue or the
    /// device is idle.
    fence: vk::Fence,
}

impl Frame {
    fn destroy(self, api: &Vulkan) {
        unsafe {
            api.device.destroy_image_view(self.image_view, None);
            api.device.destroy_framebuffer(self.framebuffer, None);
            // no need to free command buffers if we're destroying the pool
            api.device.destroy_command_pool(self.command_pool, None);
            api.device.destroy_fence(self.fence, None);
        }
        self.geometry.destroy(api);
    }
}

fn regenerate_frames(
    api: &Vulkan,
    swapchain: &Swapchain,
    shader: &Shader,
    frames: &mut Vec<Frame>,
) -> Result<(), Error> {
    let images = unsafe { api.swapchain_khr.get_swapchain_images(swapchain.handle) }?;

    // if there are more frames than images
    if frames.len() > images.len() {
        for extra in frames.drain(images.len()..) {
            extra.destroy(api);
        }
    }

    // all the frames in the middle
    for (frame, image) in frames.iter_mut().zip(images.iter()) {
        unsafe {
            api.device.destroy_image_view(frame.image_view, None);
            api.device.destroy_framebuffer(frame.framebuffer, None);
        }

        frame.image = *image;

        frame.image_view = {
            let create_info = vk::ImageViewCreateInfo {
                image: *image,
                view_type: vk::ImageViewType::TYPE_2D,
                format: swapchain.format,
                components: vk::ComponentMapping::default(),
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };

            unsafe { api.device.create_image_view(&create_info, None) }?
        };

        frame.framebuffer = {
            let create_info = vk::FramebufferCreateInfo {
                render_pass: shader.render_pass,
                attachment_count: 1,
                p_attachments: &frame.image_view,
                width: swapchain.extent.width,
                height: swapchain.extent.height,
                layers: 1,
                ..Default::default()
            };

            unsafe { api.device.create_framebuffer(&create_info, None) }?
        };
    }

    // if there are more images than frames
    for image in &images[frames.len()..] {
        let image = *image;

        let image_view = {
            let create_info = vk::ImageViewCreateInfo {
                image,
                view_type: vk::ImageViewType::TYPE_2D,
                format: swapchain.format,
                components: vk::ComponentMapping::default(),
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };

            unsafe { api.device.create_image_view(&create_info, None) }?
        };

        let framebuffer = {
            let create_info = vk::FramebufferCreateInfo {
                render_pass: shader.render_pass,
                attachment_count: 1,
                p_attachments: &image_view,
                width: swapchain.extent.width,
                height: swapchain.extent.height,
                layers: 1,
                ..Default::default()
            };

            unsafe { api.device.create_framebuffer(&create_info, None) }?
        };

        let command_pool = {
            let create_info = vk::CommandPoolCreateInfo::builder()
                .queue_family_index(api.physical_device.graphics_queue_family);
            unsafe { api.device.create_command_pool(&create_info, None) }?
        };

        let command_buffer = {
            let create_info = vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let mut command_buffer = vk::CommandBuffer::null();
            unsafe {
                (api.device.fp_v1_0().allocate_command_buffers)(
                    api.device.handle(),
                    &create_info.build(),
                    &mut command_buffer,
                )
            }
            .result()?;
            command_buffer
        };

        let geometry = GeometryBuffer::new(api)?;

        let fence = {
            let create_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
            unsafe { api.device.create_fence(&create_info, None) }?
        };

        frames.push(Frame {
            image,
            image_view,
            framebuffer,
            command_pool,
            command_buffer,
            geometry,
            fence,
        });
    }

    assert_eq!(frames.len(), images.len());

    Ok(())
}

struct FrameSync {
    acquire_semaphore: vk::Semaphore,
    present_semaphore: vk::Semaphore,
}

impl FrameSync {
    fn new(device: &ash::Device) -> Result<Self, Error> {
        let create_info = vk::SemaphoreCreateInfo::builder();
        Ok(Self {
            acquire_semaphore: unsafe { device.create_semaphore(&create_info, None) }?,
            present_semaphore: unsafe { device.create_semaphore(&create_info, None) }?,
        })
    }

    fn destroy(self, device: &ash::Device) {
        unsafe {
            device.destroy_semaphore(self.acquire_semaphore, None);
            device.destroy_semaphore(self.present_semaphore, None);
        }
    }
}

/// Utility struct containing per-swapchain members. Separate from `WindowData`
/// because all of this information changes when a swapchain resizes.
struct Swapchain {
    surface: vk::SurfaceKHR,
    handle: vk::SwapchainKHR,
    extent: vk::Extent2D,
    format: vk::Format,
    color_space: vk::ColorSpaceKHR,
}

impl Swapchain {
    fn new(api: &Vulkan, surface: vk::SurfaceKHR, extent: vk::Extent2D) -> Result<Self, Error> {
        Self::create_swapchain(api, surface, extent, vk::SwapchainKHR::null())
    }

    fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> Result<(), Error> {
        unsafe { api.device.device_wait_idle() }?;
        let new = Self::create_swapchain(api, self.surface, extent, self.handle)?;
        unsafe { api.swapchain_khr.destroy_swapchain(self.handle, None) };
        *self = new;
        Ok(())
    }

    fn destroy(self, api: &Vulkan) {
        unsafe {
            api.swapchain_khr.destroy_swapchain(self.handle, None);
        }
    }

    fn create_swapchain(
        api: &Vulkan,
        surface: vk::SurfaceKHR,
        #[allow(unused)] extent: vk::Extent2D,
        old_swapchain: vk::SwapchainKHR,
    ) -> Result<Swapchain, Error> {
        let vk::SurfaceFormatKHR {
            format,
            color_space,
        } = {
            let available = unsafe {
                api.surface_khr
                    .get_physical_device_surface_formats(api.physical_device.handle, surface)
            }?;

            let mut sdr = None;
            for format in available {
                if format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR {
                    match format.format {
                        vk::Format::R8G8B8A8_SRGB | vk::Format::B8G8R8A8_SRGB => sdr = Some(format),
                        _ => {}
                    }
                }

                if sdr.is_some() {
                    break;
                }
            }

            sdr.unwrap()
        };

        let capabilities = unsafe {
            api.surface_khr
                .get_physical_device_surface_capabilities(api.physical_device.handle, surface)
        }?;

        // Current extent is always defined as the size of the window on win32
        #[cfg(target_os = "windows")]
        let image_extent = capabilities.current_extent;

        let handle = {
            let min_image_count = if capabilities.max_image_array_layers == 0
                || capabilities.min_image_count <= PREFERRED_SWAPCHAIN_LENGTH
            {
                PREFERRED_SWAPCHAIN_LENGTH
            } else {
                capabilities.min_image_count
            };

            let concurrent_family_indices = [
                api.physical_device.graphics_queue_family,
                api.physical_device.present_queue_family,
            ];

            let needs_concurrent = api.physical_device.graphics_queue_family
                != api.physical_device.present_queue_family;

            let create_info = vk::SwapchainCreateInfoKHR {
                surface,
                min_image_count,
                image_format: format,
                image_color_space: color_space,
                image_extent,
                image_array_layers: 1,
                image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                image_sharing_mode: if needs_concurrent {
                    vk::SharingMode::CONCURRENT
                } else {
                    vk::SharingMode::EXCLUSIVE
                },
                queue_family_index_count: if needs_concurrent { 1 } else { 2 },
                p_queue_family_indices: concurrent_family_indices.as_ptr(),
                pre_transform: capabilities.current_transform,
                composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
                present_mode: vk::PresentModeKHR::FIFO,
                clipped: vk::TRUE,
                old_swapchain,
                ..Default::default()
            };

            unsafe { api.swapchain_khr.create_swapchain(&create_info, None) }?
        };

        Ok(Self {
            surface,
            handle,
            extent: image_extent,
            format,
            color_space,
        })
    }
}

/// Utility struct that holds members relating to a specific window. Swapchain
/// details are separate to delineate the frequency with which things change.
struct WindowData {
    /// The window's swapchain.
    swapchain: Swapchain,

    /// Dependent on swapchain format. Though this could technically change on
    /// resize, I know of no circumstance in which this actually happens.
    shader: Shader,

    /// A monotonically increasing id used to keep track of which `FrameSync`
    /// object to use each frame.
    frame_id: u64,

    /// Set between calls to `vkAcquireNextImageKHR` and `vkQueuePresentKHR`,
    /// this holds the image index (pointing into `frames`). An `Option<u32>`
    /// was selected instead of a bare `u32` to catch any instances where a user
    /// might attempt to present without first acquiring an image. No idea if
    /// this check is actually useful, but it was left in just in case.
    current_image: Option<u32>,

    // Though frames change every time the swapchain is resized, only a part of
    // it changes so it's been left out here. There is much opportunity to split
    // hairs here.
    frames: Vec<Frame>,

    /// Frame synchronization objects, used in alternating order as tracked by
    /// `frame_id`.
    frame_sync: [FrameSync; FRAMES_IN_FLIGHT],
}

impl WindowData {
    #[cfg(target_os = "windows")]
    fn new(api: &Vulkan, hwnd: windows::Win32::Foundation::HWND) -> Result<Self, Error> {
        use windows::Win32::{
            Foundation::RECT, System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::GetClientRect,
        };

        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe {
            api.win32_surface_khr
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

        Self::_new(api, surface, extent)
    }

    /// Platform-independent code for initializing a window. See `new` for the
    /// platform-dependent coe needed to call this method.
    fn _new(api: &Vulkan, surface: vk::SurfaceKHR, extent: vk::Extent2D) -> Result<Self, Error> {
        let swapchain = Swapchain::new(api, surface, extent)?;
        let shader = create_shader(api, swapchain.format)?;

        let mut frames = Vec::new();
        regenerate_frames(api, &swapchain, &shader, &mut frames)?;

        Ok(Self {
            swapchain,
            shader,
            frame_id: 0,
            current_image: None,
            frames,
            frame_sync: [FrameSync::new(&api.device)?, FrameSync::new(&api.device)?],
        })
    }

    /// Resize the swapchain and create the necessary per-frame data.
    fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> Result<(), Error> {
        unsafe { api.device.device_wait_idle() }?;
        self.swapchain.resize(api, extent)?;
        regenerate_frames(api, &self.swapchain, &self.shader, &mut self.frames)
    }

    fn get_next_image(&mut self, api: &Vulkan) -> Result<(), Error> {
        let sync = &self.frame_sync[self.frame_id as usize % self.frame_sync.len()];

        let (index, out_of_date) = unsafe {
            api.swapchain_khr.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                sync.acquire_semaphore,
                vk::Fence::null(),
            )
        }?;

        if out_of_date {
            Err(Error::SwapchainOutOfDate)
        } else {
            self.current_image = Some(index);
            Ok(())
        }
    }

    fn present(&mut self, api: &Vulkan) -> Result<(), Error> {
        let sync = &self.frame_sync[self.frame_id as usize % self.frame_sync.len()];

        if let Some(index) = self.current_image.take() {
            let mut results = [vk::Result::ERROR_UNKNOWN];
            unsafe {
                api.swapchain_khr.queue_present(
                    api.present_queue,
                    &vk::PresentInfoKHR::builder()
                        .wait_semaphores(&[sync.present_semaphore])
                        .swapchains(&[self.swapchain.handle])
                        .image_indices(&[index])
                        .results(&mut results),
                )
            }?;
            results[0].result()?;
            self.frame_id += 1;
            Ok(())
        } else {
            panic!("didn't acquire swapchain image before attempting to present")
        }
    }

    fn destroy(mut self, api: &Vulkan) {
        self.swapchain.destroy(api);
        self.shader.destroy(&api.device);
        for frame in self.frames.drain(..) {
            frame.destroy(api);
        }
        for sync in self.frame_sync {
            sync.destroy(&api.device);
        }
    }
}

/// Utility struct for holding a pipeline and render pass.
struct Shader {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
}

impl Shader {
    fn destroy(self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_pipeline_layout(self.layout, None);
        }
    }
}

fn create_shader(api: &Vulkan, format: vk::Format) -> Result<Shader, vk::Result> {
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

    Ok(Shader {
        pipeline,
        layout,
        render_pass,
    })
}

/// Utility struct for a VkBuffer suitable for vertices and indices.
struct GeometryBuffer {
    handle: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
    // first_vertex is assumed to be 0
    index_offset: vk::DeviceSize,
}

impl GeometryBuffer {
    const NUM_INIT_VERTICES: vk::DeviceSize = 1024 * 4;
    const NUM_INIT_INDICES: vk::DeviceSize = 1024 * 6;

    /// Allocates a new buffer suitable for 1024 rects (4096 vertices and 6144
    /// indices).
    fn new(api: &Vulkan) -> Result<Self, Error> {
        let index_offset = Self::index_offset(api, Self::NUM_INIT_VERTICES);
        let buffer_size = index_offset + Self::index_size(Self::NUM_INIT_INDICES);

        let (handle, memory) = Self::alloc(api, buffer_size)?;

        Ok(Self {
            handle,
            memory,
            size: buffer_size,
            index_offset,
        })
    }

    /// Destroys the buffer and frees its memory from the GPU.
    fn destroy(self, api: &Vulkan) {
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
    fn copy(&mut self, api: &Vulkan, vertices: &[Vertex], indices: &[u16]) -> Result<(), Error> {
        let index_offset = Self::index_offset(api, vertices.len() as vk::DeviceSize);
        let required_size = index_offset + Self::index_size(indices.len() as vk::DeviceSize);

        if required_size > self.size {
            unsafe {
                api.device.destroy_buffer(self.handle, None);
                api.device.free_memory(self.memory, None);
            }

            let (handle, memory) = Self::alloc(api, required_size)?;
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

    /// Allocates `size` bytes fand binds it to a buffer.
    fn alloc(api: &Vulkan, size: vk::DeviceSize) -> Result<(vk::Buffer, vk::DeviceMemory), Error> {
        let buffer = {
            let create_info = vk::BufferCreateInfo::builder()
                .size(size)
                .usage(vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER);

            unsafe { api.device.create_buffer(&create_info, None) }?
        };

        let memory = {
            let requirements = unsafe { api.device.get_buffer_memory_requirements(buffer) };
            let properties =
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::DEVICE_LOCAL;

            let type_index = Self::find_memory_type(
                &api.physical_device.memory_properties,
                requirements.memory_type_bits,
                properties,
            )
            .ok_or(vk::Result::ERROR_UNKNOWN)?;

            let create_info = vk::MemoryAllocateInfo::builder()
                .allocation_size(requirements.size)
                .memory_type_index(type_index);

            unsafe { api.device.allocate_memory(&create_info, None) }?
        };

        unsafe { api.device.bind_buffer_memory(buffer, memory, 0) }?;

        Ok((buffer, memory))
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

    fn find_memory_type(
        properties: &vk::PhysicalDeviceMemoryProperties,
        type_bits: u32,
        required_properties: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        (0..properties.memory_type_count).find(|&i| {
            (type_bits & (1 << i)) != 0
                && properties.memory_types[i as usize]
                    .property_flags
                    .contains(required_properties)
        })
    }
}

/// Writes everything that is needed to draw the commands to the command buffer,
/// including `vkBeginCommandbuffer` and `vkEndCommandBuffer`. You just need to
/// submit it to the graphics queue.
fn write_command_buffer(
    api: &Vulkan,
    polygon_shader: &Shader,
    commands: &DrawCommandList,
    target: vk::Framebuffer,
    viewport: vk::Extent2D,
    geometry: &mut GeometryBuffer,
    command_buffer: vk::CommandBuffer,
) -> Result<(), Error> {
    geometry.copy(api, &commands.vertices, &commands.indices)?;

    unsafe {
        api.device.begin_command_buffer(
            command_buffer,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;

        api.device.cmd_begin_render_pass(
            command_buffer,
            &vk::RenderPassBeginInfo::builder()
                .render_pass(polygon_shader.render_pass)
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

        let push_constants = PushConstantBytes {
            constants: PushConstants {
                scale: [2.0 / viewport.width as f32, 2.0 / viewport.height as f32],
                translate: [-1.0, -1.0],
            },
        };

        api.device.cmd_push_constants(
            command_buffer,
            polygon_shader.layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            &push_constants.bytes,
        );
    }

    for command in commands.commands.iter().chain(commands.current.as_ref()) {
        match command {
            super::Command::Scissor { rect } => unsafe {
                api.device
                    .cmd_set_scissor(command_buffer, 0, &[vk::Rect2D::from(*rect)])
            },
            super::Command::Polygon {
                first_index,
                num_indices,
            } => unsafe {
                api.device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    polygon_shader.pipeline,
                );

                api.device
                    .cmd_bind_vertex_buffers(command_buffer, 0, &[geometry.handle], &[0]);

                api.device.cmd_bind_index_buffer(
                    command_buffer,
                    geometry.handle,
                    geometry.index_offset,
                    vk::IndexType::UINT16,
                );

                api.device.cmd_draw_indexed(
                    command_buffer,
                    *num_indices as u32,
                    1,
                    *first_index as u32,
                    0,
                    0,
                );
            },
            super::Command::Texture {
                texture,
                first_index,
                first_uv,
                num_vertices,
                num_indices,
            } => todo!(),
        }
    }

    unsafe {
        api.device.cmd_end_render_pass(command_buffer);
        api.device.end_command_buffer(command_buffer)?;
    }

    Ok(())
}

impl Vertex {
    pub const BINDING_DESCRIPTION: vk::VertexInputBindingDescription =
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        };

    pub const ATTRIBUTE_DESCRIPTIONS: [vk::VertexInputAttributeDescription; 2] = [
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
}

/// Helper used to check if required and optional layers and extensions exist
/// within a set of items.
/// 
/// Returns `None` if one or more required names could not be found, or else
/// returns all the required names as well as every optional name that was
/// found.
fn has_names<T, F: Fn(&T) -> &[c_char]>(
    items: &[T],
    to_name: F,
    required: &[&[c_char]],
    optional: &[&[c_char]],
) -> Option<SmallVec<[*const c_char; 8]>> {
    let mut item_set = std::collections::HashSet::with_capacity(items.len());
    for item in items {
        let name = to_name(item);
        for (i, c) in name.iter().enumerate() {
            if *c == 0 {
                item_set.insert(&name[0..i + 1]);
                break;
            }
        }
    }

    let mut found_names = SmallVec::new();

    for name in required {
        if item_set.contains(name) {
            found_names.push(name.as_ptr());
        } else {
            return None;
        }
    }

    for name in optional {
        if item_set.contains(name) {
            found_names.push(name.as_ptr());
        }
    }

    Some(found_names)
}

/// Helper function for selecting a physical device. Moved out of
/// `Vulkan::new()` due to its size.
fn select_gpu(
    instance: &ash::Instance,
    can_present: impl Fn(vk::PhysicalDevice, u32) -> bool,
) -> Result<(PhysicalDevice, SmallVec<[*const c_char; 8]>), Error> {
    for gpu in unsafe { instance.enumerate_physical_devices() }? {
        let (mut graphics, mut transfer, mut present) = (None, None, None);

        let queue_families = unsafe { instance.get_physical_device_queue_family_properties(gpu) };
        for (index, queue_family) in queue_families.iter().enumerate() {
            let index = index.try_into().unwrap();

            if can_present(gpu, index) {
                present = present.or(Some(index));
            }

            if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                graphics = graphics.or(Some(index));
            }

            if queue_family.queue_flags.contains(vk::QueueFlags::TRANSFER)
                && !queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            {
                transfer = transfer.or(Some(index));
            }
        }

        if let (Some(graphics), Some(present)) = (graphics, present) {
            let extensions = has_names(
                &unsafe { instance.enumerate_device_extension_properties(gpu) }?,
                |e| &e.extension_name,
                REQUIRED_DEVICE_EXTENSIONS,
                OPTIONAL_DEVICE_EXTENSIONS,
            );

            if let Some(extensions) = extensions {
                let properties = unsafe { instance.get_physical_device_properties(gpu) };
                let memory_properties =
                    unsafe { instance.get_physical_device_memory_properties(gpu) };

                return Ok((
                    PhysicalDevice {
                        handle: gpu,
                        properties,
                        graphics_queue_family: graphics,
                        transfer_queue_family: transfer.unwrap_or(graphics),
                        present_queue_family: present,
                        memory_properties,
                    },
                    extensions,
                ));
            }
        }
    }
    Err(Error::NoGraphicsDevice)
}

/// Copied from unstable std while waiting for #![feature(int_roundigs)] to
/// stabilize.
///
/// https://github.com/rust-lang/rust/issues/88581
const fn next_multiple_of(lhs: vk::DeviceSize, rhs: vk::DeviceSize) -> vk::DeviceSize {
    match lhs % rhs {
        0 => lhs,
        r => lhs + (rhs - r),
    }
}

impl From<crate::handle_pool::Error> for Error {
    fn from(e: crate::handle_pool::Error) -> Self {
        match e {
            crate::handle_pool::Error::TooManyObjects {
                num_allocated: _,
                num_retired: _,
                capacity,
            }
            | crate::handle_pool::Error::Exhausted { capacity } => Self::TooManyObjects {
                limit: capacity as u32,
            },
        }
    }
}

impl From<Extent> for vk::Extent2D {
    fn from(e: Extent) -> Self {
        Self {
            width: e.width.0.try_into().unwrap(),
            height: e.height.0.try_into().unwrap(),
        }
    }
}

impl From<Rect> for vk::Rect2D {
    fn from(r: Rect) -> Self {
        Self {
            offset: vk::Offset2D {
                x: r.left.0 as i32,
                y: r.top.0 as i32,
            },
            extent: vk::Extent2D {
                width: (r.right - r.left).0 as u32,
                height: (r.bottom - r.top).0 as u32,
            },
        }
    }
}
