//! NOTE(straivers): This implementation is intentionally naive and suffers
//! horrible internal fragmentation. However, it does its job well enough for
//! the moment. A proper memory allocator will have to be written at some point,
//! but that point is not today.

use std::{mem::MaybeUninit, ptr::NonNull};

use ash::vk;

use super::Error;

const PAGE_SIZE: vk::DeviceSize = 4 * 1024 * 1024;
const HOST_BLOCK_SIZE: vk::DeviceSize = 32 * 1024 * 1024;
const DEVICE_BLOCK_SIZE: vk::DeviceSize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum MemoryLocation {
    #[default]
    Unknown,
    Staging,
    GpuOnly,
    CpuToGpu,
}

#[derive(Default)]
pub struct Allocation {
    memory: vk::DeviceMemory,
    offset: vk::DeviceSize,
    size: vk::DeviceSize,

    location: MemoryLocation,
    type_index: u8,
    block_index: u8,
    page_index: u8,
}

pub struct Memory {
    memory_types: Vec<MemoryType>,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
}

impl Memory {
    pub(super) const PAGE_SIZE: vk::DeviceSize = PAGE_SIZE;

    pub(super) fn new(memory_properties: vk::PhysicalDeviceMemoryProperties) -> Self {
        let mut memory_types = vec![];
        for type_index in 0..memory_properties.memory_type_count {
            memory_types.push(MemoryType::new(
                type_index,
                memory_properties.memory_types[type_index as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL),
            ))
        }

        Self {
            memory_types,
            memory_properties,
        }
    }

    pub(super) fn destroy(&mut self, device: &ash::Device) {
        for mut memory_type in self.memory_types.drain(..) {
            memory_type.destroy(device);
        }
    }

    pub(super) fn map<T>(
        &self,
        device: &ash::Device,
        allocation: &Allocation,
    ) -> Result<NonNull<[MaybeUninit<T>]>, Error> {
        if allocation.location == MemoryLocation::GpuOnly {
            todo!()
        } else {
            self.memory_types[allocation.type_index as usize].map(
                device,
                &HeapAllocation {
                    memory: allocation.memory,
                    offset: allocation.offset,
                    block_index: allocation.block_index,
                    page_index: allocation.page_index,
                },
            )
        }
    }

    pub(super) fn unmap(&self, device: &ash::Device, allocation: &Allocation) -> Result<(), Error> {
        self.memory_types[allocation.type_index as usize].unmap(
            device,
            &HeapAllocation {
                memory: allocation.memory,
                offset: allocation.offset,
                block_index: allocation.block_index,
                page_index: allocation.page_index,
            },
        )
    }

    pub(super) fn allocate_buffer(
        &mut self,
        device: &ash::Device,
        buffer: vk::Buffer,
        location: MemoryLocation,
        bind_immediately: bool,
    ) -> Result<Allocation, Error> {
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = self.allocate(device, requirements, location);

        if let Ok(allocation) = &allocation {
            if bind_immediately {
                unsafe { device.bind_buffer_memory(buffer, allocation.memory, allocation.offset) }?;
            }
        }

        allocation
    }

    pub(super) fn allocate_image(
        &mut self,
        device: &ash::Device,
        image: vk::Image,
        location: MemoryLocation,
        bind_immediately: bool,
    ) -> Result<Allocation, Error> {
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = self.allocate(device, requirements, location);

        if let Ok(allocation) = &allocation {
            if bind_immediately {
                unsafe { device.bind_image_memory(image, allocation.memory, allocation.offset) }?;
            }
        }

        allocation
    }

    pub(super) fn allocate(
        &mut self,
        device: &ash::Device,
        requirements: vk::MemoryRequirements,
        location: MemoryLocation,
    ) -> Result<Allocation, Error> {
        let required_properties = match location {
            MemoryLocation::Staging => {
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
            }
            MemoryLocation::GpuOnly => vk::MemoryPropertyFlags::DEVICE_LOCAL,
            MemoryLocation::CpuToGpu => {
                vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT
                    | vk::MemoryPropertyFlags::DEVICE_LOCAL
            }
            MemoryLocation::Unknown => {
                panic!("a memory allocation must have a designated location")
            }
        };

        let type_index = self
            .find_memory_type(requirements.memory_type_bits, required_properties)
            .ok_or(Error::NoSuitableMemoryType(requirements, location))?;

        let allocation = self.memory_types[type_index as usize].allocate(device)?;

        Ok(Allocation {
            memory: allocation.memory,
            offset: allocation.offset,
            size: requirements.size,
            location,
            type_index: type_index.try_into().unwrap(),
            block_index: allocation.block_index,
            page_index: allocation.page_index,
        })
    }

    pub(super) fn deallocate(&mut self, allocation: Allocation) {
        self.memory_types[allocation.type_index as usize].deallocate(HeapAllocation {
            memory: allocation.memory,
            offset: allocation.offset,
            block_index: allocation.block_index,
            page_index: allocation.page_index,
        });
    }

    fn find_memory_type(
        &self,
        type_bits: u32,
        required_properties: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        for i in 0..self.memory_properties.memory_type_count {
            if (type_bits & (1 << i)) != 0
                && self.memory_properties.memory_types[i as usize]
                    .property_flags
                    .contains(required_properties)
            {
                return Some(i);
            }
        }

        None
    }
}

