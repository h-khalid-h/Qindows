//! # PS/2 Keyboard Driver
//!
//! Handles keyboard input from the PS/2 controller.
//! Translates scan codes to key events, manages modifier state,
//! and feeds the Aether input pipeline.

use alloc::collections::VecDeque;
use spin::Mutex;

/// Keyboard event
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    /// Scan code (Set 1)
    pub scancode: u8,
    /// Virtual key code (translated)
    pub keycode: KeyCode,
    /// Was the key pressed (true) or released (false)?
    pub pressed: bool,
    /// Current modifier state at the time of this event
    pub modifiers: ModifierState,
}

/// Modifier key state
#[derive(Debug, Clone, Copy, Default)]
pub struct ModifierState {
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_alt: bool,
    pub right_alt: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
    pub meta: bool, // Q-key (Windows key)
}

impl ModifierState {
    pub fn shift(&self) -> bool {
        self.left_shift || self.right_shift
    }
    pub fn ctrl(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }
    pub fn alt(&self) -> bool {
        self.left_alt || self.right_alt
    }
}

/// Virtual key codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Numbers
    Num0, Num1, Num2, Num3, Num4,
    Num5, Num6, Num7, Num8, Num9,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Special keys
    Escape, Tab, CapsLock, Enter, Backspace, Space,
    LeftShift, RightShift, LeftCtrl, RightCtrl,
    LeftAlt, RightAlt, Meta,
    // Navigation
    Up, Down, Left, Right,
    Home, End, PageUp, PageDown,
    Insert, Delete,
    // Punctuation
    Minus, Equals, LeftBracket, RightBracket,
    Semicolon, Quote, Backslash, Comma, Period, Slash,
    Backtick,
    // Unknown
    Unknown(u8),
}

/// Translate scancode (Set 1) to KeyCode.
fn scancode_to_keycode(scancode: u8) -> KeyCode {
    match scancode & 0x7F {
        0x01 => KeyCode::Escape,
        0x02 => KeyCode::Num1, 0x03 => KeyCode::Num2, 0x04 => KeyCode::Num3,
        0x05 => KeyCode::Num4, 0x06 => KeyCode::Num5, 0x07 => KeyCode::Num6,
        0x08 => KeyCode::Num7, 0x09 => KeyCode::Num8, 0x0A => KeyCode::Num9,
        0x0B => KeyCode::Num0,
        0x0C => KeyCode::Minus, 0x0D => KeyCode::Equals,
        0x0E => KeyCode::Backspace, 0x0F => KeyCode::Tab,
        0x10 => KeyCode::Q, 0x11 => KeyCode::W, 0x12 => KeyCode::E,
        0x13 => KeyCode::R, 0x14 => KeyCode::T, 0x15 => KeyCode::Y,
        0x16 => KeyCode::U, 0x17 => KeyCode::I, 0x18 => KeyCode::O,
        0x19 => KeyCode::P,
        0x1A => KeyCode::LeftBracket, 0x1B => KeyCode::RightBracket,
        0x1C => KeyCode::Enter,
        0x1D => KeyCode::LeftCtrl,
        0x1E => KeyCode::A, 0x1F => KeyCode::S, 0x20 => KeyCode::D,
        0x21 => KeyCode::F, 0x22 => KeyCode::G, 0x23 => KeyCode::H,
        0x24 => KeyCode::J, 0x25 => KeyCode::K, 0x26 => KeyCode::L,
        0x27 => KeyCode::Semicolon, 0x28 => KeyCode::Quote,
        0x29 => KeyCode::Backtick,
        0x2A => KeyCode::LeftShift, 0x2B => KeyCode::Backslash,
        0x2C => KeyCode::Z, 0x2D => KeyCode::X, 0x2E => KeyCode::C,
        0x2F => KeyCode::V, 0x30 => KeyCode::B, 0x31 => KeyCode::N,
        0x32 => KeyCode::M,
        0x33 => KeyCode::Comma, 0x34 => KeyCode::Period, 0x35 => KeyCode::Slash,
        0x36 => KeyCode::RightShift,
        0x38 => KeyCode::LeftAlt,
        0x39 => KeyCode::Space,
        0x3A => KeyCode::CapsLock,
        0x3B => KeyCode::F1, 0x3C => KeyCode::F2, 0x3D => KeyCode::F3,
        0x3E => KeyCode::F4, 0x3F => KeyCode::F5, 0x40 => KeyCode::F6,
        0x41 => KeyCode::F7, 0x42 => KeyCode::F8, 0x43 => KeyCode::F9,
        0x44 => KeyCode::F10,
        0x57 => KeyCode::F11, 0x58 => KeyCode::F12,
        0x48 => KeyCode::Up, 0x50 => KeyCode::Down,
        0x4B => KeyCode::Left, 0x4D => KeyCode::Right,
        0x47 => KeyCode::Home, 0x4F => KeyCode::End,
        0x49 => KeyCode::PageUp, 0x51 => KeyCode::PageDown,
        0x52 => KeyCode::Insert, 0x53 => KeyCode::Delete,
        code => KeyCode::Unknown(code),
    }
}

