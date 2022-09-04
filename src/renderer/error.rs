use ash::vk;

use super::memory::MemoryLocation;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a compatible Vulkan driver was not found")]
    NoVulkanLibrary,
    #[error("no suitable GPU was found")]
    NoSuitableGpu,
    #[error("the swapchain cannot be used because it is out of date")]
    SwapchainOutOfDate,
    #[error("{0}")]
    Vulkan(#[from] vk::Result),
    #[error("too many objects")]
    TooManyObjects,
    #[error("per-draw index buffer exceeds 2^16 indices")]
    IndexBufferTooLarge,
    #[error("the requested allocation failed")]
    OutOfMemory(vk::Result),
    #[error("no suitable memory type for the requested allocation could be found")]
    NoSuitableMemoryType(vk::MemoryRequirements, MemoryLocation),
}
