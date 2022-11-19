use ash::vk;

use super::{
    api::{VkResult, Vulkan},
    ui_shader::{UiGeometryBuffer, UiShader},
    FRAMES_IN_FLIGHT, PREFERRED_SWAPCHAIN_LENGTH,
};

/// Utility struct that holds members relating to a specific window. Swapchain
/// details are separate to delineate the frequency with which things change.
pub struct Window {
    /// The window's swapchain.
    swapchain: Swapchain,

    /// Dependent on swapchain format. Though this could technically change on
    /// resize, I know of no circumstance in which this actually happens.
    shader: UiShader,

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

impl Window {
    #[cfg(target_os = "windows")]
    pub fn new(api: &Vulkan, hwnd: windows::Win32::Foundation::HWND) -> VkResult<Self> {
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
    fn _new(api: &Vulkan, surface: vk::SurfaceKHR, extent: vk::Extent2D) -> VkResult<Self> {
        let swapchain = Swapchain::new(api, surface, extent)?;
        let shader = UiShader::new(api, swapchain.format)?;

        let mut frames = Vec::new();
        regenerate_frames(api, &swapchain, &shader, &mut frames)?;

        Ok(Self {
            swapchain,
            shader,
            frame_id: 0,
            current_image: None,
            frames,
            frame_sync: [FrameSync::new(api)?, FrameSync::new(api)?],
        })
    }

    pub fn destroy(mut self, api: &Vulkan) {
        self.swapchain.destroy(api);
        self.shader.destroy(&api.device);
        for frame in self.frames.drain(..) {
            frame.destroy(api);
        }
        for sync in self.frame_sync {
            sync.destroy(&api.device);
        }
    }

    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }

    pub fn render_state(&mut self) -> (&mut Frame, vk::Extent2D, &FrameSync, &UiShader) {
        (
            &mut self.frames[self.current_image.unwrap() as usize],
            self.swapchain.extent,
            &self.frame_sync[self.frame_id as usize % FRAMES_IN_FLIGHT],
            &self.shader,
        )
    }

    /// Resize the swapchain and create the necessary per-frame data.
    pub fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> VkResult<()> {
        unsafe { api.device.device_wait_idle() }?;
        self.swapchain.resize(api, extent)?;
        regenerate_frames(api, &self.swapchain, &self.shader, &mut self.frames)
    }

    pub fn get_next_image(&mut self, api: &Vulkan) -> VkResult<()> {
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
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR)
        } else {
            self.current_image = Some(index);
            Ok(())
        }
    }

    pub fn present(&mut self, api: &Vulkan) -> VkResult<()> {
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
}

/// Utility struct that contains all the ancillary information needed to render
/// a frame. Each window has `FRAME_IN_FLIGHT` `Frame`s that are used
/// alternately to allow a previously submitted frame to complete on the GPU.
pub struct Frame {
    pub image: vk::Image,
    pub image_view: vk::ImageView,
    pub framebuffer: vk::Framebuffer,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,

    pub geometry: UiGeometryBuffer,
    // todo: smallvec?
    pub descriptors: Vec<vk::DescriptorSet>,

    /// The fence is used to determine when the GPU is done rendering this
    /// frame. Once rendering is done, the command pool can be reset, and the
    /// buffer reused.
    ///
    /// NOTE: It is not sufficient to check that all the fences are signalled
    /// before resizing a window! Check either that the graphics queue or the
    /// device is idle.
    pub fence: vk::Fence,
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

    pub fn reset(&self, api: &Vulkan) -> VkResult<()> {
        unsafe {
            api.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            api.device.reset_fences(&[self.fence])?;
            api.device
                .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
        }
        Ok(())
    }
}

fn regenerate_frames(
    api: &Vulkan,
    swapchain: &Swapchain,
    shader: &UiShader,
    frames: &mut Vec<Frame>,
) -> VkResult<()> {
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
        frame.image_view = api.create_image_view(frame.image, swapchain.format)?;
        frame.framebuffer = shader.create_framebuffer(api, frame.image_view, swapchain.extent)?;
    }

    // if there are more images than frames
    for image in &images[frames.len()..] {
        let image = *image;
        let image_view = api.create_image_view(image, swapchain.format)?;
        let framebuffer = shader.create_framebuffer(api, image_view, swapchain.extent)?;

        let command_pool = {
            let create_info = vk::CommandPoolCreateInfo::builder()
                .queue_family_index(api.physical_device.graphics_queue_family);
            unsafe { api.device.create_command_pool(&create_info, None) }?
        };

        let command_buffer = api.allocate_command_buffer(command_pool)?;

        let geometry = UiGeometryBuffer::new(api)?;

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
            descriptors: Vec::new(),
            fence,
        });
    }

    assert_eq!(frames.len(), images.len());

    Ok(())
}

/// Utility struct containing per-swapchain members. Separate from `WindowData`
/// because all of this information changes when a swapchain resizes.
struct Swapchain {
    surface: vk::SurfaceKHR,
    handle: vk::SwapchainKHR,
    extent: vk::Extent2D,
    format: vk::Format,
}

impl Swapchain {
    fn new(api: &Vulkan, surface: vk::SurfaceKHR, extent: vk::Extent2D) -> VkResult<Self> {
        Self::create_swapchain(api, surface, extent, vk::SwapchainKHR::null())
    }

    fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> VkResult<()> {
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
    ) -> VkResult<Swapchain> {
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
        })
    }
}

pub struct FrameSync {
    pub acquire_semaphore: vk::Semaphore,
    pub present_semaphore: vk::Semaphore,
}

impl FrameSync {
    fn new(api: &Vulkan) -> VkResult<Self> {
        Ok(Self {
            acquire_semaphore: api.create_semaphore(false)?,
            present_semaphore: api.create_semaphore(false)?,
        })
    }

    fn destroy(self, device: &ash::Device) {
        unsafe {
            device.destroy_semaphore(self.acquire_semaphore, None);
            device.destroy_semaphore(self.present_semaphore, None);
        }
    }
}
