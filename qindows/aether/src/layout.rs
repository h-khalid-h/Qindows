//! # Aether Layout Engine
//!
//! Flexbox-inspired layout system for the Qindows UI.
//! Every widget declares its constraints, and the layout engine
//! resolves positions and sizes in a single pass (top-down measure,
//! bottom-up arrange).

extern crate alloc;

use alloc::vec::Vec;

/// Layout direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Row,
    Column,
}

/// Alignment on the main axis.
#[derive(Debug, Clone, Copy)]
pub enum MainAlign {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Alignment on the cross axis.
#[derive(Debug, Clone, Copy)]
pub enum CrossAlign {
    Start,
    Center,
    End,
    Stretch,
}

/// Size constraint.
#[derive(Debug, Clone, Copy)]
pub enum Size {
    /// Fixed pixel size
    Fixed(f32),
    /// Percentage of parent
    Percent(f32),
    /// Flex grow factor
    Flex(f32),
    /// Size to fit content
    FitContent,
    /// Fill remaining space
    Fill,
}

/// Edge insets (padding / margin).
#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeInsets {
    pub const fn all(v: f32) -> Self {
        EdgeInsets { top: v, right: v, bottom: v, left: v }
    }
    pub const fn symmetric(h: f32, v: f32) -> Self {
        EdgeInsets { top: v, right: h, bottom: v, left: h }
    }
    pub fn horizontal(&self) -> f32 { self.left + self.right }
    pub fn vertical(&self) -> f32 { self.top + self.bottom }
}

/// Layout constraints passed down from parent.
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    pub fn tight(width: f32, height: f32) -> Self {
        Constraints {
            min_width: width, max_width: width,
            min_height: height, max_height: height,
        }
    }

    pub fn loose(max_width: f32, max_height: f32) -> Self {
        Constraints {
            min_width: 0.0, max_width,
            min_height: 0.0, max_height,
        }
    }

    pub fn clamp_width(&self, w: f32) -> f32 {
        w.max(self.min_width).min(self.max_width)
    }

    pub fn clamp_height(&self, h: f32) -> f32 {
        h.max(self.min_height).min(self.max_height)
    }
}

/// A resolved layout rectangle.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl LayoutRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// A layout node in the tree.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    /// Unique ID
    pub id: u64,
    /// Width constraint
    pub width: Size,
    /// Height constraint
    pub height: Size,
    /// Layout direction (for container nodes)
    pub direction: Direction,
    /// Main axis alignment
    pub main_align: MainAlign,
    /// Cross axis alignment
    pub cross_align: CrossAlign,
    /// Padding
    pub padding: EdgeInsets,
    /// Margin
    pub margin: EdgeInsets,
    /// Gap between children
    pub gap: f32,
    /// Children
    pub children: Vec<LayoutNode>,
    /// Resolved layout (filled by solve())
    pub rect: LayoutRect,
    /// Min intrinsic size
    pub min_size: (f32, f32),
}

impl LayoutNode {
    pub fn new(id: u64) -> Self {
        LayoutNode {
            id,
            width: Size::FitContent,
            height: Size::FitContent,
            direction: Direction::Column,
            main_align: MainAlign::Start,
            cross_align: CrossAlign::Stretch,
            padding: EdgeInsets::default(),
            margin: EdgeInsets::default(),
            gap: 0.0,
            children: Vec::new(),
            rect: LayoutRect::default(),
            min_size: (0.0, 0.0),
        }
    }

    /// Solve layout for this node and all children.
    pub fn solve(&mut self, constraints: Constraints, x: f32, y: f32) {
        // Step 1: determine own size
        let content_max_w = constraints.max_width - self.padding.horizontal() - self.margin.horizontal();
        let content_max_h = constraints.max_height - self.padding.vertical() - self.margin.vertical();

        let self_width = match self.width {
            Size::Fixed(w) => constraints.clamp_width(w),
            Size::Percent(p) => constraints.clamp_width(constraints.max_width * p / 100.0),
            Size::Fill => constraints.max_width - self.margin.horizontal(),
            _ => content_max_w + self.padding.horizontal(),
        };

        let self_height = match self.height {
            Size::Fixed(h) => constraints.clamp_height(h),
            Size::Percent(p) => constraints.clamp_height(constraints.max_height * p / 100.0),
            Size::Fill => constraints.max_height - self.margin.vertical(),
            _ => content_max_h + self.padding.vertical(),
        };

        self.rect = LayoutRect {
            x: x + self.margin.left,
            y: y + self.margin.top,
            width: self_width,
            height: self_height,
        };

        if self.children.is_empty() { return; }

        // Step 2: measure children and distribute space
        let inner_x = self.rect.x + self.padding.left;
        let inner_y = self.rect.y + self.padding.top;
        let inner_w = self_width - self.padding.horizontal();
        let inner_h = self_height - self.padding.vertical();

        let total_gap = self.gap * (self.children.len().saturating_sub(1)) as f32;
        let is_row = self.direction == Direction::Row;

        // Calculate flex total and fixed sizes
        let mut flex_total: f32 = 0.0;
        let mut fixed_total: f32 = 0.0;

        for child in &self.children {
            let child_main = if is_row { &child.width } else { &child.height };
            match child_main {
                Size::Fixed(v) => fixed_total += v + if is_row { child.margin.horizontal() } else { child.margin.vertical() },
                Size::Flex(f) => flex_total += f,
                _ => {}
            }
        }

        let available_for_flex = if is_row {
            (inner_w - fixed_total - total_gap).max(0.0)
        } else {
            (inner_h - fixed_total - total_gap).max(0.0)
        };

        // Step 3: position children
        let mut offset: f32 = 0.0;

        for child in &mut self.children {
            let child_main_size = if is_row { &child.width } else { &child.height };
            let main_size = match child_main_size {
                Size::Fixed(v) => *v,
                Size::Flex(f) => if flex_total > 0.0 { available_for_flex * f / flex_total } else { 0.0 },
                Size::Fill => available_for_flex,
                Size::Percent(p) => if is_row { inner_w * p / 100.0 } else { inner_h * p / 100.0 },
                Size::FitContent => if is_row { 100.0 } else { 30.0 }, // Default fallback
            };

            let cross_size = if is_row { inner_h } else { inner_w };

            let (child_x, child_y) = if is_row {
                (inner_x + offset, inner_y)
            } else {
                (inner_x, inner_y + offset)
            };

            let child_constraints = if is_row {
                Constraints { min_width: 0.0, max_width: main_size, min_height: 0.0, max_height: cross_size }
            } else {
                Constraints { min_width: 0.0, max_width: cross_size, min_height: 0.0, max_height: main_size }
            };

            child.solve(child_constraints, child_x, child_y);
            offset += main_size + self.gap;
        }
    }

    /// Hit-test: find the deepest node at point (px, py).
    pub fn hit_test(&self, px: f32, py: f32) -> Option<u64> {
        if !self.rect.contains(px, py) {
            return None;
        }

        // Check children (deepest first = last child)
        for child in self.children.iter().rev() {
            if let Some(id) = child.hit_test(px, py) {
                return Some(id);
            }
        }

        Some(self.id)
    }
}
