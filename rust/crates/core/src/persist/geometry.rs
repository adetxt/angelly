use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const ZERO: Self = Self { width: 0.0, height: 0.0 };

    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A rectangle with its origin and extent, encoded like `NSRect` as `[[x, y], [width, height]]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn is_finite(&self) -> bool {
        [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite())
    }
}

impl Serialize for Rect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [[self.x, self.y], [self.width, self.height]].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [[x, y], [width, height]] = <[[f64; 2]; 2]>::deserialize(deserializer)?;
        Ok(Self { x, y, width, height })
    }
}
