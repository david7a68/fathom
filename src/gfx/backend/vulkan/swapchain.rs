use std::cell::Cell;

use ash::vk;
use smallvec::SmallVec;

use crate::gfx::{
    backend::{Error, MAX_SWAPCHAINS},
    geometry::Extent,
};

use super::api::VulkanApi;

const FRAMES_IN_FLIGHT: usize = 2;
const PREFERRED_NUM_IMAGES: usize = 2;

#[derive(Clone)]
pub struct FrameSync {
    pub acquire_semaphore: vk::Semaphore,
    pub present_semaphore: vk::Semaphore,
    pub acquire_fence: vk::Fence,
    pub submit_fence: vk::Fence,
}

impl FrameSync {
    fn new(api: &VulkanApi) -> Result<Self, Error> {
        Ok(Self {
            acquire_semaphore: api.create_semaphore()?,
            present_semaphore: api.create_semaphore()?,
            acquire_fence: api.create_fence(false)?,
            submit_fence: api.create_fence(true)?,
        })
    }

    fn destroy(self, api: &VulkanApi) {
        unsafe {
            api.device.destroy_semaphore(self.acquire_semaphore, None);
            api.device.destroy_semaphore(self.present_semaphore, None);
            api.device.destroy_fence(self.acquire_fence, None);
            api.device.destroy_fence(self.submit_fence, None);
        }
    }
}

pub struct Frame {
    pub sync: FrameSync,
    pub image_view: vk::ImageView,
}

pub struct VulkanSwapchain {
    inner: SwapchainInner,
    current_frame: Cell<u64>,
    current_image: Option<u32>,
    frames: [FrameSync; FRAMES_IN_FLIGHT],
}

impl VulkanSwapchain {
    #[cfg(target_os = "windows")]
    pub fn new(hwnd: windows::Win32::Foundation::HWND, api: &VulkanApi) -> Result<Self, Error> {
        Ok(Self {
            inner: SwapchainInner::new(hwnd, api)?,
            current_frame: Cell::new(0),
            current_image: None,
            frames: [FrameSync::new(api)?, FrameSync::new(api)?],
        })
    }

    pub fn wait_idle(&self, api: &VulkanApi) -> Result<(), Error> {
        let fences = [self.frames[0].submit_fence, self.frames[1].submit_fence];
        unsafe { api.device.wait_for_fences(&fences, true, u64::MAX) }?;
        Ok(())
    }

    pub fn destroy(mut self, api: &VulkanApi) -> Result<(), Error> {
        let [frame0, frame1] = self.frames;
        let fences = [frame0.submit_fence, frame1.submit_fence];
        match unsafe { api.device.wait_for_fences(&fences, true, 0) } {
            Ok(_) => {
                self.inner.destroy(api);
                frame0.destroy(api);
                frame1.destroy(api);
                Ok(())
            }
            Err(vk::Result::TIMEOUT) => {
                panic!("cannot destroy a swapchain that is still in use, call wait_idle first")
            }
            Err(e) => Err(Error::VulkanInternal { error_code: e }),
        }
    }

    pub fn resize(&mut self, api: &VulkanApi, new_size: Extent) -> Result<(), Error> {
        self.inner.update(new_size.into(), api)?;
        Ok(())
    }

    pub fn get_next_image(&mut self, api: &VulkanApi) -> Result<(), Error> {
        assert!(
            self.current_image.is_none(),
            "cannot acquire more images from swapchain than have been presented"
        );

        let sync = self.frame().clone();
        let (index, out_of_date) = unsafe {
            // may be a sync error here, need to reset fence/semaphore?
            //
            // should this wait? is there a better way to do this?
            api.swapchain_khr.acquire_next_image(
                self.inner.handle,
                u64::MAX,
                sync.acquire_semaphore,
                sync.acquire_fence,
            )
        }?;

        if out_of_date {
            Err(Error::SwapchainOutOfDate)
        } else {
            self.current_image = Some(index);
            Ok(())
        }
    }

    /// Presents the swapchains, blocking until all have flipped.
    pub fn present(api: &VulkanApi, swapchains: &[&VulkanSwapchain]) -> Result<(), Error> {
        let mut handles = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::new();
        let mut images = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::new();
        let mut fences = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::new();
        let mut semaphores = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::new();

        for swapchain in swapchains {
            handles.push(swapchain.inner.handle);
            images.push(
                swapchain
                    .current_image
                    .expect("cannot present a swapchain image that has not been acquired"),
            );
            let frame = swapchain.frame();
            fences.push(frame.submit_fence);
            semaphores.push(frame.present_semaphore);
        }

        let mut results = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::from_elem(
            vk::Result::SUCCESS,
            swapchains.len(),
        );

        let _ = unsafe {
            api.swapchain_khr.queue_present(
                api.present_queue,
                &vk::PresentInfoKHR {
                    wait_semaphore_count: semaphores.len() as u32,
                    p_wait_semaphores: semaphores.as_ptr(),
                    swapchain_count: handles.len() as u32,
                    p_swapchains: handles.as_ptr(),
                    p_image_indices: images.as_ptr(),
                    p_results: results.as_mut_ptr(),
                    ..Default::default()
                },
            )
        }?;

        unsafe { api.device.wait_for_fences(&fences, true, u64::MAX) }?;

        for swapchain in swapchains {
            swapchain
                .current_frame
                .set(swapchain.current_frame.get() + 1);
        }

        Ok(())
    }

