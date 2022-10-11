use std::cell::RefCell;

use ash::vk;
use smallvec::{smallvec, SmallVec};

use crate::{
    gfx::{
        backend::vulkan::memory::BUFFER_BLOCK_SIZE,
        geometry::Extent,
        pixel_buffer::{ColorSpace, Layout, PixelBuffer},
    },
    handle_pool::{Handle, HandlePool},
};

use super::{
    Backend, CommandStream, Error, Image, RenderTarget, Swapchain, Vertex, MAX_SWAPCHAINS,
};

mod api;
mod memory;
mod simple_shader;
mod swapchain;

use self::{
    api::VulkanApi,
    memory::VulkanMemory,
    simple_shader::{SimpleShader, SimpleShaderFactory},
    swapchain::{VulkanSwapchain, PREFERRED_NUM_IMAGES},
};

const SDR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
const HDR_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

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
    memory: RefCell<VulkanMemory>,

    sdr_shader: SimpleShader,

    swapchains: RefCell<HandlePool<_Swapchain, Swapchain, { MAX_SWAPCHAINS }>>,
    render_targets: RefCell<HandlePool<VulkanRenderTarget, RenderTarget, 64>>,
}

// each computer has a finite number of display formats
// changing display modes causes only minor increase in memory footprint
// must look up formats every time
// could cache shader id in the swapchain...
// also need formats for drawing to pixel buffers
// one format for each pixel buffer
// fixed number of formats
// WE ONLY SUPPORT TWO FORMATS: R8G8B8A8_SRGB, R16G16B16A16_HALF

enum VulkanRenderTarget {
    Swapchain { swapchain: Handle<Swapchain> },
}

impl Vulkan {
    pub fn new() -> Result<Self, Error> {
        let api = VulkanApi::new(true)?;
        let simple_shader_factory = SimpleShaderFactory::new(&api)?;

        let sdr_shader = simple_shader_factory.create_shader(vk::Format::R8G8B8A8_SRGB, &api)?;

        Ok(Self {
            api,
            memory: RefCell::new(VulkanMemory::new()),
            sdr_shader,
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
        let swapchain = VulkanSwapchain::new(hwnd, &self.api)?;
        let extent = swapchain.extent();

        let mut frame_buffers = SmallVec::new();
        for image_view in swapchain.image_views() {
            let create_info = vk::FramebufferCreateInfo {
                render_pass: self.sdr_shader.render_pass,
                attachment_count: 1,
                p_attachments: image_view,
                width: extent.width,
                height: extent.height,
                layers: 1,
                ..Default::default()
            };

            frame_buffers.push(unsafe { self.api.device.create_framebuffer(&create_info, None) }?);
        }

        let handle = self.swapchains.borrow_mut().insert(_Swapchain {
            swapchain,
            frame_buffers,
        })?;
        Ok(handle)
    }

    /// Resizes the swapchain's buffers. If this function is called between
    /// `get_next_swapchain_image()` and `present_swapchain_images()`, the
    /// render target returned by `get_next_swapchain_image` remains valid.
    fn resize_swapchain(&self, handle: Handle<Swapchain>, extent: Extent) -> Result<(), Error> {
        let mut sc = self.swapchains.borrow_mut();
        let swapchain = sc.get_mut(handle).ok_or(Error::InvalidHandle)?;

        swapchain.swapchain.resize(&self.api, extent)?;
        if swapchain.swapchain.current_image().is_some() {
            swapchain.swapchain.get_next_image(&self.api)?;
        }

        Ok(())
    }

    fn destroy_swapchain(&self, handle: Handle<Swapchain>) -> Result<(), Error> {
        // doesn't check that the swapchain is idle... where should that go?
        let swapchain = self
            .swapchains
            .borrow_mut()
            .remove(handle)
            .ok_or(Error::InvalidHandle)?;

        for fb in swapchain.frame_buffers {
            unsafe { self.api.device.destroy_framebuffer(fb, None) };
        }

        swapchain.swapchain.destroy(&self.api)
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
        let mut memory = self.memory.borrow_mut();
        let index_buffers = smallvec![memory.allocate_buffer(&self.api)?];
        let vertex_buffers = smallvec![memory.allocate_buffer(&self.api)?];
        let uv_buffers = smallvec![memory.allocate_buffer(&self.api)?];

        Ok(CommandStream {
            commands: vec![],
            index_buffers,
            vertex_buffers,
            uv_buffers,
            backend: self,
            index_buffer_cursor: 0,
            vertex_buffer_cursor: 0,
            uv_buffer_cursor: 0,
        })
    }

    fn cancel_command_stream(&self, mut commands: CommandStream) {
        let mut memory = self.memory.borrow_mut();
        for buffer in commands.index_buffers.drain(..) {
            memory.free_buffer(buffer).expect("internal error");
        }
        for buffer in commands.vertex_buffers.drain(..) {
            memory.free_buffer(buffer).expect("internal error");
        }
    }

    fn extend_command_stream(
        &self,
        commands: &mut CommandStream,
        index_count: u32,
        vertex_count: u32,
    ) -> Result<(), Error> {
        let mut memory = self.memory.borrow_mut();

        if index_count > 0 {
            assert!(
                index_count as usize <= BUFFER_BLOCK_SIZE as usize / std::mem::size_of::<u16>()
            );
            commands
                .index_buffers
                .push(memory.allocate_buffer(&self.api)?);
        }

        if vertex_count > 0 {
            assert!(
                vertex_count as usize <= BUFFER_BLOCK_SIZE as usize / std::mem::size_of::<Vertex>()
            );
            commands
                .vertex_buffers
                .push(memory.allocate_buffer(&self.api)?);
        }

        Ok(())
    }

    fn draw(&self, target: Handle<RenderTarget>, commands: CommandStream) -> Result<(), Error> {
        // translate the commands into render passes

        // image to rendertarget (preserves the render target)
        // swapchain to rendertarget (uses the most recently acquired image, invalidates the handle)

        let mut rt = self.render_targets.borrow_mut();
        let render_target = rt.get(target).ok_or(Error::InvalidHandle)?;

        match render_target {
            VulkanRenderTarget::Swapchain { swapchain } => {
                if let Some(swapchain) = self.swapchains.borrow().get(*swapchain) {
                    // This fails only if the swapchain image was somehow reset
                    // since the last call to `get_swapchain_image`.
                    let image_view = swapchain.current_image().expect("internal error");

                    // create frame buffer for that swapchain image?
                    // what's the point if the frame buffer's dependencies only change on swapchain resize?

                    // create a frame buffer for that image (why do it here?)
                    // get a command buffer
                    // begin the pipeline
                    // begin the render pass
                    // for command in commands

                    // end the render pass
                    // end the pipeline
                    // submit the command buffer
                    // bind fence
                    rt.remove(target);
                    Ok(())
                } else {
                    // A swapchain image was acquired, but the swapchain was
                    // destroyed before it could be used.
                    rt.remove(target);
                    Err(Error::InvalidHandle)
                }
            }
        }
    }
}

pub struct _Swapchain {
    swapchain: VulkanSwapchain,
    frame_buffers: SmallVec<[vk::Framebuffer; PREFERRED_NUM_IMAGES]>,
}
