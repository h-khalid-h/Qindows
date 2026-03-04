//! # Aether Widget Toolkit
//!
//! Pre-built UI components for Qindows apps.
//! Each widget knows how to measure itself, lay itself out,
//! handle input, and produce render commands.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::renderer::{Color, RenderCommand, RenderFrame};

/// Widget ID (unique within a window).
pub type WidgetId = u64;

/// Common widget state.
#[derive(Debug, Clone, Copy, Default)]
pub struct WidgetState {
    pub hovered: bool,
    pub focused: bool,
    pub pressed: bool,
    pub disabled: bool,
    pub visible: bool,
}

impl WidgetState {
    pub fn normal() -> Self {
        WidgetState { visible: true, ..Default::default() }
    }
}

/// UI events that widgets can emit.
#[derive(Debug, Clone)]
pub enum WidgetEvent {
    Clicked(WidgetId),
    TextChanged(WidgetId, String),
    ValueChanged(WidgetId, f32),
    Toggled(WidgetId, bool),
    Selected(WidgetId, usize),
    Submitted(WidgetId),
}

/// A text label widget.
#[derive(Debug, Clone)]
pub struct Label {
    pub id: WidgetId,
    pub text: String,
    pub size: f32,
    pub color: Color,
    pub bold: bool,
    pub state: WidgetState,
}

impl Label {
    pub fn new(id: WidgetId, text: &str) -> Self {
        Label {
            id,
            text: String::from(text),
            size: 14.0,
            color: Color::rgb(1.0, 1.0, 1.0),
            bold: false,
            state: WidgetState::normal(),
        }
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }
        frame.push(RenderCommand::Text {
            x, y,
            text: self.text.clone(),
            size: self.size,
            color: self.color,
        });
    }
}

/// A clickable button.
#[derive(Debug, Clone)]
pub struct Button {
    pub id: WidgetId,
    pub label: String,
    pub width: f32,
    pub height: f32,
    pub accent: bool,
    pub state: WidgetState,
}

impl Button {
    pub fn new(id: WidgetId, label: &str) -> Self {
        Button {
            id,
            label: String::from(label),
            width: 120.0,
            height: 36.0,
            accent: false,
            state: WidgetState::normal(),
        }
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }
        frame.draw_button(x, y, self.width, self.height, &self.label,
                          self.state.hovered, self.state.pressed);
    }

    pub fn handle_click(&self) -> Option<WidgetEvent> {
        if self.state.disabled { return None; }
        Some(WidgetEvent::Clicked(self.id))
    }
}

/// A text input field.
#[derive(Debug, Clone)]
pub struct TextInput {
    pub id: WidgetId,
    pub value: String,
    pub placeholder: String,
    pub width: f32,
    pub cursor_pos: usize,
    pub max_length: usize,
    pub password: bool,
    pub state: WidgetState,
}

impl TextInput {
    pub fn new(id: WidgetId, placeholder: &str) -> Self {
        TextInput {
            id,
            value: String::new(),
            placeholder: String::from(placeholder),
            width: 200.0,
            cursor_pos: 0,
            max_length: 256,
            password: false,
            state: WidgetState::normal(),
        }
    }

    pub fn insert_char(&mut self, c: char) -> Option<WidgetEvent> {
        if self.value.len() >= self.max_length { return None; }
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
        Some(WidgetEvent::TextChanged(self.id, self.value.clone()))
    }

    pub fn backspace(&mut self) -> Option<WidgetEvent> {
        if self.cursor_pos == 0 { return None; }
        self.cursor_pos -= 1;
        self.value.remove(self.cursor_pos);
        Some(WidgetEvent::TextChanged(self.id, self.value.clone()))
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }

        let border_color = if self.state.focused {
            Color::rgba(0.024, 0.839, 0.627, 0.8)
        } else {
            Color::rgba(0.3, 0.3, 0.35, 0.6)
        };

        frame.push(RenderCommand::RoundedRect {
            x, y, width: self.width, height: 36.0,
            radius: 6.0,
            fill: Color::rgba(0.1, 0.1, 0.12, 0.9),
            border: Some((1.5, border_color)),
        });

        let display_text = if self.value.is_empty() {
            &self.placeholder
        } else if self.password {
            &String::from("•".repeat(self.value.len()))
        } else {
            &self.value
        };

        let text_color = if self.value.is_empty() {
            Color::rgba(0.5, 0.5, 0.5, 0.6)
        } else {
            Color::rgb(1.0, 1.0, 1.0)
        };

