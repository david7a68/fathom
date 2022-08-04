/// The smallest unit of measurement in the UI. It has the same span as a 16-bit
/// signed integer (`i16`).
///
/// It is important to note that conversions from floats always round towards 0.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Px(pub i16);

impl From<Px> for f32 {
    fn from(px: Px) -> Self {
        f32::from(px.0)
    }
}

impl From<i16> for Px {
    fn from(i: i16) -> Self {
        Px(i)
    }
}

impl From<f32> for Px {
    fn from(f: f32) -> Self {
        Px(f as i16)
    }
}

impl TryFrom<i32> for Px {
    type Error = <i16 as TryFrom<i32>>::Error;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        i16::try_from(value).map(Px)
    }
}

impl std::ops::Add for Px {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Px(self.0 + other.0)
    }
}

impl std::ops::AddAssign for Px {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl std::ops::Sub for Px {
    type Output = Px;
    fn sub(self, other: Px) -> Self::Output {
        Px(self.0 - other.0)
    }
}

impl std::ops::Mul<f32> for Px {
    type Output = Px;
    fn mul(self, other: f32) -> Self::Output {
        (self.0 as f32 * other).into()
    }
}

impl std::ops::Mul<Px> for f32 {
    type Output = Px;
    fn mul(self, other: Px) -> Self::Output {
        (self * other.0 as f32).into()
    }
}

/// A 2D point in space. It may be negative (to the left or above the top-left
/// corner of the window) if the cursor has been captured and has left the
/// window.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub x: Px,
    pub y: Px,
}

/// The size of a 2D rectangle. It is never negative.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Extent {
    pub width: Px,
    pub height: Px,
}

/// A 2D rectangle. All coordinates are in pixels and may be negative (outside
/// the window).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub top: Px,
    pub left: Px,
    pub bottom: Px,
    pub right: Px,
}

impl Rect {
    pub fn new(point: Point, extent: Extent) -> Self {
        Rect {
            top: point.y,
            left: point.x,
            bottom: point.y + extent.height,
            right: point.x + extent.width,
        }
    }

    pub fn top_left(&self) -> Point {
        Point {
            x: self.left,
            y: self.top,
        }
    }

    pub fn top_right(&self) -> Point {
        Point {
            x: self.right,
            y: self.top,
        }
    }

    pub fn bottom_left(&self) -> Point {
        Point {
            x: self.left,
            y: self.bottom,
        }
    }

    pub fn bottom_right(&self) -> Point {
        Point {
            x: self.right,
            y: self.bottom,
        }
    }

    pub fn width(&self) -> Px {
        self.right - self.left
    }

    pub fn extent(&self) -> Extent {
        Extent {
            width: self.width(),
            height: self.bottom - self.top,
        }
    }

    pub fn contains(&self, point: Point) -> bool {
        self.left <= point.x
            && point.x < self.right
            && self.top <= point.y
            && point.y <= self.bottom
    }
}
