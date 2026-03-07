//! # Aether Widget Toolkit
//!
//! Primitive UI widgets for Aether's rendering pipeline.
//! Provides Button, Label, TextInput, Checkbox, Slider, ProgressBar,
//! and Container with layout, focus, and event handling.

extern crate alloc;

use alloc::string::String;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::vec::Vec;

/// Widget unique identifier.
pub type WidgetId = u64;

/// Widget visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden,
    Collapsed, // Hidden and takes no space
}

/// Widget bounds.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Bounds {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Bounds { x, y, width: w, height: h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width as i32
            && py >= self.y && py < self.y + self.height as i32
    }
}

/// Widget events.
#[derive(Debug, Clone)]
pub enum WidgetEvent {
    Click(i32, i32),
    Hover(i32, i32),
    KeyPress(u8),
    TextInput(char),
    FocusGained,
    FocusLost,
    ValueChanged(String),
}

/// Common widget properties.
#[derive(Debug, Clone)]
pub struct WidgetProps {
    pub id: WidgetId,
    pub bounds: Bounds,
    pub visibility: Visibility,
    pub enabled: bool,
    pub focused: bool,
    pub tooltip: Option<String>,
}

impl WidgetProps {
    pub fn new(id: WidgetId, bounds: Bounds) -> Self {
        WidgetProps {
            id, bounds,
            visibility: Visibility::Visible,
            enabled: true,
            focused: false,
            tooltip: None,
        }
    }
}

/// A Button widget.
#[derive(Debug, Clone)]
pub struct Button {
    pub props: WidgetProps,
    pub label: String,
    pub pressed: bool,
    pub hover: bool,
    pub icon: Option<String>,
    pub variant: ButtonVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Outline,
    Ghost,
    Danger,
}

impl Button {
    pub fn new(id: WidgetId, label: &str, bounds: Bounds) -> Self {
        Button {
            props: WidgetProps::new(id, bounds),
            label: String::from(label),
            pressed: false,
            hover: false,
            icon: None,
            variant: ButtonVariant::Primary,
        }
    }

    pub fn handle_event(&mut self, event: &WidgetEvent) -> Option<WidgetEvent> {
        match event {
            WidgetEvent::Click(x, y) if self.props.enabled && self.props.bounds.contains(*x, *y) => {
                self.pressed = true;
                Some(WidgetEvent::Click(*x, *y))
            }
            WidgetEvent::Hover(x, y) => {
                self.hover = self.props.bounds.contains(*x, *y);
                None
            }
            _ => None,
        }
    }
}

/// A Label widget.
#[derive(Debug, Clone)]
pub struct Label {
    pub props: WidgetProps,
    pub text: String,
    pub font_size: u8,
    pub bold: bool,
    pub color: u32,
    pub alignment: TextAlign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl Label {
    pub fn new(id: WidgetId, text: &str, bounds: Bounds) -> Self {
        Label {
            props: WidgetProps::new(id, bounds),
            text: String::from(text),
            font_size: 14,
            bold: false,
            color: 0xFFFFFFFF,
            alignment: TextAlign::Left,
        }
    }
}

/// A TextInput widget.
#[derive(Debug, Clone)]
pub struct TextInput {
    pub props: WidgetProps,
    pub value: String,
    pub placeholder: String,
    pub cursor_pos: usize,
    pub selection: Option<(usize, usize)>,
    pub max_length: usize,
    pub password: bool,
}

impl TextInput {
    pub fn new(id: WidgetId, bounds: Bounds) -> Self {
        TextInput {
            props: WidgetProps::new(id, bounds),
            value: String::new(),
            placeholder: String::new(),
            cursor_pos: 0,
            selection: None,
            max_length: 256,
            password: false,
        }
    }

    pub fn handle_event(&mut self, event: &WidgetEvent) -> Option<WidgetEvent> {
        if !self.props.enabled || !self.props.focused { return None; }

        match event {
            WidgetEvent::TextInput(ch) => {
                if self.value.chars().count() < self.max_length {
                    // Convert char position to byte offset
                    let byte_pos = self.value.char_indices()
                        .nth(self.cursor_pos)
                        .map(|(i, _)| i)
                        .unwrap_or(self.value.len());
                    self.value.insert(byte_pos, *ch);
                    self.cursor_pos += 1;
                    return Some(WidgetEvent::ValueChanged(self.value.clone()));
                }
                None
            }
            WidgetEvent::KeyPress(8) => { // Backspace
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    let byte_pos = self.value.char_indices()
                        .nth(self.cursor_pos)
                        .map(|(i, _)| i)
                        .unwrap_or(self.value.len());
                    self.value.remove(byte_pos);
                    return Some(WidgetEvent::ValueChanged(self.value.clone()));
                }
                None
            }
            _ => None,
        }
    }
}

/// A Checkbox widget.
#[derive(Debug, Clone)]
pub struct Checkbox {
    pub props: WidgetProps,
    pub checked: bool,
    pub label: String,
    pub indeterminate: bool,
}

impl Checkbox {
    pub fn new(id: WidgetId, label: &str, bounds: Bounds) -> Self {
        Checkbox {
            props: WidgetProps::new(id, bounds),
            checked: false,
            label: String::from(label),
            indeterminate: false,
        }
    }

    pub fn toggle(&mut self) {
        self.checked = !self.checked;
        self.indeterminate = false;
    }
}

/// A Slider widget.
#[derive(Debug, Clone)]
pub struct Slider {
    pub props: WidgetProps,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub step: f32,
    pub dragging: bool,
}

impl Slider {
    pub fn new(id: WidgetId, min: f32, max: f32, bounds: Bounds) -> Self {
        Slider {
            props: WidgetProps::new(id, bounds),
            min, max, value: min, step: 1.0,
            dragging: false,
        }
    }

    pub fn set_value(&mut self, val: f32) {
        self.value = val.max(self.min).min(self.max);
        // Snap to step
        if self.step > 0.0 {
            self.value = ((self.value - self.min) / self.step).round() * self.step + self.min;
        }
    }

    pub fn fraction(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON { return 0.0; }
        (self.value - self.min) / (self.max - self.min)
    }
}

/// A ProgressBar widget.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    pub props: WidgetProps,
    pub value: f32,      // 0.0 to 1.0
    pub indeterminate: bool,
    pub label: Option<String>,
    pub color: u32,
}

impl ProgressBar {
    pub fn new(id: WidgetId, bounds: Bounds) -> Self {
        ProgressBar {
            props: WidgetProps::new(id, bounds),
            value: 0.0,
            indeterminate: false,
            label: None,
            color: 0xFF0078D7,
        }
    }

    pub fn set(&mut self, value: f32) {
        self.value = value.max(0.0).min(1.0);
    }
}

/// A Container widget (holds children).
#[derive(Debug, Clone)]
pub struct Container {
    pub props: WidgetProps,
    pub children: Vec<WidgetId>,
    pub layout: ContainerLayout,
    pub padding: u16,
    pub gap: u16,
    pub scroll_y: i32,
    pub bg_color: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerLayout {
    Vertical,
    Horizontal,
    Stack, // Overlay children
}

impl Container {
    pub fn new(id: WidgetId, bounds: Bounds, layout: ContainerLayout) -> Self {
        Container {
            props: WidgetProps::new(id, bounds),
            children: Vec::new(),
            layout,
            padding: 8,
            gap: 4,
            scroll_y: 0,
            bg_color: None,
        }
    }

    pub fn add_child(&mut self, child_id: WidgetId) {
        self.children.push(child_id);
    }
}