        frame.push(RenderCommand::Text {
            x: x + 12.0, y: y + 22.0,
            text: display_text.clone(),
            size: 14.0,
            color: text_color,
        });
    }
}

/// A toggle switch (on/off).
#[derive(Debug, Clone)]
pub struct Toggle {
    pub id: WidgetId,
    pub checked: bool,
    pub label: String,
    pub state: WidgetState,
}

impl Toggle {
    pub fn new(id: WidgetId, label: &str) -> Self {
        Toggle {
            id,
            checked: false,
            label: String::from(label),
            state: WidgetState::normal(),
        }
    }

    pub fn toggle(&mut self) -> Option<WidgetEvent> {
        if self.state.disabled { return None; }
        self.checked = !self.checked;
        Some(WidgetEvent::Toggled(self.id, self.checked))
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }

        let track_color = if self.checked {
            Color::rgba(0.024, 0.839, 0.627, 0.8)
        } else {
            Color::rgba(0.3, 0.3, 0.35, 0.5)
        };

        // Track
        frame.push(RenderCommand::RoundedRect {
            x, y: y + 3.0, width: 40.0, height: 20.0,
            radius: 10.0,
            fill: track_color,
            border: None,
        });

        // Thumb
        let thumb_x = if self.checked { x + 22.0 } else { x + 2.0 };
        frame.push(RenderCommand::Circle {
            cx: thumb_x + 8.0, cy: y + 13.0,
            radius: 8.0,
            fill: Color::rgb(1.0, 1.0, 1.0),
        });

        // Label
        frame.push(RenderCommand::Text {
            x: x + 48.0, y: y + 17.0,
            text: self.label.clone(),
            size: 14.0,
            color: Color::rgb(0.9, 0.9, 0.9),
        });
    }
}

/// A slider (range input).
#[derive(Debug, Clone)]
pub struct Slider {
    pub id: WidgetId,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub width: f32,
    pub label: String,
    pub state: WidgetState,
}

impl Slider {
    pub fn new(id: WidgetId, min: f32, max: f32) -> Self {
        Slider {
            id,
            value: min,
            min, max,
            width: 200.0,
            label: String::new(),
            state: WidgetState::normal(),
        }
    }

    pub fn set_value(&mut self, v: f32) -> Option<WidgetEvent> {
        self.value = v.max(self.min).min(self.max);
        Some(WidgetEvent::ValueChanged(self.id, self.value))
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }

        // Track
        frame.push(RenderCommand::RoundedRect {
            x, y: y + 8.0, width: self.width, height: 4.0,
            radius: 2.0,
            fill: Color::rgba(0.3, 0.3, 0.35, 0.5),
            border: None,
        });

        // Filled portion
        let fill_pct = (self.value - self.min) / (self.max - self.min);
        let fill_w = self.width * fill_pct;
        frame.push(RenderCommand::RoundedRect {
            x, y: y + 8.0, width: fill_w, height: 4.0,
            radius: 2.0,
            fill: Color::rgba(0.024, 0.839, 0.627, 0.8),
            border: None,
        });

        // Thumb
        frame.push(RenderCommand::Circle {
            cx: x + fill_w, cy: y + 10.0,
            radius: 8.0,
            fill: Color::rgb(1.0, 1.0, 1.0),
        });
    }
}

/// A progress bar.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    pub id: WidgetId,
    pub progress: f32, // 0.0 - 1.0
    pub width: f32,
    pub indeterminate: bool,
    pub state: WidgetState,
}

impl ProgressBar {
    pub fn new(id: WidgetId) -> Self {
        ProgressBar {
            id,
            progress: 0.0,
            width: 200.0,
            indeterminate: false,
            state: WidgetState::normal(),
        }
    }

    pub fn render(&self, frame: &mut RenderFrame, x: f32, y: f32) {
        if !self.state.visible { return; }

        // Background
        frame.push(RenderCommand::RoundedRect {
            x, y, width: self.width, height: 6.0,
            radius: 3.0,
            fill: Color::rgba(0.2, 0.2, 0.25, 0.5),
            border: None,
        });

        // Fill
        let fill_w = self.width * self.progress.max(0.0).min(1.0);
        frame.push(RenderCommand::Gradient {
            x, y, width: fill_w, height: 6.0,
            start_color: Color::rgba(0.024, 0.839, 0.627, 0.9),
            end_color: Color::rgba(0.016, 0.576, 0.867, 0.9),
            angle: 90.0,
        });
    }
}
