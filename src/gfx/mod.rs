use crate::handle_pool::Handle;

use self::{
    color::Color,
    geometry::{Extent, Point, Rect, Offset},
    pixel_buffer::{PixelBuffer, PixelBufferView},
};

pub mod color;
pub mod geometry;
pub mod pixel_buffer;
mod vulkan;

pub const MAX_SWAPCHAINS: u32 = 32;
pub const MAX_IMAGES: u32 = 64;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the requested backend is not available")]
    BackendNotFound,
    #[error("no suitable graphics processor could be connected to this computer")]
    NoGraphicsDevice,
    #[error("an object limit has been exceeded")]
    TooManyObjects { limit: u32 },
    #[error("the resource is in use and cannot be modified")]
    ResourceInUse,
    #[error("the resource pointed to by this handle does not exist")]
    InvalidHandle,
    #[error("the swapchain's features are out of sync of the window that it is bound to, update the swapchain and try again")]
    SwapchainOutOfDate,
    #[error(
        "the image cannot be copied as described without resampling, but resampling was disabled"
    )]
    MustResampleImage,
    #[from(ash::vk::Result)]
    #[error("an unhandled error in the Vulkan backend occurred")]
    VulkanInternal {
        #[from]
        error_code: ash::vk::Result,
    },
}

/// An image to which render operations may write to.
pub struct RenderTarget {}

/// A sequence of render targets associated with a window. Each render target
/// may be acquired in turn for rendering, and be 'presented' to the user once
/// rendering is complete.
pub struct Swapchain {}

/// A 2-dimensional image with configurable pixel layout and color space. Refer
/// to [`Layout`] and [`ColorSpace`] for more details.
pub struct Image {}

#[repr(C)]
#[derive(Clone, Copy)]
pub(self) struct Vertex {
    // 32 bytes
    pub point: Point,
    pub color: Color,
}

#[derive(Clone, Copy, Debug)]
pub enum Paint {
    Fill { color: Color },
}

pub struct ImageCopy {
    pub src_rect: Rect,
    pub dst_location: Offset,
}

pub enum Draw {
    Rect { rect: Rect, paint: Paint },
}

#[derive(Debug)]
enum Command {
    // members sorted by size to reduce enum size
    Scissor {
        rect: Rect,
    },
    Polygon {
        first_index: u16,
        num_indices: u16,
    },
    Texture {
        texture: Handle<Image>,
        first_index: u16,
        first_uv: u16,
        num_vertices: u16,
        num_indices: u16,
    },
}

/// A list of drawing commands to submit to the graphics device.
#[must_use]
#[derive(Default)]
pub struct DrawCommandList {
    pub(self) current: Option<Command>,
    pub(self) commands: Vec<Command>,
    pub(self) vertices: Vec<Vertex>,
    pub(self) indices: Vec<u16>,
}

impl DrawCommandList {
    const MAX_VERTICES: usize = u16::MAX as usize + 1;
    const MAX_INDICES: usize = Self::MAX_VERTICES;

    // We want to batch as many commands as we can, as cheaply as possible in
    // order to reduce the number of draw calls that will be necessary. The
    // following rules are used to determine when a new draw command is needed.
    //
    // 1. If the command is Command::Geomtry and the prior command is
    //    Command::Geomtry, and the prior command's vertex and index counts have
    //    enough range to accommodate the new geometry, the new command is
    //    merged with the previous one. This is permitted because each vertex
    //    carries its own color information.
    // 2. If the command is Command::Texture and the prior command is also
    //    Command::Texture, and the rules for geometry also hold, the new
    //    command is merged with the previous one.

    // Why this design?
    //
    // Two other designs were considered: discrete draw commands, and batched
    // commands where the geometry gets written directly to GPU memory buffers.
    // Discrete draw commands was rejected because it means that drawing
    // arbitrary shapes would require that a Vec<Vertex> and Vec<u16> would be
    // needed for each custom mesh, or else some scheme with lifetimes. Directly
    // using GPU buffers was rejected because that means that each command list
    // would need access to the GPU to allocate memory (if the buffer isn't big
    // enough), making the interface clunkier.
    //
    // The current design means that we get a small number of draws without
    // having to deal with the backend directly and the backend doesn't have to
    // deal with batching geometry if it doesn't want to at the cost of
    // increased memory usage (the buffer is copied twice CPU->Host
    // Visible->Device Local).

    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.current = None;
        self.commands.clear();
        self.vertices.clear();
        self.indices.clear();
    }

    pub fn scissor(&mut self, rect: Rect) {
        self.push_command(Command::Scissor { rect });
    }

    /// Draws a rectangle with the specified paint.
    ///
    /// ## Panics
    ///
    /// This function will panic if the number of vertices or indices exceeds
    /// `Self::MAX_VERTICES` or `Self::MAX_INDICES` respectively.
    pub fn draw_rect(&mut self, rect: Rect, paint: Paint) {
        const NUM_VERTICES: u16 = 4;
        const NUM_INDICES: u16 = 6;

        assert!(Self::MAX_VERTICES >= self.vertices.len() + NUM_VERTICES as usize);
        assert!(Self::MAX_INDICES >= self.indices.len() + NUM_INDICES as usize);

        let color = match paint {
            Paint::Fill { color } => color,
        };

        let vertex_offset = self.vertices.len() as u16;
        self.vertices.extend_from_slice(&[
            Vertex {
                point: rect.top_left(),
                color,
            },
            Vertex {
                point: rect.top_right(),
                color,
            },
            Vertex {
                point: rect.bottom_right(),
                color,
            },
            Vertex {
                point: rect.bottom_left(),
                color,
            },
        ]);

        let index_offset = self.indices.len() as u16;
        self.indices.extend_from_slice(&[
            vertex_offset,
            vertex_offset + 1,
            vertex_offset + 2,
            vertex_offset + 2,
            vertex_offset + 3,
            vertex_offset,
        ]);

        if let Some(Command::Polygon { num_indices, .. }) = &mut self.current {
            *num_indices += NUM_INDICES;
        } else {
            self.push_command(Command::Polygon {
                first_index: index_offset,
                num_indices: NUM_INDICES,
            });
        }
    }

    fn push_command(&mut self, new_command: Command) {
        if let Some(old_command) = self.current.replace(new_command) {
            self.commands.push(old_command);
        }
    }
}

