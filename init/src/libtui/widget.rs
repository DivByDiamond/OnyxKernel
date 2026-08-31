//! Widget trait and basic widgets with real text rendering (todo P3 #4).
//!
//! Text is blitted through libtui::font (PSF glyphs into the mmap'd
//! framebuffer); the caller passes the framebuffer slice plus its pixel
//! stride so widgets can position and clip correctly.

use super::event::Event;
use super::font;

/// Widget trait - all UI elements implement this
pub trait Widget {
    /// Draw widget to framebuffer at (x, y); `stride` is the pixel width
    /// of one framebuffer row.
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, stride: usize);

    /// Handle input event, return true if consumed
    fn handle_event(&mut self, ev: &Event) -> bool;

    /// Get widget dimensions (width, height)
    fn bounds(&self) -> (u16, u16);
}

/// Fill an axis-aligned rectangle (helper shared by the widgets below).
fn fill_rect(fb: &mut [u32], x: u16, y: u16, w: u16, h: u16, stride: usize, color: u32) {
    for dy in 0..h as usize {
        for dx in 0..w as usize {
            let px = x as usize + dx;
            let py = y as usize + dy;
            let idx = py * stride + px;
            if idx < fb.len() {
                fb[idx] = color;
            }
        }
    }
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

impl Button {
    /// True when the pointer position is inside the button face.
    fn hit_test(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

impl Widget for Button {
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, stride: usize) {
        // Solid face + darker border for contrast.
        fill_rect(fb, x, y, self.width, self.height, stride, self.color);
        let border = 0x222222;
        // Border strips (2px).
        fill_rect(fb, x, y, self.width, 2, stride, border);
        fill_rect(fb, x, y + self.height - 2, self.width, 2, stride, border);
        fill_rect(fb, x, y, 2, self.height, stride, border);
        fill_rect(fb, x + self.width - 2, y, 2, self.height, stride, border);
        // Centered text (8x16 cell assumption via font advance).
        let text_w = self.text.len() as u16 * 8;
        let tx = x + (self.width.saturating_sub(text_w)) / 2;
        let ty = y + (self.height.saturating_sub(16)) / 2;
        font::draw_text(
            fb,
            stride,
            tx as usize,
            ty as usize,
            self.text,
            0xFFFFFF,
            self.color,
        );
    }

    fn handle_event(&mut self, ev: &Event) -> bool {
        match ev {
            Event::MouseClick { x, y, button: 1 } => self.hit_test(*x, *y),
            // Hover inside the face is consumed (highlight hook for skins).
            Event::MouseMove { x, y } => self.hit_test(*x, *y),
            _ => false,
        }
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
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, stride: usize) {
        // Transparent glyphs only (no background paint).
        font::draw_text_fg(fb, stride, x as usize, y as usize, self.text, 0xFFFFFF);
    }

    fn handle_event(&mut self, _ev: &Event) -> bool {
        false // labels don't handle events
    }

    fn bounds(&self) -> (u16, u16) {
        (self.text.len() as u16 * 8, 16) // assume 8x16 font
    }
}

/// TextBox widget (input field with cursor, insert and delete)
pub struct TextBox {
    pub buffer: [u8; 64],
    pub cursor: usize,
    pub len: usize,
    pub x: u16,
    pub y: u16,
    pub width: u16,
}

impl Widget for TextBox {
    fn draw(&self, fb: &mut [u32], x: u16, y: u16, stride: usize) {
        // Box face + border.
        fill_rect(fb, x, y, self.width, 24, stride, 0xCCCCCC);
        fill_rect(fb, x, y, self.width, 2, stride, 0x444444);
        fill_rect(fb, x, y + 22, self.width, 2, stride, 0x444444);
        // Text content as a NUL-terminated buffer slice.
        let end = self.len.min(63);
        let text = &self.buffer[..end];
        let as_str = core::str::from_utf8(text).unwrap_or("");
        font::draw_text(
            fb,
            stride,
            x as usize + 4,
            y as usize + 4,
            as_str,
            0x000000,
            0xCCCCCC,
        );
        // Block cursor under the insertion point.
        let cx = x as usize + 4 + self.cursor * 8;
        let cy = y as usize + 4;
        for row in 0..14usize {
            for col in 0..7usize {
                let idx = (cy + row) * stride + cx + col;
                if cx + col < stride && idx < fb.len() {
                    fb[idx] = 0x222222;
                }
            }
        }
    }

    fn handle_event(&mut self, ev: &Event) -> bool {
        if let Event::KeyPress(key) = ev {
            match *key {
                // Backspace/Delete: remove the character before the cursor.
                127 | 8 => {
                    if self.cursor > 0 {
                        self.buffer
                            .copy_within(self.cursor..self.len, self.cursor - 1);
                        self.cursor -= 1;
                        self.len -= 1;
                    }
                }
                // Insert: shift the tail right, place the char at cursor.
                32..=126 => {
                    if self.len < 63 {
                        self.buffer
                            .copy_within(self.cursor..self.len, self.cursor + 1);
                        self.buffer[self.cursor] = *key;
                        self.cursor += 1;
                        self.len += 1;
                    }
                }
                _ => {}
            }
            return true;
        }
        false
    }

    fn bounds(&self) -> (u16, u16) {
        (self.width, 24)
    }
}