/// Convert a keycode to its ASCII character (if printable).
pub fn keycode_to_char(code: KeyCode, shifted: bool) -> Option<char> {
    match (code, shifted) {
        (KeyCode::A, false) => Some('a'), (KeyCode::A, true) => Some('A'),
        (KeyCode::B, false) => Some('b'), (KeyCode::B, true) => Some('B'),
        (KeyCode::C, false) => Some('c'), (KeyCode::C, true) => Some('C'),
        (KeyCode::D, false) => Some('d'), (KeyCode::D, true) => Some('D'),
        (KeyCode::E, false) => Some('e'), (KeyCode::E, true) => Some('E'),
        (KeyCode::F, false) => Some('f'), (KeyCode::F, true) => Some('F'),
        (KeyCode::G, false) => Some('g'), (KeyCode::G, true) => Some('G'),
        (KeyCode::H, false) => Some('h'), (KeyCode::H, true) => Some('H'),
        (KeyCode::I, false) => Some('i'), (KeyCode::I, true) => Some('I'),
        (KeyCode::J, false) => Some('j'), (KeyCode::J, true) => Some('J'),
        (KeyCode::K, false) => Some('k'), (KeyCode::K, true) => Some('K'),
        (KeyCode::L, false) => Some('l'), (KeyCode::L, true) => Some('L'),
        (KeyCode::M, false) => Some('m'), (KeyCode::M, true) => Some('M'),
        (KeyCode::N, false) => Some('n'), (KeyCode::N, true) => Some('N'),
        (KeyCode::O, false) => Some('o'), (KeyCode::O, true) => Some('O'),
        (KeyCode::P, false) => Some('p'), (KeyCode::P, true) => Some('P'),
        (KeyCode::Q, false) => Some('q'), (KeyCode::Q, true) => Some('Q'),
        (KeyCode::R, false) => Some('r'), (KeyCode::R, true) => Some('R'),
        (KeyCode::S, false) => Some('s'), (KeyCode::S, true) => Some('S'),
        (KeyCode::T, false) => Some('t'), (KeyCode::T, true) => Some('T'),
        (KeyCode::U, false) => Some('u'), (KeyCode::U, true) => Some('U'),
        (KeyCode::V, false) => Some('v'), (KeyCode::V, true) => Some('V'),
        (KeyCode::W, false) => Some('w'), (KeyCode::W, true) => Some('W'),
        (KeyCode::X, false) => Some('x'), (KeyCode::X, true) => Some('X'),
        (KeyCode::Y, false) => Some('y'), (KeyCode::Y, true) => Some('Y'),
        (KeyCode::Z, false) => Some('z'), (KeyCode::Z, true) => Some('Z'),
        (KeyCode::Num0, false) => Some('0'), (KeyCode::Num0, true) => Some(')'),
        (KeyCode::Num1, false) => Some('1'), (KeyCode::Num1, true) => Some('!'),
        (KeyCode::Num2, false) => Some('2'), (KeyCode::Num2, true) => Some('@'),
        (KeyCode::Num3, false) => Some('3'), (KeyCode::Num3, true) => Some('#'),
        (KeyCode::Num4, false) => Some('4'), (KeyCode::Num4, true) => Some('$'),
        (KeyCode::Num5, false) => Some('5'), (KeyCode::Num5, true) => Some('%'),
        (KeyCode::Num6, false) => Some('6'), (KeyCode::Num6, true) => Some('^'),
        (KeyCode::Num7, false) => Some('7'), (KeyCode::Num7, true) => Some('&'),
        (KeyCode::Num8, false) => Some('8'), (KeyCode::Num8, true) => Some('*'),
        (KeyCode::Num9, false) => Some('9'), (KeyCode::Num9, true) => Some('('),
        (KeyCode::Space, _) => Some(' '),
        (KeyCode::Enter, _) => Some('\n'),
        (KeyCode::Tab, _) => Some('\t'),
        (KeyCode::Minus, false) => Some('-'), (KeyCode::Minus, true) => Some('_'),
        (KeyCode::Equals, false) => Some('='), (KeyCode::Equals, true) => Some('+'),
        (KeyCode::Period, false) => Some('.'), (KeyCode::Period, true) => Some('>'),
        (KeyCode::Comma, false) => Some(','), (KeyCode::Comma, true) => Some('<'),
        (KeyCode::Slash, false) => Some('/'), (KeyCode::Slash, true) => Some('?'),
        (KeyCode::Semicolon, false) => Some(';'), (KeyCode::Semicolon, true) => Some(':'),
        (KeyCode::Quote, false) => Some('\''), (KeyCode::Quote, true) => Some('"'),
        (KeyCode::LeftBracket, false) => Some('['), (KeyCode::LeftBracket, true) => Some('{'),
        (KeyCode::RightBracket, false) => Some(']'), (KeyCode::RightBracket, true) => Some('}'),
        (KeyCode::Backslash, false) => Some('\\'), (KeyCode::Backslash, true) => Some('|'),
        (KeyCode::Backtick, false) => Some('`'), (KeyCode::Backtick, true) => Some('~'),
        _ => None,
    }
}

