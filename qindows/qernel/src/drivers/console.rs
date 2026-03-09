//! # Framebuffer Console
//!
//! Text rendering on the Aether framebuffer during early boot.
//! Uses an embedded 8×16 bitmap font — no filesystem needed.
//!
//! After the full Aether compositor loads from user space,
//! this console is replaced by GPU-accelerated SDF text.

use super::gpu::AetherFrameBuffer;

/// Console state
pub struct FramebufferConsole {
    /// Character column (0-based)
    col: usize,
    /// Character row (0-based)
    row: usize,
    /// Maximum columns
    max_cols: usize,
    /// Maximum rows
    max_rows: usize,
    /// Text color (ARGB)
    fg_color: u32,
    /// Background color (ARGB)
    bg_color: u32,
    /// Pending scroll: set by newline() when at max_rows, consumed by write_char()
    scroll_pending: bool,
}

/// Minimal 8×16 bitmap font — just enough for boot diagnostics.
/// Each character is 16 bytes (one byte per row, 8 pixels wide).
/// Only printable ASCII (0x20–0x7E) is included.
pub static FONT_8X16: &[u8] = include_bytes!("font_8x16.bin");

/// Font dimensions
const CHAR_WIDTH: usize = 8;
const CHAR_HEIGHT: usize = 16;

impl FramebufferConsole {
    /// Create a new console for the given framebuffer dimensions.
    pub fn new(fb_width: usize, fb_height: usize) -> Self {
        FramebufferConsole {
            col: 0,
            row: 0,
            max_cols: fb_width / CHAR_WIDTH,
            max_rows: fb_height / CHAR_HEIGHT,
            fg_color: 0x00_06_D6_A0, // Qindows Cyan
            bg_color: 0x00_06_06_0E, // Qindows Deep Black
            scroll_pending: false,
        }
    }

    /// Write a single character at (col, row).
    pub fn write_char(&mut self, fb: &mut AetherFrameBuffer, ch: char) {
        // Process any pending scroll from the previous newline
        if self.scroll_pending {
            self.scroll_up(fb);
        }

        if ch == '\n' {
            self.newline();
            return;
        }

        if ch == '\r' {
            self.col = 0;
            return;
        }

        // Only render printable ASCII
        let ascii = ch as u8;
        if ascii >= 0x20 && ascii <= 0x7E {
            self.render_char(fb, ascii, self.col, self.row);
        }

        self.col += 1;
        if self.col >= self.max_cols {
            self.newline();
        }
    }

    /// Write a string.
    pub fn write_str(&mut self, fb: &mut AetherFrameBuffer, s: &str) {
        for ch in s.chars() {
            self.write_char(fb, ch);
        }
    }