    fn frame(&self) -> &FrameSync {
        &self.frames[self.current_frame.get() as usize % FRAMES_IN_FLIGHT]
    }
}

struct SwapchainInner {
    handle: vk::SwapchainKHR,
    surface: vk::SurfaceKHR,
    format: vk::SurfaceFormatKHR,
    image_views: SmallVec<[vk::ImageView; PREFERRED_NUM_IMAGES]>,
}

impl SwapchainInner {
    #[cfg(target_os = "windows")]
    fn new(hwnd: windows::Win32::Foundation::HWND, api: &VulkanApi) -> Result<Self, Error> {
        use windows::Win32::{
            Foundation::RECT, System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::GetClientRect,
        };

        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe { api.os_surface_khr.create_win32_surface(&surface_ci, None)? };

        let extent = unsafe {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            vk::Extent2D {
                width: u32::try_from(rect.right).unwrap(),
                height: u32::try_from(rect.bottom).unwrap(),
            }
        };

        Self::create_swapchain(surface, extent, None, api)
    }

    fn update(&mut self, new_size: vk::Extent2D, api: &VulkanApi) -> Result<(), Error> {
        let new = Self::create_swapchain(self.surface, new_size, Some(self.handle), api)?;

        unsafe {
            for view in self.image_views.drain(..) {
                api.device.destroy_image_view(view, None);
            }
            api.swapchain_khr.destroy_swapchain(self.handle, None);
        }

        *self = new;
        Ok(())
    }

    fn destroy(&mut self, api: &VulkanApi) {
        unsafe {
            for view in self.image_views.drain(..) {
                api.device.destroy_image_view(view, None);
            }
            api.swapchain_khr.destroy_swapchain(self.handle, None);
            api.surface_khr.destroy_surface(self.surface, None);
        }
    }

    fn create_swapchain(
        surface: vk::SurfaceKHR,
        #[allow(unused)] extent: vk::Extent2D,
        old: Option<vk::SwapchainKHR>,
        api: &VulkanApi,
    ) -> Result<Self, Error> {
        let format = {
            let available = unsafe {
                api.surface_khr
                    .get_physical_device_surface_formats(api.physical_device, surface)
            }?;

            let mut rgb8_srgb = None;
            for format in available {
                match format.color_space {
                    vk::ColorSpaceKHR::SRGB_NONLINEAR => match format.format {
                        vk::Format::R8G8B8_SRGB | vk::Format::B8G8R8A8_SRGB => {
                            rgb8_srgb = rgb8_srgb.or(Some(format));
                        }
                        _ => {}
                    },
                    vk::ColorSpaceKHR::BT2020_LINEAR_EXT => {}
                    _ => {}
                }
            }

            // if let Some(format) = rgb16f_bt2020 {
            //     format
            // } else
            if let Some(format) = rgb8_srgb {
                format
            } else {
                panic!("no srgb format found")
            }
        };

        let capabilities = unsafe {
            api.surface_khr
                .get_physical_device_surface_capabilities(api.physical_device, surface)
        }?;

        #[cfg(target_os = "windows")]
        let image_extent = capabilities.current_extent;

        let handle = {
            let min_image_count = if capabilities.max_image_array_layers == 0
                || capabilities.min_image_count <= PREFERRED_NUM_IMAGES as u32
            {
                PREFERRED_NUM_IMAGES as u32
            } else {
                capabilities.min_image_count
            };

            let concurrent_family_indices = [api.graphics_queue_family, api.present_queue_family];

            let needs_concurrent = api.graphics_queue_family != api.present_queue_family;

            let create_info = vk::SwapchainCreateInfoKHR {
                surface,
                min_image_count,
                image_format: format.format,
                image_color_space: format.color_space,
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
                old_swapchain: old.unwrap_or(vk::SwapchainKHR::null()),
                ..Default::default()
            };

            unsafe { api.swapchain_khr.create_swapchain(&create_info, None) }?
        };

        let image_views = {
            let mut images = unsafe { api.swapchain_khr.get_swapchain_images(handle) }?;
            let mut views = SmallVec::<[_; PREFERRED_NUM_IMAGES]>::new();

            for image in images.drain(..) {
                let create_info = vk::ImageViewCreateInfo {
                    image,
                    view_type: vk::ImageViewType::TYPE_2D,
                    format: format.format,
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

                views.push(unsafe { api.device.create_image_view(&create_info, None) }?);
            }

            views
        };

        Ok(Self {
            handle,
            surface,
            format,
            image_views,
        })
    }
}
