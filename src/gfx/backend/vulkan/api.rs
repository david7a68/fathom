use std::ffi::c_char;

use ash::vk::{self, PresentFrameTokenGGP};
use smallvec::SmallVec;

use crate::gfx::backend::Error;

const fn as_cchar_slice(slice: &[u8]) -> &[c_char] {
    unsafe { std::mem::transmute(slice) }
}

const VALIDATION_LAYER: &[c_char] = as_cchar_slice(b"VK_LAYER_KHRONOS_VALIDATION\0");

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

impl From<vk::Result> for Error {
    fn from(vkr: vk::Result) -> Self {
        Self::VulkanInternal { error_code: vkr }
    }
}

pub struct VulkanApi {
    #[allow(dead_code)]
    entry: ash::Entry,
    pub instance: ash::Instance,

    pub device: ash::Device,
    pub physical_device: vk::PhysicalDevice,

    pub graphics_queue_family: u32,
    pub transfer_queue_family: u32,
    pub present_queue_family: u32,
    pub graphics_queue: vk::Queue,
    pub transfer_queue: vk::Queue,
    pub present_queue: vk::Queue,

    pub surface_khr: ash::extensions::khr::Surface,
    pub swapchain_khr: ash::extensions::khr::Swapchain,

    #[cfg(target_os = "windows")]
    pub os_surface_khr: ash::extensions::khr::Win32Surface,
}

impl VulkanApi {
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
                    &[],
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
        let os_surface_khr = ash::extensions::khr::Win32Surface::new(&entry, &instance);

        let (
            physical_device,
            graphics_queue_family,
            transfer_queue_family,
            present_queue_family,
            device_extensions,
        ) = {
            let mut physical_devices = unsafe { instance.enumerate_physical_devices() }?;

            loop {
                let gpu = physical_devices.pop().ok_or(Error::NoGraphicsDevice)?;
                let (mut graphics, mut transfer, mut present) = (None, None, None);

                let queue_families =
                    unsafe { instance.get_physical_device_queue_family_properties(gpu) };
                for (index, queue_family) in queue_families.iter().enumerate() {
                    let index = index.try_into().unwrap();

                    #[cfg(target_os = "windows")]
                    {
                        if unsafe {
                            os_surface_khr
                                .get_physical_device_win32_presentation_support(gpu, index)
                        } {
                            present = present.or(Some(index));
                        }
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
                        break (
                            gpu,
                            graphics,
                            transfer.unwrap_or(graphics),
                            present,
                            extensions,
                        );
                    }
                }
            }
        };

        let device = {
            let queue_priority = 1.0;
            let mut queues = SmallVec::<[vk::DeviceQueueCreateInfo; 3]>::new();

            queues.push(vk::DeviceQueueCreateInfo {
                queue_family_index: graphics_queue_family,
                queue_count: 1,
                p_queue_priorities: &queue_priority,
                ..Default::default()
            });

            if graphics_queue_family != transfer_queue_family {
                queues.push(vk::DeviceQueueCreateInfo {
                    queue_family_index: transfer_queue_family,
                    queue_count: 1,
                    p_queue_priorities: &queue_priority,
                    ..Default::default()
                });
            }

            if graphics_queue_family != present_queue_family {
                queues.push(vk::DeviceQueueCreateInfo {
                    queue_family_index: transfer_queue_family,
                    queue_count: 1,
                    p_queue_priorities: &queue_priority,
                    ..Default::default()
                });
            }

            let create_info = vk::DeviceCreateInfo {
                queue_create_info_count: queues.len() as u32,
                p_queue_create_infos: queues.as_ptr(),
                enabled_extension_count: device_extensions.len() as u32,
                pp_enabled_extension_names: device_extensions.as_ptr(),
                ..Default::default()
            };

            unsafe { instance.create_device(physical_device, &create_info, None) }?
        };

        let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family, 0) };
        let transfer_queue = unsafe { device.get_device_queue(transfer_queue_family, 0) };
        let present_queue = unsafe { device.get_device_queue(present_queue_family, 0) };

        let swapchain_khr = ash::extensions::khr::Swapchain::new(&instance, &device);

        Ok(Self {
            entry,
            instance,
            device,
            physical_device,
            graphics_queue_family,
            transfer_queue_family,
            present_queue_family,
            graphics_queue,
            transfer_queue,
            present_queue,
            surface_khr,
            swapchain_khr,
            os_surface_khr,
        })
    }

    pub fn create_semaphore(&self) -> Result<vk::Semaphore, Error> {
        let create_info = vk::SemaphoreCreateInfo::default();
        Ok(unsafe { self.device.create_semaphore(&create_info, None) }?)
    }

    pub fn create_fence(&self, signalled: bool) -> Result<vk::Fence, Error> {
        let mut create_info = vk::FenceCreateInfo::default();
        if signalled {
            create_info.flags |= vk::FenceCreateFlags::SIGNALED;
        }
        Ok(unsafe { self.device.create_fence(&create_info, None) }?)
    }
}

impl Drop for VulkanApi {
    fn drop(&mut self) {
        todo!()
    }
}

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
