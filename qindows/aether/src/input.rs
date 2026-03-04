//! # Aether Input Routing
//!
//! Routes keyboard, mouse, and touch events from the Qernel
//! to the correct window/Silo. Implements the focus model,
//! global hotkeys, and gesture recognition.

extern crate alloc;

use alloc::vec::Vec;

/// Input event types from hardware drivers.
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    /// Key press/release
    Key {
        scancode: u16,
        pressed: bool,
        modifiers: Modifiers,
    },
    /// Mouse movement (absolute coordinates)
    MouseMove { x: f32, y: f32 },
    /// Mouse button press/release
    MouseButton {
        button: MouseBtn,
        pressed: bool,
        x: f32,
        y: f32,
    },
    /// Mouse wheel scroll
    MouseScroll { dx: f32, dy: f32 },
    /// Touch point (finger/stylus)
    Touch {
        id: u32,
        phase: TouchPhase,
        x: f32,
        y: f32,
    },
}

/// Keyboard modifier flags
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool, // "Q-key" in Qindows
}

/// Mouse buttons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseBtn {
    Left,
    Right,
    Middle,
    Extra1,
    Extra2,
}

/// Touch event phases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// Global hotkey binding
#[derive(Debug)]
pub struct Hotkey {
    pub scancode: u16,
    pub modifiers: Modifiers,
    pub action: HotkeyAction,
}

/// Actions triggered by global hotkeys
#[derive(Debug, Clone, Copy)]
pub enum HotkeyAction {
    /// Q+Tab — switch windows (Alt-Tab equivalent)
    SwitchWindow,
    /// Q+Space — open Q-Shell
    OpenShell,
    /// Q+1..4 — switch Q-Space
    SwitchSpace(u32),
    /// Q+Left/Right — snap window
    SnapLeft,
    SnapRight,
    /// Q+Up — maximize
    Maximize,
    /// Q+Down — restore/minimize
    MinimizeRestore,
    /// Q+L — lock screen
    LockScreen,
    /// Q+D — show desktop
    ShowDesktop,
    /// Q+Shift+S — screenshot
    Screenshot,
    /// Ctrl+Q — close window
    CloseWindow,
}

/// The input router — dispatches events to windows or system actions.
pub struct InputRouter {
    /// Registered global hotkeys
    pub hotkeys: Vec<Hotkey>,
    /// Current mouse position
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// Whether the cursor is visible
    pub cursor_visible: bool,
    /// Active touch points
    pub active_touches: Vec<(u32, f32, f32)>,
}

impl InputRouter {
    pub fn new() -> Self {
        let mut router = InputRouter {
            hotkeys: Vec::new(),
            cursor_x: 0.0,
            cursor_y: 0.0,
            cursor_visible: true,
            active_touches: Vec::new(),
        };

        // Register default hotkeys
        router.register_defaults();
        router
    }

    /// Register the default Qindows hotkeys.
    fn register_defaults(&mut self) {
        // Q+Space = Open Q-Shell
        self.hotkeys.push(Hotkey {
            scancode: 0x39, // Space
            modifiers: Modifiers { meta: true, ..Default::default() },
            action: HotkeyAction::OpenShell,
        });

        // Q+Tab = Switch windows
        self.hotkeys.push(Hotkey {
            scancode: 0x0F, // Tab
            modifiers: Modifiers { meta: true, ..Default::default() },
            action: HotkeyAction::SwitchWindow,
        });

        // Q+L = Lock screen
        self.hotkeys.push(Hotkey {
            scancode: 0x26, // L
            modifiers: Modifiers { meta: true, ..Default::default() },
            action: HotkeyAction::LockScreen,
        });

        // Q+1..4 = Switch Q-Space
        for i in 0..4u32 {
            self.hotkeys.push(Hotkey {
                scancode: 0x02 + i as u16, // 1-4
                modifiers: Modifiers { meta: true, ..Default::default() },
                action: HotkeyAction::SwitchSpace(i),
            });
        }
    }

    /// Process an input event and determine routing.
    ///
    /// Returns either:
    /// - `Routed(WindowId)`: send the event to this window's Silo IPC ring
    /// - `System(HotkeyAction)`: handle as a global system action
    /// - `Consumed`: event was handled internally (cursor update, etc.)
    pub fn route(&mut self, event: &InputEvent) -> InputResult {
        match event {
            InputEvent::Key { scancode, pressed, modifiers } if *pressed => {
                // Check global hotkeys first
                for hotkey in &self.hotkeys {
                    if hotkey.scancode == *scancode
                        && hotkey.modifiers.meta == modifiers.meta
                        && hotkey.modifiers.ctrl == modifiers.ctrl
                        && hotkey.modifiers.alt == modifiers.alt
                        && hotkey.modifiers.shift == modifiers.shift
                    {
                        return InputResult::System(hotkey.action);
                    }
                }
                // Route to focused window
                InputResult::RouteToFocused
            }
            InputEvent::Key { .. } => InputResult::RouteToFocused,

            InputEvent::MouseMove { x, y } => {
                self.cursor_x = *x;
                self.cursor_y = *y;
                InputResult::RouteToFocused
            }
            InputEvent::MouseButton { x, y, pressed, button } => {
                self.cursor_x = *x;
                self.cursor_y = *y;
                if *pressed && *button == MouseBtn::Left {
                    // Click — route to window under cursor (may change focus)
                    InputResult::RouteToPoint(*x, *y)
                } else {
                    InputResult::RouteToFocused
                }
            }
            InputEvent::MouseScroll { .. } => InputResult::RouteToFocused,

            InputEvent::Touch { id, phase, x, y } => {
                match phase {
                    TouchPhase::Start => {
                        self.active_touches.push((*id, *x, *y));
                    }
                    TouchPhase::Move => {
                        if let Some(t) = self.active_touches.iter_mut().find(|t| t.0 == *id) {
                            t.1 = *x;
                            t.2 = *y;
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel => {
                        self.active_touches.retain(|t| t.0 != *id);
                    }
                }
                InputResult::RouteToPoint(*x, *y)
            }
        }
    }
}

/// Result of input routing.
#[derive(Debug)]
pub enum InputResult {
    /// Send to the currently focused window
    RouteToFocused,
    /// Send to the window at this screen point (may change focus)
    RouteToPoint(f32, f32),
    /// Handle as a global system action
    System(HotkeyAction),
    /// Event consumed internally
    Consumed,
}
