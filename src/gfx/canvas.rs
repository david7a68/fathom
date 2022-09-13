use crate::indexed_object_pool::newtype_index;

use super::{
    color::Color,
    geometry::{Extent, Rect},
    pixel_buffer::PixelBuffer,
};

/// An empty struct used to distinguish [`ImageHandle`]s from other kinds of
/// handles.
pub struct DummyImage();

newtype_index!(ImageHandle, DummyImage);

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum ImageMode {
    /// Instructs the canvas to treat the uncovered area as transparent.
    /// Uncovered areas will show whatever is underneath the shape.
    Transparent,
    /// Instructs the canvas to scale the image along the axis to fully cover
    /// the shape.
    ScaleToFit,
    /// Instructs the canvas to repeat the image along the axis to fully cover
    /// the shape.
    Repeat,
}

pub enum Paint {
    Fill {
        color: Color,
    },
    Texture {
        handle: ImageHandle,
        /// Defines what happens if the shape is wider than the image's sampled
        /// area.
        mode_x: ImageMode,
        /// Defines what happens if the shape is taller than the image's sampled
        /// area.
        mode_y: ImageMode,
        /// Defines the part of the image to sample to fill the shape (the
        /// sample area). Set this to `Rect(Offset::zero(), Extent::max())` if
        /// the whole image should be used.
        crop: Rect,
    },
}

pub trait Canvas {
    fn extent(&self) -> Extent;

    fn create_image(&mut self, pixels: &PixelBuffer) -> ImageHandle;

    fn destroy_image(&mut self, image: ImageHandle);

    fn draw_rect(&mut self, rect: Rect, paint: &Paint);
}
