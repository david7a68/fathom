use png::{BitDepth, Decoder, DecodingError, Transformations};

use crate::gfx::{pixel_buffer::{ColorSpace, Layout, PixelBuffer}, geometry::{Extent, Px}};

pub const MAX_IMAGE_WIDTH: u32 = Px::MAX.0 as u32;
pub const MAX_IMAGE_HEIGHT: u32 = Px::MAX.0 as u32;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the provided bytestream is not a valid PNG image")]
    InvalidHeader,
    #[error("the provided image is too large to decode")]
    ImageTooLarge {
        requested_width: u32,
        requested_height: u32
    },
    #[error("an unknown error was encountered within the decoder")]
    Unknown(DecodingError),
}

/// Decodes a blob containing a PNG-encoded image into a pixel buffer. Animated
/// images are not supported; only the first frame will be decoded.
pub fn decode_png(bytes: &[u8]) -> Result<PixelBuffer, Error> {
    let mut decoder = Decoder::new(std::io::Cursor::new(bytes));

    // Decode only 8-bit samples until we have 16-bit color support. Tt might be
    // much more efficient to maintain a representation that is as small as
    // possible (and can be copied to the GPU as quickly as possible), then run
    // a shader on it.
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);

    let mut reader = decoder.read_info().map_err(|_| Error::InvalidHeader)?;

    // if reader.info().is_animated() { warn!("animated png not supported"); }

    let mut image = vec![0; reader.output_buffer_size()];
    let stats = reader.next_frame(&mut image).map_err(|e| match e {
        DecodingError::Format(_) => Error::InvalidHeader,
        r => Error::Unknown(r),
    })?;

    assert_eq!(stats.bit_depth, BitDepth::Eight);
    let layout = match stats.color_type {
        png::ColorType::Rgb => Layout::RGB8,
        png::ColorType::Rgba => Layout::RGBA8,
        _ => panic!("should only ever get RGB or RGBA from the decoder because of the Transformations::EXPAND flag"),
    };

    // color space
    let color_space = if reader.info().srgb.is_some() {
        ColorSpace::Srgb
    } else {
        // we don't know what it is, so fall back to linear
        ColorSpace::Linear
    };

    Ok(PixelBuffer::new(
        layout,
        ColorSpace::Srgb,
        Extent {
            width: Px::try_from(stats.width).unwrap(),
            height: Px::try_from(stats.height).unwrap(),
        },
        image.into_boxed_slice(),
    ))
}
