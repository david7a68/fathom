//! # Textures and their Storage
//!
//! ## Assumptions/Requirements
//!
//! - all textures stored in RGBA_F16_LINEAR format for simplicity
//! - must accept all formats used by PixelBuffer (and convert)
//! - must permit updating of subtextures in bulk

use ash::vk;

use crate::gfx::geometry::Extent;

use super::{VkResult, Vulkan};

const STORAGE_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

pub struct Texture {
    image: vk::Image,
    image_view: vk::ImageView,
    memory: vk::DeviceMemory,
    /// A timeline semaphore used to track write operations. If
    /// `write_semaphore==write_count`, the texture is not currently being
    /// written to and can be used for reading.
    pub write_semaphore: vk::Semaphore,
    /// A count of the number of write operations executed on this texture.
    pub write_count: u64,
    /// A timeline semaphore used to track read operations. If
    /// `read_semaphore==read_count`, the texture is not currently being read
    /// and can be used for write operations.
    pub read_semaphore: vk::Semaphore,
    /// A count of the number of read operations executed on this texture.
    pub read_count: u64,
}

impl Texture {
    pub fn new(api: &Vulkan, extent: Extent) -> VkResult<Self> {
        let image = {
            let create_info = vk::ImageCreateInfo {
                flags: vk::ImageCreateFlags::empty(),
                image_type: vk::ImageType::TYPE_2D,
                format: STORAGE_FORMAT,
                extent: vk::Extent3D {
                    width: extent.width.0 as u32,
                    height: extent.height.0 as u32,
                    depth: 1,
                },
                mip_levels: 1,
                array_layers: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                tiling: vk::ImageTiling::OPTIMAL,
                usage: vk::ImageUsageFlags::SAMPLED,
                initial_layout: vk::ImageLayout::UNDEFINED,
                ..Default::default()
            };

            unsafe { api.device.create_image(&create_info, None) }?
        };

        let memory = {
            let requirements = unsafe { api.device.get_image_memory_requirements(image) };
            let properties = vk::MemoryPropertyFlags::DEVICE_LOCAL;

            let type_index = Vulkan::find_memory_type(
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

        unsafe { api.device.bind_image_memory(image, memory, 0) }?;

        let image_view = api.create_image_view(image, STORAGE_FORMAT)?;

        let write_semaphore = api.create_semaphore(true)?;
        let read_semaphore = api.create_semaphore(true)?;

        Ok(Self {
            image,
            image_view,
            memory,
            write_semaphore,
            write_count: 0,
            read_semaphore,
            read_count: 0,
        })
    }

    pub fn is_idle(&self, api: &Vulkan) -> VkResult<bool> {
        let write_count = unsafe { api.device.get_semaphore_counter_value(self.write_semaphore) }?;
        let read_count = unsafe { api.device.get_semaphore_counter_value(self.read_semaphore) }?;
        Ok(write_count == self.write_count && read_count == self.read_count)
    }

    pub fn destroy(self, api: &Vulkan) {
        assert!(
            self.is_idle(api).unwrap(),
            "must not destory an image that is in use"
        );
        unsafe {
            api.device.destroy_image_view(self.image_view, None);
            api.device.destroy_image(self.image, None);
            api.device.free_memory(self.memory, None);

            api.device.destroy_semaphore(self.write_semaphore, None);
            api.device.destroy_semaphore(self.read_semaphore, None);
        }
    }
}
