use ash::vk;

use crate::gfx::Vertex;

use super::api::{next_multiple_of, MemoryUsage, Vulkan};

/// Utility struct for a `VkBuffer` suitable for vertices and indices.
pub struct UiGeometryBuffer {
    pub handle: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
    // first_vertex is assumed to be 0
    pub index_offset: vk::DeviceSize,
}

impl UiGeometryBuffer {
    const NUM_INIT_VERTICES: vk::DeviceSize = 1024 * 4;
    const NUM_INIT_INDICES: vk::DeviceSize = 1024 * 6;

    /// Allocates a new buffer suitable for 1024 rects (4096 vertices and 6144
    /// indices).
    pub fn new(api: &Vulkan) -> Result<Self, vk::Result> {
        let index_offset = Self::index_offset(api, Self::NUM_INIT_VERTICES);
        let buffer_size = index_offset + Self::index_size(Self::NUM_INIT_INDICES);

        let (handle, memory) = api.allocate_buffer(
            MemoryUsage::Dynamic,
            buffer_size,
            vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
        )?;

        Ok(Self {
            handle,
            memory,
            size: buffer_size,
            index_offset,
        })
    }

    /// Destroys the buffer and frees its memory from the GPU.
    pub fn destroy(self, api: &Vulkan) {
        unsafe {
            api.device.destroy_buffer(self.handle, None);
            api.device.free_memory(self.memory, None);
        }
    }

    /// Copies the vertices and indices into the GPU buffer, resizing as needed
    /// to fit the data.
    ///
    /// This copy _does not_ shrink the buffer, however, as there is no real
    /// usecase for it yet.
    pub(super) fn copy(
        &mut self,
        api: &Vulkan,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> Result<(), vk::Result> {
        let index_offset = Self::index_offset(api, vertices.len() as vk::DeviceSize);
        let required_size = index_offset + Self::index_size(indices.len() as vk::DeviceSize);

        if required_size > self.size {
            unsafe {
                api.device.destroy_buffer(self.handle, None);
                api.device.free_memory(self.memory, None);
            }

            let (handle, memory) = api.allocate_buffer(
                MemoryUsage::Dynamic,
                required_size,
                vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
            )?;

            self.handle = handle;
            self.memory = memory;
        }

        // This may change even if the buffer size doesn't.
        self.index_offset = index_offset;

        unsafe {
            let ptr = api.device.map_memory(
                self.memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?;

            std::slice::from_raw_parts_mut(ptr.cast(), vertices.len()).copy_from_slice(vertices);

            std::slice::from_raw_parts_mut(ptr.add(index_offset as usize).cast(), indices.len())
                .copy_from_slice(indices);

            api.device.unmap_memory(self.memory);
        }

        Ok(())
    }

    /// Calculates the offset offset into a buffer with `n_vertices`.
    fn index_offset(api: &Vulkan, n_vertices: vk::DeviceSize) -> vk::DeviceSize {
        let vertex_bytes = std::mem::size_of::<Vertex>() as vk::DeviceSize * n_vertices;
        next_multiple_of(
            vertex_bytes,
            api.physical_device.properties.limits.non_coherent_atom_size,
        )
    }

    /// Calculates the size of the index buffer.
    fn index_size(n_indices: vk::DeviceSize) -> vk::DeviceSize {
        std::mem::size_of::<u16>() as vk::DeviceSize * n_indices
    }
}
