//! Layout manager for widget positioning

/// Simple vertical layout
pub struct Layout {
    pub x: u16,
    pub y: u16,
    pub spacing: u16,
}

impl Layout {
    pub fn new(x: u16, y: u16) -> Self {
        Self { x, y, spacing: 10 }
    }

    /// Get next widget position and advance
    pub fn next(&mut self, height: u16) -> (u16, u16) {
        let pos = (self.x, self.y);
        self.y += height + self.spacing;
        pos
    }
}
