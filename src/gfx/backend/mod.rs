//! ## Design Rationale
//!
//! TODO(straivers): elaborate
//!
//! - maximize flexibility for backend implementations
//! - minimize implementation effort
//!   - an api that is too low level requires a more fine-grained mapping
//!     between the api and the backend, possibly exposing impedance mismatches
//!     that increase effot
//!   - at the same time, a low-level api permits more code to be shared between
//!     apis
//!   - since we're only supporting one api for now, err on the side of
//!     low-effort, ossifying the wrong low level primitives just wastes time
//! - provide uniform interface for higher-level drawing API
//!   - gotta go somewhere, right? don't want rendering to bleed into the UI or
//!     what-have-you
//! - minimize performance impact of this intermediate layer
//!   - goes together with maximizing implementation flexibility
//!   - most important part of this design that affects performance is probably
//!     the draw stream
//!     - hopefully, keeping DrawCommand internal wil make it easier to optimize
//!       in the future
//! - do only the things needed for the usecase
//!   - expand or reformulate later, as needs change
//!   - don't know enough to make forward-looking designs, bearing that burden
//!     only slows development (very important rn)

use std::ptr::NonNull;

use smallvec::SmallVec;

use crate::handle_pool::Handle;

use super::{
    color::Color,
    geometry::{Extent, Point, Rect},
    pixel_buffer::{ColorSpace, Layout, PixelBuffer},
};

mod vulkan;

const MAX_SWAPCHAINS: u32 = 64;

/// An image to which render operations may write to.
pub struct RenderTarget {}

/// A sequence of render targets associated with a window. Each render target
/// may be acquired in turn for rendering, and be 'presented' to the user once
/// rendering is complete.
pub struct Swapchain {}

/// A 2-dimensional image with configurable pixel layout and color space. Refer
/// to [`Layout`] and [`ColorSpace`] for more details.
pub struct Image {}

/// A region of memory used by the backend to store vertex and index data.
pub struct Buffer {}

pub(self) enum DrawCommand {
    Scissor {
        rect: Rect,
    },
    // we can calculat vertex_offset and index_offset relative to the previous draw call
    Indexed {
        vertex_buffer: u8,
        vertex_count: u32,
        index_buffer: u8,
        index_count: u32,
    },
    SubImage {
        image: Handle<Image>,
        vertex_buffer: u8,
        // implies uv_count
        vertex_count: u32,
        index_buffer: u8,
        index_count: u32,
    },
}

pub struct CommandStream<'a> {
    pub(self) commands: Vec<DrawCommand>,
    /// We use a SmallVec here since there must always be at least one index
    /// buffer, and (we assume) most usecases should fit within just that one
    /// buffer. However, if that buffer is not large enough for whatever reason,
    /// the backend can either extend the existing buffer, or append a new
    /// buffer.
    pub(self) index_buffers: SmallVec<[MappedBuffer<u16>; 1]>,
    /// We use a SmallVec here since there must always be at least one vertex
    /// buffer, and (we assume) most usecases should fit within just that one
    /// buffer. However, if that buffer is not large enough for whatever reason,
    /// the backend can either extend the existing buffer, or append a new
    /// buffer.
    pub(self) vertex_buffers: SmallVec<[MappedBuffer<Vertex>; 1]>,
    /// We use a SmallVec here since there must always be at least one UV
    /// buffer, and (we assume) most usecases should fit within just that one
    /// buffer. However, if that buffer is not large enough for whatever reason,
    /// the backend can either extend the existing buffer, or append a new
    /// buffer.
    pub(self) uv_buffers: SmallVec<[MappedBuffer<UV>; 1]>,
    /// A reference to the backend so that the index and vertex buffers can be
    /// resized if necessary.
    pub(self) backend: &'a dyn Backend,
    /// The location of the first unused index in the last index buffer
    index_buffer_cursor: u32,
    /// The location of the first unused vertex in the last vertex buffer
    vertex_buffer_cursor: u32,
    /// The location of the first unused UV in the last UV buffer
    uv_buffer_cursor: u32,
}

