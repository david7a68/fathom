use ash::vk;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a compatible Vulkan driver was not found")]
    NoVulkanLibrary,
    #[error("no suitable GPU was found")]
    NoSuitableGpu,

    #[error("{0}")]
    Vulkan(#[from] vk::Result),
}
