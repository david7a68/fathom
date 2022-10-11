//! Memory management utilities.
//!
//! The memory allocator implemented here is intended to fulfill a few usecases:
//!
//! - Geometry buffers (vertex, index, uniform)
//!   - Written by the CPU
//!   - Copied to the GPU for rendering
//!   - Reset for future use
//! - Texture Atlases
//!   - 2-phase texture uploads
//!     - Pixel buffer gets copied into CPU-side atlas (linear)
//!     - Updated region gets copied to GPU-side atlas (optimal)
//!     - Regions may differ between CPU-side and GPU-side
//!   - Small images more likely than large images

use std::{ffi::c_void, ptr::NonNull};

use ash::vk;
use smallvec::SmallVec;

use crate::{
    gfx::backend::{Buffer, Error, MappedBuffer},
    handle_pool::{Handle, HandlePool},
};

use super::api::VulkanApi;

// 64k
const BUFFER_BLOCK_SIZE: vk::DeviceSize = 64 * 1024;
// 4986k (4m)
const SLAB_ALLOCATION_SIZE: vk::DeviceSize = (u64::BITS as vk::DeviceSize) * BUFFER_BLOCK_SIZE;
// 1024 * 64k = 64m
const MAX_BUFFERS: usize = 1024;

// We keep slabs in 128k, 256k, 512k, 1m
// We keep images in 1024x1024, 2048x2048, 4096x4096
pub struct VulkanMemory {
    // OPTIMIZE(straivers): To reduce memory usage, it may be worth trying to
    // allocate from fullest slabs first. We should see slow migration towards
    // fully slabs, possibly exposing slabs that could be deallocated entirely.
    // This would likely require that slabs be sorted in some way, with full
    // slabs taken out of the running entirely. However, this might not be worth
    // it if there are only a small number of slabs (as might be expected of a
    // GUI application). A partial implementation with a linear scan may be
    // worth it in the short term, though measurement would be needed.
    buffer_slabs: Vec<BufferSlab>,
    buffers: HandlePool<BufferAlloc, Buffer, { MAX_BUFFERS as u32 }>,
}

impl VulkanMemory {
    pub fn new() -> Self {
        Self {
            buffer_slabs: Vec::with_capacity(1),
            buffers: HandlePool::preallocate(),
        }
    }

    pub(in crate::gfx::backend) fn allocate_buffer<T>(
        &mut self,
        api: &VulkanApi,
    ) -> Result<MappedBuffer<T>, Error> {
        let mut found_slab = None;
        for (slab_index, slab) in self.buffer_slabs.iter_mut().enumerate() {
            if slab.blocks_free() > 0 {
                let (block_index, pointer) = slab.alloc_block();
                found_slab = Some((slab_index, block_index, pointer));
                break;
            }
        }

        let (slab_index, block_index, pointer) =
            if let Some((slab_index, block_index, pointer)) = found_slab {
                (slab_index, block_index, pointer)
            } else {
                let mut new_slab = BufferSlab::new(api)?;
                let (block_index, pointer) = new_slab.alloc_block();
                let slab_index = self.buffer_slabs.len();
                self.buffer_slabs.push(new_slab);
                (slab_index, block_index, pointer)
            };

        let capacity = BUFFER_BLOCK_SIZE as usize / std::mem::size_of::<T>();

        let handle = self.buffers.insert(BufferAlloc {
            slab_index,
            block_index,
        })?;

        Ok(MappedBuffer {
            handle,
            capacity: capacity as u32,
            pointer: pointer.cast(),
        })
    }

    pub(in crate::gfx::backend) fn flush_buffers(
        &mut self,
        handles: &[Handle<Buffer>],
        api: &VulkanApi,
    ) -> Result<(), Error> {
        // NOTE(straivers): Reserve memory on stack for 3x vertex buffer, 2x
        // index buffer, 1x uniform buffer. This is an imaginary usecase, but
        // seems reasonable enough.
        let mut ranges = SmallVec::<[vk::MappedMemoryRange; 6]>::new();
        for handle in handles {
            let alloc = self.buffers.get(*handle).ok_or(Error::InvalidHandle)?;
            let slab = &self.buffer_slabs[alloc.slab_index];
            ranges.push(vk::MappedMemoryRange {
                memory: slab.memory,
                offset: alloc.block_index as vk::DeviceSize * BUFFER_BLOCK_SIZE,
                size: BUFFER_BLOCK_SIZE,
                ..Default::default()
            });
        }

        unsafe {
            api.device.flush_mapped_memory_ranges(&ranges)?;
        }

        Ok(())
    }

    pub(in crate::gfx::backend) fn free_buffer<T>(
        &mut self,
        buffer: MappedBuffer<T>,
    ) -> Result<(), Error> {
        let alloc = self
            .buffers
            .remove(buffer.handle)
            .ok_or(Error::InvalidHandle)?;

        self.buffer_slabs[alloc.slab_index].free_block(alloc.block_index);

        Ok(())
    }
}

struct BufferAlloc {
    slab_index: usize,
    block_index: u32,
}

struct BufferSlab {
    bitmap: u64,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped_ptr: NonNull<c_void>,
}

impl BufferSlab {
    fn new(api: &VulkanApi) -> Result<Self, Error> {
        let buffer = {
            let create_info = vk::BufferCreateInfo {
                size: SLAB_ALLOCATION_SIZE,
                usage: vk::BufferUsageFlags::VERTEX_BUFFER
                    | vk::BufferUsageFlags::INDEX_BUFFER
                    | vk::BufferUsageFlags::UNIFORM_BUFFER,
                sharing_mode: vk::SharingMode::EXCLUSIVE,
                ..Default::default()
            };

            unsafe { api.device.create_buffer(&create_info, None) }?
        };

        let requirements = unsafe { api.device.get_buffer_memory_requirements(buffer) };

        let required_properties = vk::MemoryPropertyFlags::HOST_VISIBLE
            | vk::MemoryPropertyFlags::HOST_CACHED
            | vk::MemoryPropertyFlags::DEVICE_LOCAL;

        let type_index = api
            .find_memory_type(requirements.memory_type_bits, required_properties)
            .ok_or(Error::VulkanInternal {
                error_code: vk::Result::ERROR_UNKNOWN,
            })?;

        let memory = {
            let create_info = vk::MemoryAllocateInfo {
                allocation_size: SLAB_ALLOCATION_SIZE,
                memory_type_index: type_index,
                ..Default::default()
            };

            unsafe { api.device.allocate_memory(&create_info, None) }?
        };

        let mapped_ptr = unsafe {
            NonNull::new_unchecked(api.device.map_memory(
                memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?)
        };

        Ok(Self {
            bitmap: u64::MAX,
            buffer,
            memory,
            mapped_ptr,
        })
    }

    fn destroy(self, api: &VulkanApi) {
        unsafe {
            api.device.unmap_memory(self.memory);
            api.device.free_memory(self.memory, None);
        }
    }

    fn blocks_free(&self) -> u32 {
        self.bitmap.count_ones()
    }

    fn alloc_block(&mut self) -> (u32, NonNull<c_void>) {
        let block_index = u64::BITS - self.bitmap.leading_zeros();
        self.bitmap &= !(1 << block_index);

        let offset = block_index as vk::DeviceSize * BUFFER_BLOCK_SIZE;

        (block_index, unsafe {
            NonNull::new_unchecked(self.mapped_ptr.as_ptr().add(offset as usize))
        })
    }

    fn free_block(&mut self, block_index: u32) {
        self.bitmap |= 1 << block_index;
    }
}
