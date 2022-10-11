use std::cell::RefCell;

use ash::vk;
use smallvec::SmallVec;

use crate::{
    gfx::{
        geometry::Extent,
        pixel_buffer::{ColorSpace, Layout, PixelBuffer},
    },
    handle_pool::{Handle, HandlePool},
};

use super::{Backend, CommandStream, Error, Image, RenderTarget, Swapchain, MAX_SWAPCHAINS};

mod api;
mod memory;
mod swapchain;

use self::{api::VulkanApi, swapchain::VulkanSwapchain};

impl From<crate::handle_pool::Error> for Error {
    fn from(e: crate::handle_pool::Error) -> Self {
        match e {
            crate::handle_pool::Error::TooManyObjects {
                num_allocated: _,
                num_retired: _,
                capacity,
            }
            | crate::handle_pool::Error::Exhausted { capacity } => Self::TooManyObjects {
                limit: capacity as u32,
            },
        }
    }
}

impl From<Extent> for vk::Extent2D {
    fn from(e: Extent) -> Self {
        Self {
            width: e.width.0.try_into().unwrap(),
            height: e.height.0.try_into().unwrap(),
        }
    }
}

pub struct Vulkan {
    api: VulkanApi,

    swapchains: RefCell<HandlePool<VulkanSwapchain, Swapchain, { MAX_SWAPCHAINS }>>,
    render_targets: RefCell<HandlePool<VulkanRenderTarget, RenderTarget, 64>>,
}

enum VulkanRenderTarget {
    Swapchain { swapchain: Handle<Swapchain> },
}

impl Vulkan {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            api: VulkanApi::new(true)?,
            swapchains: RefCell::new(HandlePool::preallocate()),
            render_targets: RefCell::new(HandlePool::preallocate()),
        })
    }
}

impl Backend for Vulkan {
    #[cfg(target_os = "windows")]
    fn create_swapchain(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Result<Handle<Swapchain>, Error> {
        let handle = self
            .swapchains
            .borrow_mut()
            .insert(VulkanSwapchain::new(hwnd, &self.api)?)?;
        Ok(handle)
    }

    fn resize_swapchain(&self, handle: Handle<Swapchain>, extent: Extent) -> Result<(), Error> {
        self.swapchains
            .borrow_mut()
            .get_mut(handle)
            .ok_or(Error::InvalidHandle)?
            .resize(&self.api, extent)
    }

    fn destroy_swapchain(&self, handle: Handle<Swapchain>) -> Result<(), Error> {
        // doesn't check that the swapchain is idle... where should that go?
        self.swapchains
            .borrow_mut()
            .remove(handle)
            .ok_or(Error::InvalidHandle)?
            .destroy(&self.api)
    }

    fn get_next_swapchain_image(
        &self,
        handle: Handle<Swapchain>,
    ) -> Result<Handle<RenderTarget>, Error> {
        self.swapchains
            .borrow_mut()
            .get_mut(handle)
            .ok_or(Error::InvalidHandle)?
            .get_next_image(&self.api)?;

        let handle = self
            .render_targets
            .borrow_mut()
            .insert(VulkanRenderTarget::Swapchain { swapchain: handle })?;

        Ok(handle)
    }

    fn present_swapchain_images(&self, handles: &[Handle<Swapchain>]) -> Result<(), Error> {
        let borrow = self.swapchains.borrow();
        let mut swapchains = SmallVec::<[_; MAX_SWAPCHAINS as usize]>::new();

        for handle in handles {
            swapchains.push(borrow.get(*handle).ok_or(Error::InvalidHandle)?);
        }

        VulkanSwapchain::present(&self.api, &swapchains)
    }

    fn create_image(
        &self,
        layout: Layout,
        color_space: ColorSpace,
    ) -> Result<Handle<Image>, Error> {
        todo!()
    }

    fn upload_image(&self, pixels: &PixelBuffer) -> Result<Handle<Image>, Error> {
        todo!()
    }

    fn delete_image(&self, handle: Handle<Image>) -> Result<(), Error> {
        todo!()
    }

    fn get_image_pixels(&self, handle: Handle<Image>) -> Result<PixelBuffer, Error> {
        todo!()
    }

    fn create_command_stream(&self) -> Result<CommandStream, Error> {
        // memory allocation... YAY
        todo!()
    }

    fn cancel_command_stream(&self, commands: CommandStream) {
        todo!()
    }

    fn extend_command_stream(
        &self,
        commands: &mut CommandStream,
        index_count: u32,
        vertex_count: u32,
    ) -> Result<(), Error> {
        todo!()
    }

    fn draw(&self, target: Handle<RenderTarget>, commands: CommandStream) -> Result<(), Error> {
        // translate the commands into render passes

        // submit to the gpu

        // mark any needed sync primitives

        todo!()
    }
}