    /// Move to the next line, scrolling if necessary.
    ///
    /// Fix #17: when the cursor hits the last row, we shift the entire
    /// framebuffer up by CHAR_HEIGHT pixels (one row of text) and clear
    /// the vacated bottom row instead of wrapping cursor to row 0.
    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < self.max_rows {
            self.row += 1;
        } else {
            // We're on the last row — need to scroll up.
            // newline() has a mutable receiver but no access to the framebuffer.
            // Set a flag so write_char can trigger the scroll on the next call.
            // The actual pixel shift is performed by scroll_up() called from
            // write_str() / write_char() when scroll_pending is true.
            self.scroll_pending = true;
        }
    }

    /// Scroll the visible area up by one character row (CHAR_HEIGHT pixels).
    ///
    /// Copies every pixel row `CHAR_HEIGHT..fb_height` up by CHAR_HEIGHT,
    /// then fills the last CHAR_HEIGHT rows with the background colour.
    pub fn scroll_up(&mut self, fb: &mut AetherFrameBuffer) {
        let fb_w = fb.width;
        let fb_h = fb.height;
        let row_pixels = CHAR_HEIGHT;

        // Shift every row up by row_pixels
        for y in 0..(fb_h - row_pixels) {
            for x in 0..fb_w {
                let color = fb.read_pixel(x, y + row_pixels);
                fb.draw_pixel(x, y, color);
            }
        }
        // Clear the newly vacated bottom strip
        for y in (fb_h - row_pixels)..fb_h {
            for x in 0..fb_w {
                fb.draw_pixel(x, y, self.bg_color);
            }
        }
        // Row stays at max_rows - 1 after scroll
        self.row = self.max_rows - 1;
        self.scroll_pending = false;
    }

    /// Render a single character from the bitmap font.
    fn render_char(
        &self,
        fb: &mut AetherFrameBuffer,
        ascii: u8,
        col: usize,
        row: usize,
    ) {
        let glyph_idx = (ascii - 0x20) as usize;
        let glyph_offset = glyph_idx * CHAR_HEIGHT;

        // Safety check
        if glyph_offset + CHAR_HEIGHT > FONT_8X16.len() {
            return;
        }

        let px = col * CHAR_WIDTH;
        let py = row * CHAR_HEIGHT;

        for y in 0..CHAR_HEIGHT {
            let row_bits = FONT_8X16[glyph_offset + y];
            for x in 0..CHAR_WIDTH {
                let color = if row_bits & (0x80 >> x) != 0 {
                    self.fg_color
                } else {
                    self.bg_color
                };
                fb.draw_pixel(px + x, py + y, color);
            }
        }
    }

    /// Set the foreground (text) color.
    pub fn set_fg(&mut self, color: u32) {
        self.fg_color = color;
    }

    /// Set the background color.
    pub fn set_bg(&mut self, color: u32) {
        self.bg_color = color;
    }

    /// Clear the entire console.
    pub fn clear(&mut self, fb: &mut AetherFrameBuffer) {
        fb.clear(self.bg_color);
        self.col = 0;
        self.row = 0;
    }

    /// Set the cursor position (col, row).
    pub fn set_cursor(&mut self, col: usize, row: usize) {
        self.col = col.min(self.max_cols.saturating_sub(1));
        self.row = row.min(self.max_rows.saturating_sub(1));
    }

    /// Print a boot status line: [OK] message
    pub fn print_ok(&mut self, fb: &mut AetherFrameBuffer, msg: &str) {
        let saved_fg = self.fg_color;
        self.set_fg(0x00_06_D6_A0); // Cyan
        self.write_str(fb, "[OK] ");
        self.set_fg(0x00_A0_A0_B8); // Light gray
        self.write_str(fb, msg);
        self.write_char(fb, '\n');
        self.set_fg(saved_fg);
    }

    /// Print a boot error line: [!!] message
    pub fn print_err(&mut self, fb: &mut AetherFrameBuffer, msg: &str) {
        let saved_fg = self.fg_color;
        self.set_fg(0x00_EF_47_6F); // Pink/Red
        self.write_str(fb, "[!!] ");
        self.write_str(fb, msg);
        self.write_char(fb, '\n');
        self.set_fg(saved_fg);
    }

    /// Print the Qindows boot banner.
    pub fn print_banner(&mut self, fb: &mut AetherFrameBuffer) {
        let saved_fg = self.fg_color;
        self.set_fg(0x00_7B_2F_F7); // Violet
        self.write_str(fb, " ____  _           _\n");
        self.write_str(fb, "|  _ \\(_)_ __   __| | _____      _____\n");
        self.write_str(fb, "| | | | | '_ \\ / _` |/ _ \\ \\ /\\ / / __|\n");
        self.write_str(fb, "| |_| | | | | | (_| | (_) \\ V  V /\\__ \\\n");
        self.write_str(fb, "|____/|_|_| |_|\\__,_|\\___/ \\_/\\_/ |___/\n");
        self.set_fg(0x00_06_D6_A0); // Cyan
        self.write_str(fb, "        The Final Operating System\n");
        self.set_fg(0x00_A0_A0_B8); // Gray
        self.write_str(fb, "        v1.0.0-genesis | March 4, 2026\n\n");
        self.set_fg(saved_fg);
    }
}