struct HeapAllocation {
    memory: vk::DeviceMemory,
    offset: vk::DeviceSize,
    block_index: u8,
    page_index: u8,
}

struct MemoryType {
    index: u32,
    is_device_local: bool,
    blocks: Vec<MemoryBlock>,
    available_block_indices: Vec<usize>,
}

impl MemoryType {
    fn new(index: u32, is_device_local: bool) -> Self {
        Self {
            index,
            is_device_local,
            blocks: vec![],
            available_block_indices: vec![],
        }
    }

    fn destroy(&mut self, device: &ash::Device) {
        for mut block in self.blocks.drain(..) {
            block.destroy(device);
        }
    }

    fn map<T>(
        &self,
        device: &ash::Device,
        allocation: &HeapAllocation,
    ) -> Result<NonNull<[MaybeUninit<T>]>, Error> {
        self.blocks[allocation.block_index as usize].map(
            device,
            Page {
                offset: allocation.offset,
                index: allocation.page_index,
            },
        )
    }

    fn unmap(&self, device: &ash::Device, allocation: &HeapAllocation) -> Result<(), Error> {
        self.blocks[allocation.block_index as usize].unmap(
            device,
            Page {
                offset: allocation.offset,
                index: allocation.page_index,
            },
        )
    }

    fn allocate(&mut self, device: &ash::Device) -> Result<HeapAllocation, Error> {
        if self.available_block_indices.is_empty() {
            let block_size = if self.is_device_local {
                DEVICE_BLOCK_SIZE
            } else {
                HOST_BLOCK_SIZE
            };

            let block_index = self.blocks.len();
            self.blocks
                .push(MemoryBlock::new(device, self.index, block_size)?);
            self.available_block_indices.push(block_index);
        }

        let index = *self.available_block_indices.last().unwrap();
        let block = &mut self.blocks[index];
        let page = block.allocate()?;

        if block.is_full() {
            self.available_block_indices.pop();
        }

        Ok(HeapAllocation {
            memory: block.memory,
            offset: page.offset,
            block_index: index.try_into().unwrap(),
            page_index: page.index,
        })
    }

    fn deallocate(&mut self, allocation: HeapAllocation) {
        let block = &mut self.blocks[allocation.block_index as usize];

        let was_full = block.is_full();

        block.deallocate(Page {
            offset: allocation.offset,
            index: allocation.page_index,
        });

        if was_full {
            self.available_block_indices
                .push(allocation.block_index as usize);
        }
    }
}

struct Page {
    offset: vk::DeviceSize,
    index: u8,
}

pub struct MemoryBlock {
    memory: vk::DeviceMemory,
    bitmap: u64,
}

impl MemoryBlock {
    fn new(device: &ash::Device, heap_index: u32, size: vk::DeviceSize) -> Result<Self, Error> {
        let alloc_info = vk::MemoryAllocateInfo {
            allocation_size: size,
            memory_type_index: heap_index,
            ..Default::default()
        };

        let memory =
            unsafe { device.allocate_memory(&alloc_info, None) }.map_err(Error::OutOfMemory)?;

        let bitmap = u64::MAX >> (u64::BITS as vk::DeviceSize - (size / PAGE_SIZE));

        Ok(Self { memory, bitmap })
    }

    fn destroy(&mut self, device: &ash::Device) {
        unsafe { device.free_memory(std::mem::take(&mut self.memory), None) };
    }

    fn is_full(&self) -> bool {
        self.bitmap == 0
    }

    /// Maps GPU memory to the program's address space.
    fn map<T>(
        &self,
        device: &ash::Device,
        allocation: Page,
    ) -> Result<NonNull<[MaybeUninit<T>]>, Error> {
        // TODO(straivers): Could implement additional runtime safety checks
        // here such as ensuring that no two mappings overlap.
        //
        // TODO(straivers): How can we make sure that the mapped pointer never
        // outlives an unmap operation?

        let mapped_ptr = unsafe {
            device.map_memory(
                self.memory,
                allocation.offset,
                PAGE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?
        }
        .cast();

        let slice_length = PAGE_SIZE as usize / std::mem::size_of::<T>();

        // SAFETY: This is safe because Vulkan will never return a null
        // pointer instead of returning an error in VkResult.
        Ok(unsafe {
            NonNull::new_unchecked(std::slice::from_raw_parts_mut(mapped_ptr, slice_length))
        })
    }

    fn unmap(&self, device: &ash::Device, _allocation: Page) -> Result<(), Error> {
        unsafe { device.unmap_memory(self.memory) };
        Ok(())
    }

    fn allocate(&mut self) -> Result<Page, Error> {
        if self.bitmap == 0 {
            Err(Error::OutOfMemory(vk::Result::ERROR_UNKNOWN))
        } else {
            // Subtract 1 since we're 0-indexing
            let index = (u64::BITS - self.bitmap.leading_zeros()) - 1;
            println!("{index}");
            self.bitmap &= !(1 << index);
            Ok(Page {
                offset: index as vk::DeviceSize * PAGE_SIZE,
                index: index.try_into().unwrap(),
            })
        }
    }

    fn deallocate(&mut self, page: Page) {
        assert_eq!(self.bitmap & (1 << page.index), 0);
        self.bitmap |= 1 << page.index;
    }
}
