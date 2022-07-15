use ash::vk;

use super::{error::Error, Device};

pub const FRAMES_IN_FLIGHT: u32 = 2;
pub const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

pub struct Swapchain {
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub surface: vk::SurfaceKHR,
    pub image_views: Vec<vk::ImageView>,

    pub current_frame: u32,
    pub current_image: u32,

    pub frames_in_flight: FramesInFlight,
}

pub struct FramesInFlight {
    /// Semaphores indicating when the swapchain image can be rendered to.
    pub acquire_semaphores: [vk::Semaphore; DESIRED_SWAPCHAIN_LENGTH as usize],
    /// Semaphores indicating when the image is ready to be presented.
    pub present_semaphores: [vk::Semaphore; DESIRED_SWAPCHAIN_LENGTH as usize],
    /// Fences indicating when the command buffer is finished.
    pub fences: [vk::Fence; DESIRED_SWAPCHAIN_LENGTH as usize],
}

pub(super) fn create(
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

pub(super) fn destroy(
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

pub(super) fn acquire_next_image(
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

pub(super) fn present(
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
