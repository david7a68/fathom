use std::{mem::MaybeUninit, ptr::NonNull};

use ash::vk;

use crate::gfx::{
    canvas::{ImageHandle, Paint},
    geometry::{Extent, Px, Rect},
    pixel_buffer::PixelBuffer,
};

use super::{
    memory::{Allocation, Memory, MemoryLocation},
    DeferredDestroy, Device, Error, SwapchainHandle, Vertex,
};

pub struct Canvas {
    pub(super) extent: vk::Extent2D,
    pub(super) swapchain: SwapchainHandle,

    pub(super) frame_buffer: vk::Framebuffer,
    pub(super) vertex_buffer: vk::Buffer,
    pub(super) vertex_memory: Allocation,
    vertices: MappedBuffer<Vertex>,

    pub(super) index_buffer: vk::Buffer,
    pub(super) index_memory: Allocation,
    indices: MappedBuffer<u16>,
}

impl Canvas {
    pub(super) fn new(
        device: &mut Device,
        extent: vk::Extent2D,
        swapchain: SwapchainHandle,
        frame_buffer: vk::Framebuffer,
    ) -> Result<Self, Error> {
        let buffer_ci = vk::BufferCreateInfo {
            size: Memory::PAGE_SIZE,
            usage: vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let vertex_buffer = unsafe { device.device.create_buffer(&buffer_ci, None) }?;
        let vertex_memory = device.memory.allocate_buffer(
            &device.device,
            vertex_buffer,
            MemoryLocation::CpuToGpu,
            true,
        )?;
        let vertices = MappedBuffer::new(device.memory.map(&device.device, &vertex_memory)?);

        let index_buffer = unsafe { device.device.create_buffer(&buffer_ci, None) }?;
        let index_memory = device.memory.allocate_buffer(
            &device.device,
            index_buffer,
            MemoryLocation::CpuToGpu,
            true,
        )?;
        let indices = MappedBuffer::new(device.memory.map(&device.device, &index_memory)?);

        Ok(Self {
            extent,
            swapchain,
            frame_buffer,
            vertex_buffer,
            vertex_memory,
            vertices,
            index_buffer,
            index_memory,
            indices,
        })
    }

    pub(super) fn finish(
        self,
        device: &mut Device,
        queue: &mut DeferredDestroy,
    ) -> Result<(), Error> {
        device.memory.unmap(&device.device, &self.vertex_memory)?;
        device.memory.unmap(&device.device, &self.index_memory)?;

        queue.buffers.push(self.vertex_buffer);
        queue.allocations.push(self.vertex_memory);
        queue.buffers.push(self.index_buffer);
        queue.allocations.push(self.index_memory);

        Ok(())
    }

    pub fn num_indices(&self) -> usize {
        self.indices.len()
    }
}

impl crate::gfx::canvas::Canvas for Canvas {
    fn extent(&self) -> Extent {
        Extent {
            width: Px(self.extent.width.try_into().unwrap()),
            height: Px(self.extent.height.try_into().unwrap()),
        }
    }

    fn create_image(&mut self, pixels: &PixelBuffer) -> ImageHandle {
        todo!()
    }

    fn destroy_image(&mut self, image: ImageHandle) {
        todo!()
    }

    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        match paint {
            Paint::Fill { color } => {
                let offset = self.vertices.len() as u16;

                self.vertices.push(Vertex {
                    point: rect.top_left(),
                    color: *color,
                });
                self.vertices.push(Vertex {
                    point: rect.top_right(),
                    color: *color,
                });
                self.vertices.push(Vertex {
                    point: rect.bottom_right(),
                    color: *color,
                });
                self.vertices.push(Vertex {
                    point: rect.bottom_left(),
                    color: *color,
                });

                self.indices.extend_from_slice(&[
                    offset,
                    offset + 1,
                    offset + 2,
                    offset + 2,
                    offset + 3,
                    offset,
                ]);
            }
            Paint::Texture {
                handle,
                mode_x,
                mode_y,
                crop,
            } => {
                todo!()
            }
        }
    }
}

struct MappedBuffer<T> {
    ptr: NonNull<[MaybeUninit<T>]>,
    length: usize,
}

impl<T> MappedBuffer<T> {
    fn new(ptr: NonNull<[MaybeUninit<T>]>) -> Self {
        Self { ptr, length: 0 }
    }

    fn len(&self) -> usize {
        self.length
    }

    fn push(&mut self, value: T) {
        assert!(self.length < unsafe { self.ptr.as_ref() }.len());
        unsafe { self.ptr.as_mut()[self.length] = MaybeUninit::new(value) };
        self.length += 1;
    }
}

impl<T: Copy> MappedBuffer<T> {
    fn extend_from_slice(&mut self, slice: &[T]) {
        assert!(self.length + slice.len() < unsafe { self.ptr.as_ref() }.len());
        unsafe {
            self.ptr.as_mut()[self.length..self.length + slice.len()]
                .clone_from_slice(std::mem::transmute(slice));
        }

        self.length += slice.len();
    }
}