impl<'a> CommandStream<'a> {
    pub fn set_scissor(&mut self, rect: Rect) {
        self.commands.push(DrawCommand::Scissor { rect });
    }

    pub fn draw_indexed(&mut self, vertices: &[Vertex], indices: &[u16]) {
        todo!()
    }

    pub fn draw_sub_image(
        &mut self,
        image: Handle<Image>,
        vertices: &[Vertex],
        uvs: &[UV],
        indices: &[u16],
    ) {
        todo!()
    }
}

pub struct Vertex {
    pub point: Point,
    pub color: Color,
}

pub struct UV {
    pub u: f32,
    pub v: f32,
}

pub(self) struct MappedBuffer<T> {
    pub(self) handle: Handle<Buffer>,
    pub(self) capacity: u32,
    pub(self) pointer: NonNull<T>,
}

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
    #[error("an unhandled error in the Vulkan backend occurred")]
    VulkanInternal { error_code: ash::vk::Result },
    #[error("an extension required by the Vulkan backend could not be found")]
    VulkanExtensionNotPresent { name: &'static str },
}

///
/// Most methods take `&self` instead of `&mut self` for two reasons: so that
/// the methods can be treated much like one might treat `malloc` (that is,
/// global and without side effects), and so that `CommandStream` can borrow the
/// backend to expand its buffers at need. The second reason is more absolute,
/// but certainly could have been worked around in some way.
///
pub trait Backend {
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

    /// Presents the next image in each swapchain. Any draws submitted to the
    /// backend since the last presentation are guaranteed to be complete.
    ///
    /// Once this method returns, all render target handles pointing to those
    /// images will be invalidated. Retrieve the next image in a swapchain by
    /// calling `get_next_swapchain_image()`.
    ///
    /// ## Synchronization
    ///
    /// This is a synchronizing operations and will block until rendering to the
    /// next image in each swapchain is complete.
    fn present_swapchain_images(&self, handles: &[Handle<Swapchain>]) -> Result<(), Error>;

    /// Creates an image that can be used in rendering operations.
    fn create_image(&self, layout: Layout, color_space: ColorSpace)
        -> Result<Handle<Image>, Error>;

    /// Uploads an image from a pixel buffer so that it can be used for
    /// rendering operations.
    fn upload_image(&self, pixels: &PixelBuffer) -> Result<Handle<Image>, Error>;

    /// Deletes the image, freeing any resources that were associated with it.
    ///
    /// ## Note
    ///
    /// Any pending operations depending on the image will be permitted to
    /// complete before the resources backing the image are released.
    fn delete_image(&self, handle: Handle<Image>) -> Result<(), Error>;

    /// Copies the pixels from the handle into a [`PixelBuffer`].
    ///
    /// ## Synchronization
    ///
    /// This is a synchronizing operation and will block until any operations
    /// rendering into (writing to) this image are complete.
    fn get_image_pixels(&self, handle: Handle<Image>) -> Result<PixelBuffer, Error>;

    /// Creates a new command stream to which draw commands may be recorded.
    /// Once recording is complete, submit it for rendering by calling `draw`.
    fn create_command_stream(&self) -> Result<CommandStream, Error>;

    /// Cancels an a stream that is being recorded.
    fn cancel_command_stream(&self, commands: CommandStream);

    /// Extends the command stream to accomodate more vertices and indices. This
    /// is used by `CommandStream` and should not need to be called by client
    /// code.
    fn extend_command_stream(
        &self,
        commands: &mut CommandStream,
        index_count: u32,
        vertex_count: u32,
    ) -> Result<(), Error>;

    /// Submits a list of render operations to the backend that will be written
    /// to the render target.
    ///
    /// ## Synchronization
    ///
    /// Rendering will progress asynchronously until a synchronizing operation
    /// occurs.
    fn draw(&self, target: Handle<RenderTarget>, commands: CommandStream) -> Result<(), Error>;
}
