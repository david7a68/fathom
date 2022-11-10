mod texture;
mod ui_shader;
mod window;

use std::{cell::RefCell, ffi::c_char};

use ash::vk;
use smallvec::SmallVec;

use crate::handle_pool::{Handle, HandlePool};

use self::{texture::Texture, window::Window};

use super::{
    geometry::{Extent, Rect},
    pixel_buffer::{ColorSpace, Layout, PixelBuffer, PixelBufferView},
    DrawCommandList, Error, GfxDevice, Resample, SubImageUpdate, MAX_IMAGES, MAX_SWAPCHAINS,
};

const fn as_cchar_slice(slice: &[u8]) -> &[c_char] {
    unsafe { std::mem::transmute(slice) }
}

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

type VkResult<T> = Result<T, vk::Result>;

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

    windows: RefCell<HandlePool<Window, super::Swapchain, MAX_SWAPCHAINS>>,
    render_targets: RefCell<HandlePool<RenderTarget, super::RenderTarget, 128>>,
    images: RefCell<HandlePool<Texture, super::Image, MAX_IMAGES>>,
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
                api_version: vk::make_api_version(0, 1, 2, 0),
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

            // Enable timeline semaphores
            let mut features12 = vk::PhysicalDeviceVulkan12Features::default();
            let mut features = vk::PhysicalDeviceFeatures2::builder().push_next(&mut features12);
            unsafe { instance.get_physical_device_features2(gpu.handle, &mut features) };

            let mut features = if features12.timeline_semaphore == vk::TRUE {
                features12 = vk::PhysicalDeviceVulkan12Features::default();
                features12.timeline_semaphore = vk::TRUE;
                vk::PhysicalDeviceFeatures2::builder()
                    .push_next(&mut features12)
                    .build()
            } else {
                return Err(Error::NoGraphicsDevice);
            };

            let create_info = vk::DeviceCreateInfo::builder()
                .push_next(&mut features)
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
            images: RefCell::new(HandlePool::preallocate_n(8)),
        })
    }

    pub(self) fn find_memory_type(
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

    fn create_image_view(&self, image: vk::Image, format: vk::Format) -> VkResult<vk::ImageView> {
        let create_info = vk::ImageViewCreateInfo {
            flags: vk::ImageViewCreateFlags::empty(),
            image,
            view_type: vk::ImageViewType::TYPE_2D,
            format,
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

        unsafe { self.device.create_image_view(&create_info, None) }
    }

    pub fn create_semaphore(&self, timeline: bool) -> VkResult<vk::Semaphore> {
        let timeline_info = vk::SemaphoreTypeCreateInfo {
            semaphore_type: vk::SemaphoreType::TIMELINE,
            initial_value: 0,
            ..Default::default()
        };

        let create_info = vk::SemaphoreCreateInfo {
            p_next: if timeline {
                &timeline_info as *const vk::SemaphoreTypeCreateInfo as *const _
            } else {
                std::ptr::null()
            },
            ..Default::default()
        };

        unsafe { self.device.create_semaphore(&create_info, None) }
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
        let window = Window::new(self, hwnd)?;
        Ok(self.windows.borrow_mut().insert(window)?)
    }

    fn resize_swapchain(
        &self,
        handle: Handle<super::Swapchain>,
        extent: Extent,
    ) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle)?;
        unsafe { self.device.device_wait_idle() }?;
        window.resize(self, extent.into())?;
        Ok(())
    }

    fn destroy_swapchain(&self, handle: Handle<super::Swapchain>) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.remove(handle)?;
        unsafe { self.device.device_wait_idle() }?;
        window.destroy(self);
        Ok(())
    }

    fn get_next_swapchain_image(
        &self,
        handle: Handle<super::Swapchain>,
    ) -> Result<Handle<super::RenderTarget>, Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle)?;
        window.get_next_image(self)?;

        let mut render_targets = self.render_targets.borrow_mut();
        let handle = render_targets.insert(RenderTarget::Swapchain(handle, window.frame_id()))?;
        Ok(handle)
    }

    fn present_swapchain_images(&self, handles: &[Handle<super::Swapchain>]) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        for handle in handles {
            let window = windows.get_mut(*handle)?;
            match window.present(self) {
                Ok(()) => Ok(()),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(Error::SwapchainOutOfDate),
                Err(e) => Err(Error::VulkanInternal { error_code: e }),
            }?
        }
        Ok(())
    }

    fn create_image(&self, extent: Extent) -> Result<Handle<super::Image>, Error> {
        Ok(self
            .images
            .borrow_mut()
            .insert(Texture::new(self, extent)?)?)
    }

    fn upload_image(
        &self,
        extent: Extent,
        pixels: PixelBufferView,
        resample_mode: Resample,
    ) -> Result<Handle<super::Image>, Error> {
        todo!()
    }

    fn copy_image(
        &self,
        src: Handle<super::Image>,
        src_area: Rect,
        dst: Handle<super::Image>,
        dst_area: Rect,
        resample_mode: Resample,
    ) -> Result<(), Error> {
        todo!()
    }

    fn destroy_image(&self, handle: Handle<super::Image>) -> Result<(), Error> {
        let mut images = self.images.borrow_mut();
        // If is_idle() returns an error, remove the texture anyway.
        let texture = images.remove_if(handle, |t| t.is_idle(self).unwrap_or(true))?;
        if let Some(texture) = texture {
            texture.destroy(self);
            Ok(())
        } else {
            Err(Error::ResourceInUse)
        }
    }

    fn get_image_pixels(&self, handle: Handle<super::Image>) -> Result<PixelBuffer, Error> {
        todo!()
    }

    fn destroy_render_target(&self, handle: Handle<super::RenderTarget>) -> Result<(), Error> {
        let mut targets = self.render_targets.borrow_mut();
        let render_target = targets.remove(handle)?;

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
        let render_target = targets.get(handle)?;

        match render_target {
            RenderTarget::Swapchain(window_handle, frame_id) => {
                let mut windows = self.windows.borrow_mut();

                // Check if the window still exists.
                if let Ok(window) = windows.get_mut(*window_handle) {
                    // Check if the render target is pointing to the current image
                    if *frame_id == window.frame_id() {
                        // The only way to get a swapchain image is to get a
                        // handle to it, and we've checked that the handle was
                        // acquired for the current frame.
                        let (frame, extent, sync, shader) = window.render_state();

                        frame.reset(self)?;
                        frame
                            .geometry
                            .copy(self, &commands.vertices, &commands.indices)?;

                        let used_textures = shader.apply(
                            self,
                            commands,
                            frame.framebuffer,
                            extent,
                            &frame.geometry,
                            frame.command_buffer,
                        )?;

                        let mut wait_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut wait_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut signal_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut signal_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();

                        for texture in used_textures {
                            let mut images = self.images.borrow_mut();
                            let texture = images.get_mut(texture).unwrap();

                            // Make sure that the texture is not being written
                            // to when we start using it (semaphore == count).
                            wait_semaphores.push(texture.write_semaphore);
                            wait_values.push(texture.write_count);

                            // Increment the read semaphore when drawing is
                            // complete, and increment the check to match. Reads
                            // will be complete when semaphore == count.
                            signal_semaphores.push(texture.read_semaphore);
                            texture.read_count += 1;
                            signal_values.push(texture.read_count);
                        }

                        let mut timeline_info = vk::TimelineSemaphoreSubmitInfo {
                            wait_semaphore_value_count: wait_semaphores.len() as u32,
                            p_wait_semaphore_values: wait_values.as_ptr(),
                            signal_semaphore_value_count: signal_semaphores.len() as u32,
                            p_signal_semaphore_values: signal_values.as_ptr(),
                            ..Default::default()
                        };

                        unsafe {
                            self.device.queue_submit(
                                self.graphics_queue,
                                &[vk::SubmitInfo::builder()
                                    .push_next(&mut timeline_info)
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

    fn flush(&self) {
        unsafe { self.device.device_wait_idle() }.unwrap();
    }
}

/// Literally, a thing that can be rendered to.
enum RenderTarget {
    Swapchain(Handle<super::Swapchain>, u64),
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
                item_set.insert(&name[0..=i]);
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

/// Copied from unstable std while waiting for #![`feature(int_roundigs)`] to
/// stabilize.
///
/// <https://github.com/rust-lang/rust/issues/88581>
pub(self) const fn next_multiple_of(lhs: vk::DeviceSize, rhs: vk::DeviceSize) -> vk::DeviceSize {
    match lhs % rhs {
        0 => lhs,
        r => lhs + (rhs - r),
    }
}

impl From<crate::handle_pool::Error> for Error {
    fn from(e: crate::handle_pool::Error) -> Self {
        match e {
            crate::handle_pool::Error::InvalidHandle => Self::InvalidHandle,
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
                x: i32::from(r.left.0),
                y: i32::from(r.top.0),
            },
            extent: vk::Extent2D {
                width: (r.right - r.left).0 as u32,
                height: (r.bottom - r.top).0 as u32,
            },
        }
    }
}
