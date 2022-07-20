use ash::vk;

use super::{error::Error, Device};

pub const FRAMES_IN_FLIGHT: u32 = 2;
pub const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

#[derive(Debug)]
pub struct Swapchain {
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub surface: vk::SurfaceKHR,
    pub image_views: Vec<vk::ImageView>,

    pub current_frame: u32,
    pub current_image: Option<u32>,

    pub frame_sync_objects: [FrameSyncObjects; FRAMES_IN_FLIGHT as usize],
}

#[derive(Debug)]
pub struct FrameSyncObjects {
    pub acquire_semaphore: vk::Semaphore,
    pub present_semaphore: vk::Semaphore,
    pub fence: vk::Fence,
}

pub(super) fn create(
    device: &Device,
    surface: vk::SurfaceKHR,
    extent: vk::Extent2D,
    surface_api: &ash::extensions::khr::Surface,
) -> Result<(vk::SwapchainKHR, Swapchain), Error> {
    let frame_sync_objects = unsafe {
        let semaphore_ci = vk::SemaphoreCreateInfo::builder();
        let fence_ci = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

        [
            FrameSyncObjects {
                acquire_semaphore: device.device.create_semaphore(&semaphore_ci, None)?,
                present_semaphore: device.device.create_semaphore(&semaphore_ci, None)?,
                fence: device.device.create_fence(&fence_ci, None)?,
            },
            FrameSyncObjects {
                acquire_semaphore: device.device.create_semaphore(&semaphore_ci, None)?,
                present_semaphore: device.device.create_semaphore(&semaphore_ci, None)?,
                fence: device.device.create_fence(&fence_ci, None)?,
            },
        ]
    };

    let (handle, format, extent, image_views) = create_raw_swapchain(
        device,
        surface,
        extent,
        vk::SwapchainKHR::null(),
        surface_api,
    )?;

    Ok((
        handle,
        Swapchain {
            format,
            extent,
            surface,
            image_views,
            current_frame: 0,
            current_image: None,
            frame_sync_objects,
        },
    ))
}

pub(super) fn resize(
    device: &Device,
    old_handle: vk::SwapchainKHR,
    old_swapchain: Swapchain,
    new_size: vk::Extent2D,
    surface_api: &ash::extensions::khr::Surface,
) -> Result<(vk::SwapchainKHR, Swapchain), Error> {
    wait_idle(device, &old_swapchain)?;

    let (handle, format, extent, image_views) = create_raw_swapchain(
        device,
        old_swapchain.surface,
        new_size,
        old_handle,
        surface_api,
    )?;

    unsafe {
        device.swapchain_api.destroy_swapchain(old_handle, None);

        for image_view in old_swapchain.image_views.iter() {
            device.device.destroy_image_view(*image_view, None);
        }
    }

    Ok((
        handle,
        Swapchain {
            format,
            extent,
            surface: old_swapchain.surface,
            image_views,
            current_frame: old_swapchain.current_frame,
            current_image: None,
            frame_sync_objects: old_swapchain.frame_sync_objects,
        },
    ))
}

pub(super) fn destroy(
    device: &Device,
    surface_api: &ash::extensions::khr::Surface,
    handle: vk::SwapchainKHR,
    mut data: Swapchain,
) -> Result<(), Error> {
    wait_idle(device, &data)?;
    
    let vkdevice = &device.device;
    unsafe {
        for view in data.image_views.drain(..) {
            vkdevice.destroy_image_view(view, None);
        }

        for sync in data.frame_sync_objects {
            vkdevice.destroy_semaphore(sync.acquire_semaphore, None);
            vkdevice.destroy_semaphore(sync.present_semaphore, None);
            vkdevice.destroy_fence(sync.fence, None);
        }

        device.swapchain_api.destroy_swapchain(handle, None);
        surface_api.destroy_surface(data.surface, None);
    }

    Ok(())
}

pub(super) fn frame_objects(swapchain: &Swapchain) -> (usize, &FrameSyncObjects) {
    let index = (swapchain.current_frame % DESIRED_SWAPCHAIN_LENGTH) as usize;
    (index, &swapchain.frame_sync_objects[index])
}

pub(super) fn wait_idle(device: &Device, swapchain: &Swapchain) -> Result<(), Error> {
    let fences = [
        swapchain.frame_sync_objects[0].fence,
        swapchain.frame_sync_objects[1].fence,
    ];

    unsafe { device.device.wait_for_fences(&fences, true, u64::MAX) }?;
    Ok(())
}

pub(super) fn acquire_next_image(
    device: &Device,
    handle: vk::SwapchainKHR,
    swapchain: &mut Swapchain,
) -> Result<(), Error> {
    let (_, sync_objects) = frame_objects(swapchain);
    
    let vkdevice = &device.device;
    unsafe { vkdevice.wait_for_fences(&[sync_objects.fence], true, u64::MAX) }?;

    let (index, needs_resize) = unsafe {
        device.swapchain_api.acquire_next_image(
            handle,
            u64::MAX,
            sync_objects.acquire_semaphore,
            vk::Fence::null(),
        )?
    };

    if needs_resize {
        Err(Error::SwapchainOutOfDate)
    } else {
        unsafe { device.device.reset_fences(&[sync_objects.fence]) }?;
        swapchain.current_image = Some(index);
        Ok(())
    }
}

pub(super) fn present(
    device: &Device,
    handle: vk::SwapchainKHR,
    swapchain: &mut Swapchain,
) -> Result<(), Error> {
    let (_, frame_objects) = frame_objects(swapchain);

    let out_of_date = unsafe {
        device.swapchain_api.queue_present(
            device.present_queue,
            &vk::PresentInfoKHR::builder()
                .wait_semaphores(&[frame_objects.present_semaphore])
                .swapchains(&[handle])
                .image_indices(&[swapchain.current_image.take().unwrap()]),
        )
    }?;

    swapchain.current_frame += 1;

    if out_of_date {
        Err(Error::SwapchainOutOfDate)
    } else {
        Ok(())
    }
}

fn create_raw_swapchain(
    device: &Device,
    surface: vk::SurfaceKHR,
    extent: vk::Extent2D,
    old_swapchain: vk::SwapchainKHR,
    surface_api: &ash::extensions::khr::Surface,
) -> Result<
    (
        vk::SwapchainKHR,
        vk::Format,
        vk::Extent2D,
        Vec<vk::ImageView>,
    ),
    Error,
> {
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

    let handle = {
        let min_images = if capabilities.max_image_count == 0
            || capabilities.min_image_count <= DESIRED_SWAPCHAIN_LENGTH
        {
            DESIRED_SWAPCHAIN_LENGTH
        } else {
            capabilities.min_image_count
        };

        let concurrent_family_indices = &[device.graphics_family, device.present_family];
        let mut swapchain_ci = vk::SwapchainCreateInfoKHR::builder()
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
            .clipped(true)
            .old_swapchain(old_swapchain);

        swapchain_ci.image_sharing_mode = if device.graphics_family == device.present_family {
            vk::SharingMode::EXCLUSIVE
        } else {
            vk::SharingMode::CONCURRENT
        };

        unsafe { device.swapchain_api.create_swapchain(&swapchain_ci, None)? }
    };

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

    Ok((handle, format.format, extent, image_views))
}
