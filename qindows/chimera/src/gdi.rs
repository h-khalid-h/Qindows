//! # Chimera GDI Emulation
//!
//! Emulates GDI32 (Windows Graphics Device Interface) for
//! legacy Win32 applications. Maps GDI calls to Aether
//! rendering primitives.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// GDI handle type.
pub type HGdi = u32;

/// GDI object types.
#[derive(Debug, Clone)]
pub enum GdiObject {
    /// Device context
    Dc(DeviceContext),
    /// Pen (for lines)
    Pen(GdiPen),
    /// Brush (for fills)
    Brush(GdiBrush),
    /// Font
    Font(GdiFont),
    /// Bitmap
    Bitmap(GdiBitmap),
    /// Region (clip region)
    Region(GdiRegion),
}

/// A device context (HDC).
#[derive(Debug, Clone)]
pub struct DeviceContext {
    /// Handle
    pub handle: HGdi,
    /// Associated window handle (HWND)
    pub hwnd: u32,
    /// Current pen
    pub pen: HGdi,
    /// Current brush
    pub brush: HGdi,
    /// Current font
    pub font: HGdi,
    /// Current text color (COLORREF: 0x00BBGGRR)
    pub text_color: u32,
    /// Current background color
    pub bg_color: u32,
    /// Background mode (TRANSPARENT=1, OPAQUE=2)
    pub bg_mode: u32,
    /// Current position (for MoveTo/LineTo)
    pub pos_x: i32,
    pub pos_y: i32,
    /// Drawing operations recorded
    pub ops: Vec<DrawOp>,
}

/// A GDI pen.
#[derive(Debug, Clone)]
pub struct GdiPen {
    /// Style (PS_SOLID=0, PS_DASH=1, PS_DOT=2, etc.)
    pub style: u32,
    /// Width
    pub width: u32,
    /// Color (COLORREF)
    pub color: u32,
}

/// A GDI brush.
#[derive(Debug, Clone)]
pub struct GdiBrush {
    /// Style (BS_SOLID=0, BS_HOLLOW=1, BS_HATCHED=2)
    pub style: u32,
    /// Color (COLORREF)
    pub color: u32,
    /// Hatch pattern (for BS_HATCHED)
    pub hatch: u32,
}

/// A GDI font.
#[derive(Debug, Clone)]
pub struct GdiFont {
    /// Height (logical units)
    pub height: i32,
    /// Width (0 = default)
    pub width: i32,
    /// Weight (FW_NORMAL=400, FW_BOLD=700)
    pub weight: u32,
    /// Italic
    pub italic: bool,
    /// Underline
    pub underline: bool,
    /// Face name
    pub face_name: String,
}

/// A GDI bitmap.
#[derive(Debug, Clone)]
pub struct GdiBitmap {
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
    pub pixels: Vec<u8>,
}

/// A GDI clipping region.
#[derive(Debug, Clone)]
pub struct GdiRegion {
    pub rects: Vec<(i32, i32, i32, i32)>, // (left, top, right, bottom)
}

/// Drawing operations (recorded for batched rendering).
#[derive(Debug, Clone)]
pub enum DrawOp {
    MoveTo(i32, i32),
    LineTo(i32, i32, u32, u32), // (x, y, color, width)
    Rectangle(i32, i32, i32, i32, u32, u32), // (l, t, r, b, pen_color, brush_color)
    Ellipse(i32, i32, i32, i32, u32, u32),
    TextOut(i32, i32, String, u32), // (x, y, text, color)
    BitBlt(i32, i32, u32, u32, HGdi, i32, i32, u32), // (dx, dy, w, h, src_dc, sx, sy, rop)
    FillRect(i32, i32, i32, i32, u32), // (l, t, r, b, color)
    Polygon(Vec<(i32, i32)>, u32, u32), // (points, pen_color, brush_color)
    SetPixel(i32, i32, u32),
}

/// The GDI Emulator.
pub struct GdiEmulator {
    /// All GDI objects by handle
    pub objects: BTreeMap<HGdi, GdiObject>,
    /// Next handle
    next_handle: HGdi,
    /// Stock objects
    pub stock_black_pen: HGdi,
    pub stock_white_brush: HGdi,
    pub stock_system_font: HGdi,
    /// Stats
    pub stats: GdiStats,
}

/// GDI statistics.
#[derive(Debug, Clone, Default)]
pub struct GdiStats {
    pub objects_created: u64,
    pub objects_deleted: u64,
    pub draw_calls: u64,
    pub text_outs: u64,
    pub bit_blts: u64,
}

impl GdiEmulator {
    pub fn new() -> Self {
        let mut emu = GdiEmulator {
            objects: BTreeMap::new(),
            next_handle: 100, // Reserve 1-99 for stock objects
            stock_black_pen: 0,
            stock_white_brush: 0,
            stock_system_font: 0,
            stats: GdiStats::default(),
        };

        // Create stock objects
        emu.stock_black_pen = emu.create_pen(0, 1, 0x00000000);
        emu.stock_white_brush = emu.create_solid_brush(0x00FFFFFF);
        emu.stock_system_font = emu.create_font(16, 0, 400, false, false, "System");

        emu
    }