///
/// Most methods take `&self` instead of `&mut self` for two reasons: so that
/// the methods can be treated much like one might treat `malloc` (that is,
/// global and without side effects), and so that `CommandStream` can borrow the
/// backend to expand its buffers at need. The second reason is more absolute,
/// but certainly could have been worked around in some way.
///
pub trait GfxDevice {
    #[cfg(target_os = "windows")]
    fn create_swapchain(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Result<Handle<Swapchain>, Error>;

    fn resize_swapchain(&self, handle: Handle<Swapchain>, extent: Extent) -> Result<(), Error>;

    fn destroy_swapchain(&self, handle: Handle<Swapchain>) -> Result<(), Error>;

    fn get_next_swapchain_image(
        &self,
        handle: Handle<Swapchain>,
    ) -> Result<Handle<RenderTarget>, Error>;

    /// Presents the next image in each swapchain after waiting for drawing to
    /// those images to complete. The render target handles used to render to
    /// the swapchains will be invalid once this method returns. Retrieve the
    /// next image in a swapchain by calling `get_next_swapchain_image`.
    ///
    /// ## Synchronization
    ///
    /// This is a synchronizing operation and will block until rendering to the
    /// next image in each swapchain is complete.
    fn present_swapchain_images(&self, handles: &[Handle<Swapchain>]) -> Result<(), Error>;

    /// Creates an image that can be used in rendering operations.
    fn create_image(&self, extent: Extent) -> Result<Handle<Image>, Error>;

    /// Copies portions of a pixel buffer to a target image for rendering.
    /// Operations involving areas beyond the pixel buffer view _or_ the target
    /// image will be clipped away.
    ///
    /// If an copy operation's extents disagree, the the selected
    /// `resample_mode` is used to determine how the image will be rescaled.
    ///
    /// ## Errors
    ///
    /// Will return `Error::MustResampleImage` if `op.must_resample()` and
    /// resampling has been disabled with [`Resample::None`].
    fn copy_pixels(
        &self,
        src: PixelBufferView,
        dst: Handle<Image>,
        ops: &[ImageCopy],
    ) -> Result<(), Error>;

    /// Copies part of `src` into `dst`, resampling as necessary according to
    /// `resample_mode`. Operations involving areas beyond the pixel buffer view
    /// _or_ the target image will be clipped away.
    ///
    /// ## Errors
    ///
    /// Will return `Error::MustResampleImage` if `op.must_resample()` and
    /// resampling has been disabled with [`Resample::None`].
    fn copy_image(
        &self,
        src: Handle<Image>,
        dst: Handle<Image>,
        ops: &[ImageCopy],
    ) -> Result<(), Error>;

    /// Deletes the image, freeing any resources that were associated with it.
    ///
    /// ## Errors
    ///
    /// This method fails if the image is currently being used for an operation
    /// (such as an update, or as part of a draw) and will return [`Error::ResourceInUse`].
    fn destroy_image(&self, handle: Handle<Image>) -> Result<(), Error>;

    /// Copies the pixels from the handle into a [`PixelBuffer`].
    ///
    /// ## Synchronization
    ///
    /// This is a synchronizing operation and will block until any operations
    /// rendering into (writing to) this image are complete.
    fn get_image_pixels(&self, handle: Handle<Image>) -> Result<PixelBuffer, Error>;

    /// Destroys the render target, freeing any associated resources without
    /// affecting the image that the render target was created from.
    fn destroy_render_target(&self, handle: Handle<RenderTarget>) -> Result<(), Error>;

    /// Draws the provided geometry to the render target. All content that was
    /// once in the render target will be overwritten.
    ///
    /// The command list can be reused immediately once this method returns.
    fn draw(
        &self,
        render_target: Handle<RenderTarget>,
        commands: &DrawCommandList,
    ) -> Result<(), Error>;

    /// Flushes all work from the device. This stalls the backend and can hurt
    /// performance.
    fn flush(&self);
}

pub fn init_gfx() -> Result<Box<dyn GfxDevice>, Error> {
    Ok(Box::new(self::vulkan::VulkanGfxDevice::new(true)?))
}
