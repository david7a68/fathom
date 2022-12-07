use ash::vk;
use smallvec::SmallVec;

use super::{
    api::{VkResult, Vulkan},
    RenderFrame, FRAMES_IN_FLIGHT, PREFERRED_SWAPCHAIN_LENGTH,
};

pub struct FrameSync {
    pub acquire_semaphore: vk::Semaphore,
    pub present_semaphore: vk::Semaphore,
}

/// Utility struct that holds members relating to a specific window. Swapchain
/// details are separate to delineate the frequency with which things change.
pub struct Window {
    /// The window's swapchain.
    swapchain: Swapchain,

    /// A monotonically increasing id used to keep track of which `FrameSync`
    /// object to use each frame.
    frame_id: u64,

    /// Set between calls to `vkAcquireNextImageKHR` and `vkQueuePresentKHR`,
    /// this holds the image index (pointing into `frames`). An `Option<u32>`
    /// was selected instead of a bare `u32` to catch any instances where a user
    /// might attempt to present without first acquiring an image. No idea if
    /// this check is actually useful, but it was left in just in case.
    current_image: Option<u32>,

    /// SwapchainImage synchronization objects, used in alternating order as tracked by
    /// `frame_id`.
    frame_sync: [FrameSync; FRAMES_IN_FLIGHT],

    render_targets: [RenderFrame; FRAMES_IN_FLIGHT],
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
        Ok(Self {
            swapchain: Swapchain::new(api, surface, extent)?,
            frame_id: 0,
            current_image: None,
            frame_sync: [
                FrameSync {
                    acquire_semaphore: api.create_semaphore(false).unwrap(),
                    present_semaphore: api.create_semaphore(false).unwrap(),
                },
                FrameSync {
                    acquire_semaphore: api.create_semaphore(false).unwrap(),
                    present_semaphore: api.create_semaphore(false).unwrap(),
                },
            ],
            render_targets: [RenderFrame::new(api), RenderFrame::new(api)],
        })
    }

    pub fn destroy(self, api: &Vulkan) {
        self.swapchain.destroy(api);
        for sync in self.frame_sync {
            unsafe {
                api.device.destroy_semaphore(sync.acquire_semaphore, None);
                api.device.destroy_semaphore(sync.present_semaphore, None);
            }
        }
        for target in self.render_targets {
            target.destroy(api);
        }
    }

    pub fn format(&self) -> vk::Format {
        self.swapchain.format
    }

    pub(super) fn render_state(
        &mut self,
    ) -> (vk::ImageView, vk::Extent2D, &FrameSync, &mut RenderFrame) {
        (
            self.swapchain.views[self.current_image.unwrap() as usize],
            self.swapchain.extent,
            &self.frame_sync[self.frame_id as usize % FRAMES_IN_FLIGHT],
            &mut self.render_targets[self.frame_id as usize % FRAMES_IN_FLIGHT],
        )
    }

    /// Resize the swapchain and create the necessary per-frame data.
    pub fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> VkResult<()> {
        unsafe { api.device.device_wait_idle() }?;
        self.swapchain.resize(api, extent)?;
        Ok(())
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

/// Utility struct containing per-swapchain members. Separate from `WindowData`
/// because all of this information changes when a swapchain resizes.
struct Swapchain {
    surface: vk::SurfaceKHR,
    handle: vk::SwapchainKHR,
    extent: vk::Extent2D,
    format: vk::Format,
    views: SmallVec<[vk::ImageView; PREFERRED_SWAPCHAIN_LENGTH as usize]>,
}

impl Swapchain {
    fn new(api: &Vulkan, surface: vk::SurfaceKHR, extent: vk::Extent2D) -> VkResult<Self> {
        Self::create_swapchain(api, surface, extent, vk::SwapchainKHR::null())
    }

    fn resize(&mut self, api: &Vulkan, extent: vk::Extent2D) -> VkResult<()> {
        unsafe { api.device.device_wait_idle() }?;

        let mut new = Self::create_swapchain(api, self.surface, extent, self.handle)?;
        std::mem::swap(&mut new, self);
        new.destroy(api);

        Ok(())
    }

    fn destroy(mut self, api: &Vulkan) {
        unsafe {
            for view in self.views.drain(..) {
                api.device.destroy_image_view(view, None);
            }

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

        let views = {
            let images = unsafe { api.swapchain_khr.get_swapchain_images(handle) }.unwrap();
            let mut views = SmallVec::with_capacity(images.len());
            for image in images {
                views.push(api.create_image_view(image, format).unwrap());
            }
            views
        };

        Ok(Self {
            surface,
            handle,
            extent: image_extent,
            format,
            views,
        })
    }
}
