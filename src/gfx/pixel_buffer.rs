use super::geometry::{Extent, Point, Rect};

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

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::io::image::Error> {
        use crate::io::image;

        image::decode_png(bytes)
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

#[derive(Clone, Copy)]
pub struct PixelBufferView<'a> {
    region: Rect,
    source: &'a PixelBuffer,
}

impl<'a> PixelBufferView<'a> {
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.region
    }

    #[must_use]
    pub fn layout(&self) -> Layout {
        self.source.layout
    }

    #[must_use]
    pub fn color_space(&self) -> ColorSpace {
        self.source.color_space
    }

    #[must_use]
    pub fn subrect(&self, rect: Rect) -> Self {
        Self {
            region: Rect {
                top: (self.region.top + rect.top).min(self.region.bottom),
                bottom: (self.region.top + rect.bottom).min(self.region.bottom),
                left: (self.region.left + rect.left).min(self.region.right),
                right: (self.region.left + rect.right).min(self.region.right),
            },
            source: self.source,
        }
    }

    #[must_use]
    pub fn bytes(&self) -> Bytes {
        Bytes::new(self)
    }
}

pub struct Bytes<'a> {
    pixels: &'a PixelBuffer,
    /// The cursor tracking the iterator's current position in the buffer.
    cursor: usize,
    /// One past the last byte in the span, used as a sentinel.
    last_byte: usize,
    /// The number of bytes to return in a span.
    span_width: usize,
    /// The distance from the start of one span to the start of the next.
    span_offset: usize,
}

impl<'a> Bytes<'a> {
    fn new(view: &PixelBufferView<'a>) -> Self {
        let cursor = (view.region.left.0 as usize
            + view.region.top.0 as usize * view.region.width().0 as usize)
            * view.layout().bytes_per_pixel();

        let last_byte = (view.region.right.0 as usize
            + view.region.bottom.0 as usize * view.region.width().0 as usize)
            * view.layout().bytes_per_pixel();

        let span_width = view.region.width().0 as usize * view.layout().bytes_per_pixel();
        let span_offset = view.source.extent.width.0 as usize * view.layout().bytes_per_pixel();

        Self {
            pixels: view.source,
            cursor,
            last_byte,
            span_width,
            span_offset,
        }
    }
}

impl<'a> Iterator for Bytes<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor < self.last_byte {
            let bytes = &self.pixels.bytes[self.cursor..self.cursor + self.span_width];
            self.cursor += self.span_offset;
            Some(bytes)
        } else {
            None
        }
    }
}