    /// CreateDC — create a device context.
    pub fn create_dc(&mut self, hwnd: u32) -> HGdi {
        let handle = self.alloc_handle();
        let dc = DeviceContext {
            handle,
            hwnd,
            pen: self.stock_black_pen,
            brush: self.stock_white_brush,
            font: self.stock_system_font,
            text_color: 0x00000000,
            bg_color: 0x00FFFFFF,
            bg_mode: 2,
            pos_x: 0, pos_y: 0,
            ops: Vec::new(),
        };
        self.objects.insert(handle, GdiObject::Dc(dc));
        handle
    }

    /// CreatePen
    pub fn create_pen(&mut self, style: u32, width: u32, color: u32) -> HGdi {
        let handle = self.alloc_handle();
        self.objects.insert(handle, GdiObject::Pen(GdiPen { style, width, color }));
        self.stats.objects_created += 1;
        handle
    }

    /// CreateSolidBrush
    pub fn create_solid_brush(&mut self, color: u32) -> HGdi {
        let handle = self.alloc_handle();
        self.objects.insert(handle, GdiObject::Brush(GdiBrush { style: 0, color, hatch: 0 }));
        self.stats.objects_created += 1;
        handle
    }

    /// CreateFont
    pub fn create_font(&mut self, height: i32, width: i32, weight: u32, italic: bool, underline: bool, face: &str) -> HGdi {
        let handle = self.alloc_handle();
        self.objects.insert(handle, GdiObject::Font(GdiFont {
            height, width, weight, italic, underline,
            face_name: String::from(face),
        }));
        self.stats.objects_created += 1;
        handle
    }

    /// SelectObject — select a GDI object into a DC.
    pub fn select_object(&mut self, hdc: HGdi, obj: HGdi) -> Option<HGdi> {
        // Determine which kind of object we're selecting (read-only borrow)
        let obj_kind = match self.objects.get(&obj) {
            Some(GdiObject::Pen(_)) => 0u8,
            Some(GdiObject::Brush(_)) => 1,
            Some(GdiObject::Font(_)) => 2,
            _ => return None,
        };

        // Now mutably borrow the DC and swap the handle
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let old = match obj_kind {
                0 => { let old = dc.pen; dc.pen = obj; old }
                1 => { let old = dc.brush; dc.brush = obj; old }
                _ => { let old = dc.font; dc.font = obj; old }
            };
            Some(old)
        } else {
            None
        }
    }

    /// DeleteObject
    pub fn delete_object(&mut self, handle: HGdi) -> bool {
        if self.objects.remove(&handle).is_some() {
            self.stats.objects_deleted += 1;
            true
        } else {
            false
        }
    }

    /// SetTextColor
    pub fn set_text_color(&mut self, hdc: HGdi, color: u32) -> u32 {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let old = dc.text_color;
            dc.text_color = color;
            old
        } else { 0 }
    }

    /// MoveTo
    pub fn move_to(&mut self, hdc: HGdi, x: i32, y: i32) {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            dc.pos_x = x;
            dc.pos_y = y;
            dc.ops.push(DrawOp::MoveTo(x, y));
        }
    }

    /// LineTo
    pub fn line_to(&mut self, hdc: HGdi, x: i32, y: i32) {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let pen_color = self.get_pen_color(dc.pen);
            let pen_width = self.get_pen_width(dc.pen);
            dc.ops.push(DrawOp::LineTo(x, y, pen_color, pen_width));
            dc.pos_x = x;
            dc.pos_y = y;
            self.stats.draw_calls += 1;
        }
    }

    /// Rectangle
    pub fn rectangle(&mut self, hdc: HGdi, left: i32, top: i32, right: i32, bottom: i32) {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let pen_color = self.get_pen_color(dc.pen);
            let brush_color = self.get_brush_color(dc.brush);
            dc.ops.push(DrawOp::Rectangle(left, top, right, bottom, pen_color, brush_color));
            self.stats.draw_calls += 1;
        }
    }

    /// TextOut
    pub fn text_out(&mut self, hdc: HGdi, x: i32, y: i32, text: &str) {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let color = dc.text_color;
            dc.ops.push(DrawOp::TextOut(x, y, String::from(text), color));
            self.stats.text_outs += 1;
        }
    }

    /// Flush draw ops (returns them for Aether rendering).
    pub fn flush_ops(&mut self, hdc: HGdi) -> Vec<DrawOp> {
        if let Some(GdiObject::Dc(dc)) = self.objects.get_mut(&hdc) {
            let ops = dc.ops.clone();
            dc.ops.clear();
            ops
        } else { Vec::new() }
    }

    fn alloc_handle(&mut self) -> HGdi {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    fn get_pen_color(&self, pen: HGdi) -> u32 {
        if let Some(GdiObject::Pen(p)) = self.objects.get(&pen) { p.color } else { 0 }
    }

    fn get_pen_width(&self, pen: HGdi) -> u32 {
        if let Some(GdiObject::Pen(p)) = self.objects.get(&pen) { p.width } else { 1 }
    }

    fn get_brush_color(&self, brush: HGdi) -> u32 {
        if let Some(GdiObject::Brush(b)) = self.objects.get(&brush) { b.color } else { 0x00FFFFFF }
    }
}
