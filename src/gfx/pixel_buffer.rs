use super::geometry::Extent;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Layout {
    RGB8,
    RGBA8,
}

impl Layout {
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
    Srgb,
}

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
            extent,
            bytes,
        }
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