/// Global keyboard event buffer.
static KEYBOARD_BUFFER: Mutex<VecDeque<KeyEvent>> = Mutex::new(VecDeque::new());

/// Global modifier state.
static MODIFIER_STATE: Mutex<ModifierState> = Mutex::new(ModifierState {
    left_shift: false, right_shift: false,
    left_ctrl: false, right_ctrl: false,
    left_alt: false, right_alt: false,
    caps_lock: false, num_lock: false, scroll_lock: false,
    meta: false,
});

/// Called from the keyboard interrupt handler (IRQ 1 / vector 33).
///
/// Reads the scancode from port 0x60, translates it, updates
/// modifier state, and pushes a KeyEvent into the buffer.
pub fn handle_scancode(scancode: u8) {
    let pressed = scancode & 0x80 == 0;
    let keycode = scancode_to_keycode(scancode);

    // Update modifier state
    let mut mods = MODIFIER_STATE.lock();
    match keycode {
        KeyCode::LeftShift => mods.left_shift = pressed,
        KeyCode::RightShift => mods.right_shift = pressed,
        KeyCode::LeftCtrl => mods.left_ctrl = pressed,
        KeyCode::RightCtrl => mods.right_ctrl = pressed,
        KeyCode::LeftAlt => mods.left_alt = pressed,
        KeyCode::RightAlt => mods.right_alt = pressed,
        KeyCode::Meta => mods.meta = pressed,
        KeyCode::CapsLock if pressed => mods.caps_lock = !mods.caps_lock,
        _ => {}
    }

    let event = KeyEvent {
        scancode,
        keycode,
        pressed,
        modifiers: *mods,
    };
    drop(mods);

    let mut buffer = KEYBOARD_BUFFER.lock();
    if buffer.len() < 256 {
        buffer.push_back(event);
    }
}

/// Pop the next key event from the buffer.
pub fn poll_key() -> Option<KeyEvent> {
    KEYBOARD_BUFFER.lock().pop_front()
}

/// Check if a key is currently held.
pub fn is_key_pressed(code: KeyCode) -> bool {
    // Check the last few events for currently-held state
    // This is simplified — production would use a full key state bitmap
    let buffer = KEYBOARD_BUFFER.lock();
    buffer.iter().rev().find(|e| e.keycode == code).map_or(false, |e| e.pressed)
}
