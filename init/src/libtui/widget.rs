//! Widget trait and basic widgets

use super::event::Event;

/// Widget trait - all UI elements implement this
pub trait Widget {
    /// Draw widget to framebuffer at (x, y)
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, width: usize);

    /// Handle input event, return true if consumed
    fn handle_event(&mut self, ev: &Event) -> bool;

    /// Get widget dimensions (width, height)
    fn bounds(&self) -> (u16, u16);
}

/// Button widget
pub struct Button {
    pub text: &'static str,
    pub width: u16,
    pub height: u16,
    pub x: u16,
    pub y: u16,
    pub color: u32,
}

impl Widget for Button {
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, width: usize) {
        // Draw rectangle
        let color = self.color;
        for dy in 0..self.height {
            for dx in 0..self.width {
                let px = x + dx;
                let py = y + dy;
                let idx = py as usize * width + px as usize;
                if idx < fb.len() {
                    fb[idx] = color;
                }
            }
        }
        // TODO: Draw text (requires font rendering)
    }

    fn handle_event(&mut self, ev: &Event) -> bool {
        if let Event::MouseClick { x, y, button: 1 } = ev {
            // Check if click inside button bounds
            if *x >= self.x && *x < self.x + self.width &&
               *y >= self.y && *y < self.y + self.height
            {
                return true; // consumed
            }
        }
        false
    }

    fn bounds(&self) -> (u16, u16) {
        (self.width, self.height)
    }
}

/// Label widget (text only)
pub struct Label {
    pub text: &'static str,
    pub x: u16,
    pub y: u16,
}

impl Widget for Label {
    fn draw(&self, _fb: &mut [u32], _x: u16, _y: u16, _width: usize) {
        // TODO: Draw text with PSF font
    }

    fn handle_event(&mut self, _ev: &Event) -> bool {
        false // labels don't handle events
    }

    fn bounds(&self) -> (u16, u16) {
        (self.text.len() as u16 * 8, 16) // assume 8x16 font
    }
}

/// TextBox widget (input field)
pub struct TextBox {
    pub buffer: [u8; 64],
    pub cursor: usize,
    pub len: usize,
    pub x: u16,
    pub y: u16,
    pub width: u16,
}

impl Widget for TextBox {
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, width: usize) {
        // Draw box
        let color = 0xCCCCCC;
        for dy in 0..24 {
            for dx in 0..self.width {
                let px = x + dx;
                let py = y + dy;
                let idx = py as usize * width + px as usize;
                if idx < fb.len() {
                    fb[idx] = color;
                }
            }
        }
        // TODO: Draw text content + cursor
    }

    fn handle_event(&mut self, ev: &Event) -> bool {
        if let Event::KeyPress(key) = ev {
            if *key == 127 { // backspace
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.len -= 1;
                }
            } else if *key >= 32 && *key < 127 && self.len < 63 {
                self.buffer[self.len] = *key;
                self.len += 1;
                self.cursor += 1;
            }
            return true;
        }
        false
    }

    fn bounds(&self) -> (u16, u16) {
        (self.width, 24)
    }
}
