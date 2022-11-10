use super::geometry::{Extent, Offset, Point, Rect};

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Layout {
    RGB8,
    RGBA8,
}

impl Layout {
    #[must_use]
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Layout::RGB8 => 3,
            Layout::RGBA8 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum ColorSpace {
    Linear,
    Srgb,
}

#[must_use]
pub struct PixelBuffer {
    layout: Layout,
    color_space: ColorSpace,
    bytes: Box<[u8]>,
    //Note(straivers): We duplicate a bit of data here just to make things
    //slightly more convenient (fewer casts). If struct size becomes an issue,
    //we could keep just width or height and calculate the other value as
    //needed.
    extent: Extent,
}

impl PixelBuffer {
    pub fn new(layout: Layout, color_space: ColorSpace, extent: Extent, bytes: Box<[u8]>) -> Self {
        let num_bytes = layout.bytes_per_pixel() * extent.area();
        assert_eq!(num_bytes, bytes.len());

        Self {
            layout,
            color_space,
            bytes,
            extent,
        }
    }

    #[must_use]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    #[must_use]
    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    #[must_use]
    pub fn extent(&self) -> Extent {
        self.extent
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl<'a> From<&'a PixelBuffer> for PixelBufferView<'a> {
    fn from(pb: &'a PixelBuffer) -> Self {
        Self {
            region: Rect::new(Point::zero(), pb.extent),
            source: pb,
        }
    }
}

pub struct PixelBufferView<'a> {
    region: Rect,
    source: &'a PixelBuffer,
}

impl<'a> PixelBufferView<'a> {
    pub fn write_bytes<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        if self.region.extent() == self.source.extent {
            writer.write_all(&self.source.bytes)?;
        } else {
            let pixel_size = self.source.layout.bytes_per_pixel();
            let row_size = self.source.extent.width.0 as usize * pixel_size;

            let region_width = self.region.width().0 as usize * pixel_size;

            let start =
                row_size * self.region.top.0 as usize + self.region.left.0 as usize * pixel_size;
            let end = row_size * self.region.bottom.0 as usize
                + self.region.right.0 as usize * pixel_size;

            let mut row_offset = start;

            while row_offset < end {
                writer.write_all(&self.source.bytes[row_offset..row_offset + region_width])?;
                row_offset += row_size;
            }
        }

        Ok(())
    }
}
