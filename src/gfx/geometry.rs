/// The smallest unit of measurement in the UI. It has the same span as a 16-bit
/// signed integer (`i16`).
///
/// It is important to note that conversions from floats always round towards 0.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[must_use]
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

impl std::ops::SubAssign for Px {
    fn sub_assign(&mut self, other: Px) {
        self.0 -= other.0;
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

impl std::ops::Div<i16> for Px {
    type Output = Px;
    fn div(self, other: i16) -> Self::Output {
        Px(self.0 / other)
    }
}

impl std::ops::Rem<i16> for Px {
    type Output = Px;
    fn rem(self, rhs: i16) -> Self::Output {
        Px(self.0 % rhs)
    }
}

impl std::cmp::PartialEq<i32> for Px {
    fn eq(&self, other: &i32) -> bool {
        self.0 as i32 == *other
    }
}

impl std::cmp::PartialOrd<i32> for Px {
    fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
        (self.0 as i32).partial_cmp(other)
    }
}

/// A 2D point in space. It may be negative (to the left or above the top-left
/// corner of the window) if the cursor has been captured and has left the
/// window.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Point {
    pub x: Px,
    pub y: Px,
}

impl Point {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn within(&self, rect: &Rect) -> bool {
        rect.contains(*self)
    }
}

impl std::ops::Sub<Point> for Point {
    type Output = Offset;
    fn sub(self, other: Point) -> Self::Output {
        Offset {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl std::ops::Add<Offset> for Point {
    type Output = Self;
    fn add(self, other: Offset) -> Self::Output {
        Point {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl std::ops::AddAssign<Offset> for Point {
    fn add_assign(&mut self, other: Offset) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl std::ops::Sub<Offset> for Point {
    type Output = Self;
    fn sub(self, other: Offset) -> Self::Output {
        Point {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl std::ops::SubAssign<Offset> for Point {
    fn sub_assign(&mut self, other: Offset) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

/// The size of a 2D rectangle. It is never negative.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Offset {
    pub x: Px,
    pub y: Px,
}

impl Offset {
    pub fn zero() -> Self {
        Self::default()
    }
}

impl std::ops::Add for Offset {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl std::ops::AddAssign for Offset {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl std::ops::Sub for Offset {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl std::ops::SubAssign for Offset {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

/// The size of a 2D rectangle. It is never negative.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Extent {
    pub width: Px,
    pub height: Px,
}

impl Extent {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn area(&self) -> usize {
        (self.width.0 * self.height.0) as usize
    }
}

impl From<Offset> for Extent {
    fn from(offset: Offset) -> Self {
        Extent {
            width: offset.x,
            height: offset.y,
        }
    }
}

/// A 2D rectangle. All coordinates are in pixels and may be negative (outside
/// the window).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
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

    pub fn zero() -> Self {
        Self::default()
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

impl std::ops::Add<Offset> for Rect {
    type Output = Self;
    fn add(self, other: Offset) -> Self::Output {
        Rect {
            top: self.top + other.y,
            left: self.left + other.x,
            bottom: self.bottom + other.y,
            right: self.right + other.x,
        }
    }
}
