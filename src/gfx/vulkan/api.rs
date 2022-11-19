use std::ffi::c_char;

use ash::vk;
use smallvec::SmallVec;

use crate::gfx::Error;

pub type VkResult<T> = Result<T, vk::Result>;

#[derive(Debug)]
pub struct PhysicalDevice {
    pub handle: vk::PhysicalDevice,
    pub properties: vk::PhysicalDeviceProperties,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub graphics_queue_family: u32,
    pub transfer_queue_family: u32,
    pub present_queue_family: u32,
}

#[derive(Clone, Copy)]
pub enum MemoryUsage {
    /// The allocated memory will be used once and then freed.
    Once,
    /// The allocated memory will be written to frequently by the CPU and read
    /// frequently by the GPU.
    Dynamic,
    /// The allocated memory will not be updated frequently by the CPU.
    Static,
}

pub struct Vulkan {
    #[allow(unused)]
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub device: ash::Device,

    pub physical_device: PhysicalDevice,

    pub pipeline_cache: vk::PipelineCache,
    pub graphics_queue: vk::Queue,
    pub transfer_queue: vk::Queue,
    pub present_queue: vk::Queue,

    pub surface_khr: ash::extensions::khr::Surface,
    pub swapchain_khr: ash::extensions::khr::Swapchain,

    #[cfg(target_os = "windows")]
    pub win32_surface_khr: ash::extensions::khr::Win32Surface,
}

impl Vulkan {
    #[allow(clippy::too_many_lines)]
    pub fn new(
        required_instance_layers: &[&[c_char]],
        optional_instance_layers: &[&[c_char]],
        required_instance_extensions: &[&[c_char]],
        optional_instance_extensions: &[&[c_char]],
        required_device_extensions: &[&[c_char]],
        optional_device_extensions: &[&[c_char]],
    ) -> Result<Self, Error> {
        let entry = unsafe { ash::Entry::load() }
            .map_err(|_| Error::BackendNotFound)
            .unwrap();

        let instance = {
            let instance_layers = has_names(
                &entry.enumerate_instance_layer_properties()?,
                |layer| &layer.layer_name,
                required_instance_layers,
                optional_instance_layers,
            )
            .ok_or(Error::VulkanInternal {
                error_code: vk::Result::ERROR_INITIALIZATION_FAILED,
            })?;

            let instance_extensions = has_names(
                &entry.enumerate_instance_extension_properties(None)?,
                |extension| &extension.extension_name,
                required_instance_extensions,
                optional_instance_extensions,
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

        let (gpu, device_extensions) = select_gpu(
            &instance,
            required_device_extensions,
            optional_device_extensions,
            |gpu, queue| unsafe {
                #[cfg(target_os = "windows")]
                win32_surface_khr.get_physical_device_win32_presentation_support(gpu, queue)
            },
        )?;

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
        })
    }

    pub fn allocate_buffer(
        &self,
        usage: MemoryUsage,
        size: vk::DeviceSize,
        flags: vk::BufferUsageFlags,
    ) -> VkResult<(vk::Buffer, vk::DeviceMemory)> {
        let buffer_create_info = vk::BufferCreateInfo {
            size,
            usage: flags,
            ..Default::default()
        };

        let buffer = unsafe { self.device.create_buffer(&buffer_create_info, None) }?;
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        match self.allocate_memory(usage, requirements) {
            Ok(memory) => match unsafe { self.device.bind_buffer_memory(buffer, memory, 0) } {
                Ok(_) => Ok((buffer, memory)),
                Err(e) => {
                    unsafe {
                        self.device.destroy_buffer(buffer, None);
                        self.device.free_memory(memory, None);
                    }
                    Err(e)
                }
            },
            Err(e) => {
                unsafe { self.device.destroy_buffer(buffer, None) };
                Err(e)
            }
        }
    }

    pub fn allocate_memory(
        &self,
        usage: MemoryUsage,
        requirements: vk::MemoryRequirements,
    ) -> VkResult<vk::DeviceMemory> {
        // Use optimal and backup because Vulkan spec guarantees that a memory
        // type offering a subset of another memory type's flags must go first.
        let (optimal, backup) = match usage {
            MemoryUsage::Once | MemoryUsage::Dynamic => (
                vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE,
                vk::MemoryPropertyFlags::HOST_VISIBLE,
            ),
            MemoryUsage::Static => (
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::MemoryPropertyFlags::empty(),
            ),
        };

        let memory_types = &self.physical_device.memory_properties.memory_types;
        let memory_type_count = self.physical_device.memory_properties.memory_type_count;
        let selection = (0..memory_type_count as usize)
            .find(|i| {
                let type_ok = requirements.memory_type_bits & (1 << i) != 0;
                type_ok & memory_types[*i].property_flags.contains(optimal)
            })
            .or_else(|| {
                (0..memory_type_count as usize).find(|i| {
                    let type_ok = requirements.memory_type_bits & (1 << i) != 0;
                    type_ok & memory_types[*i].property_flags.contains(backup)
                })
            })
            .unwrap();

        let create_info = vk::MemoryAllocateInfo {
            allocation_size: requirements.size,
            memory_type_index: selection as u32,
            ..Default::default()
        };

        unsafe { self.device.allocate_memory(&create_info, None) }
    }

    pub fn create_image_view(
        &self,
        image: vk::Image,
        format: vk::Format,
    ) -> VkResult<vk::ImageView> {
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
                std::ptr::addr_of!(timeline_info).cast()
            } else {
                std::ptr::null()
            },
            ..Default::default()
        };

        unsafe { self.device.create_semaphore(&create_info, None) }
    }

    pub fn allocate_command_buffer(&self, pool: vk::CommandPool) -> VkResult<vk::CommandBuffer> {
        let create_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let mut command_buffer = vk::CommandBuffer::null();
        unsafe {
            (self.device.fp_v1_0().allocate_command_buffers)(
                self.device.handle(),
                &create_info.build(),
                &mut command_buffer,
            )
        }
        .result_with_success(command_buffer)
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
    required_device_extensions: &[&[c_char]],
    optional_device_extensions: &[&[c_char]],
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
                required_device_extensions,
                optional_device_extensions,
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
pub(super) const fn next_multiple_of(lhs: vk::DeviceSize, rhs: vk::DeviceSize) -> vk::DeviceSize {
    match lhs % rhs {
        0 => lhs,
        r => lhs + (rhs - r),
    }
}
