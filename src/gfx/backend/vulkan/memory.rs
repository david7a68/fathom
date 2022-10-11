use std::{ffi::c_void, ptr::NonNull, collections::VecDeque};

use ash::vk;

use crate::{
    gfx::backend::{Buffer, Error, MappedBuffer},
    handle_pool::HandlePool,
};

// 64k
const BUFFER_BLOCK_SIZE: vk::DeviceSize = 64 * 1024;
// 4986k (4m)
const SLAB_ALLOCATION_SIZE: vk::DeviceSize = (u64::BITS as vk::DeviceSize) * BUFFER_BLOCK_SIZE;

// We keep slabs in 128k, 256k, 512k, 1m
// We keep images in 1024x1024, 2048x2048, 4096x4096
pub struct Memory {
    buffer_slabs: Vec<Slab>,
    buffers: HandlePool<BufferAlloc, Buffer, 1024>,
}

impl Memory {
    pub fn new() -> Result<Self, Error> {
        todo!()
    }

    pub(in crate::gfx::backend) fn allocate_buffer<T>(
        &mut self,
        min_count: vk::DeviceSize,
    ) -> Result<MappedBuffer<T>, Error> {
        // find a slab that isn't full yet
        // find the first free block in that slab
        // mark that block as allocated
        // return it
        todo!()
    }

    pub(in crate::gfx::backend) fn free_buffer<T>(&mut self, buffer: MappedBuffer<T>) {
        todo!()
    }
}

struct BufferAlloc {
    slab_index: usize,
    block_index: usize,
}

struct Slab {
    bitmap: u64,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped_ptr: NonNull<c_void>,
}
