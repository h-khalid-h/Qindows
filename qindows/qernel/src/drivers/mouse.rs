//! # PS/2 Mouse Driver
//!
//! Handles mouse input from the PS/2 controller (IRQ12).
//! Decodes mouse packets, tracks position, and feeds
//! the Aether input pipeline with cursor events.

use spin::Mutex;

/// Mouse button state.
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub button4: bool,
    pub button5: bool,
}

/// A mouse event.
#[derive(Debug, Clone, Copy)]
pub struct MouseEvent {
    /// X delta (positive = right)
    pub dx: i16,
    /// Y delta (positive = down, inverted from PS/2)
    pub dy: i16,
    /// Scroll wheel delta
    pub scroll: i8,
    /// Button state
    pub buttons: MouseButtons,
}

/// Mouse state machine for packet decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacketPhase {
    /// Waiting for byte 0 (status byte)
    Status,
    /// Waiting for byte 1 (X movement)
    DeltaX,
    /// Waiting for byte 2 (Y movement)
    DeltaY,
    /// Waiting for byte 3 (scroll, if intellimouse)
    Scroll,
}

/// The PS/2 mouse driver state.
pub struct MouseDriver {
    /// Packet decoding phase
    phase: PacketPhase,
    /// Raw packet bytes
    packet: [u8; 4],
    /// Is this an IntelliMouse (has scroll wheel)?
    pub intellimouse: bool,
    /// Absolute cursor position
    pub cursor_x: i32,
    pub cursor_y: i32,
    /// Screen bounds
    pub screen_width: i32,
    pub screen_height: i32,
    /// Button state
    pub buttons: MouseButtons,
    /// Event ring buffer
    events: [Option<MouseEvent>; 128],
    event_head: usize,
    event_tail: usize,
    /// Sensitivity multiplier (1.0 = default)
    pub sensitivity: f32,
}

/// Global mouse driver instance.
static MOUSE: Mutex<Option<MouseDriver>> = Mutex::new(None);

impl MouseDriver {
    pub fn new(screen_width: i32, screen_height: i32) -> Self {
        MouseDriver {
            phase: PacketPhase::Status,
            packet: [0; 4],
            intellimouse: false,
            cursor_x: screen_width / 2,
            cursor_y: screen_height / 2,
            screen_width,
            screen_height,
            buttons: MouseButtons::default(),
            events: [None; 128],
            event_head: 0,
            event_tail: 0,
            sensitivity: 1.0,
        }
    }

    /// Process a raw byte from the PS/2 controller.
    pub fn process_byte(&mut self, byte: u8) {
        match self.phase {
            PacketPhase::Status => {
                // Bit 3 must always be set in a valid status byte
                if byte & 0x08 == 0 {
                    return; // Re-sync
                }
                self.packet[0] = byte;
                self.phase = PacketPhase::DeltaX;
            }
            PacketPhase::DeltaX => {
                self.packet[1] = byte;
                self.phase = PacketPhase::DeltaY;
            }
            PacketPhase::DeltaY => {
                self.packet[2] = byte;
                if self.intellimouse {
                    self.phase = PacketPhase::Scroll;
                } else {
                    self.decode_packet();
                    self.phase = PacketPhase::Status;
                }
            }
            PacketPhase::Scroll => {
                self.packet[3] = byte;
                self.decode_packet();
                self.phase = PacketPhase::Status;
            }
        }
    }

    /// Decode a complete packet into a MouseEvent.
    fn decode_packet(&mut self) {
        let status = self.packet[0];

        // Buttons
        self.buttons.left = status & 0x01 != 0;
        self.buttons.right = status & 0x02 != 0;
        self.buttons.middle = status & 0x04 != 0;

        // X delta (9-bit signed: sign bit in status byte)
        let mut dx = self.packet[1] as i16;
        if status & 0x10 != 0 {
            dx -= 256; // Sign extend
        }

        // Y delta (9-bit signed, inverted — PS/2 Y is up-positive)
        let mut dy = self.packet[2] as i16;
        if status & 0x20 != 0 {
            dy -= 256;
        }
        dy = -dy; // Invert: Qindows Y is down-positive

        // Scroll (4th byte, if intellimouse)
        let scroll = if self.intellimouse {
            self.packet[3] as i8
        } else {
            0
        };

        // Apply sensitivity
        let dx = (dx as f32 * self.sensitivity) as i16;
        let dy = (dy as f32 * self.sensitivity) as i16;

        // Update cursor position (clamped to screen)
        self.cursor_x = (self.cursor_x + dx as i32).max(0).min(self.screen_width - 1);
        self.cursor_y = (self.cursor_y + dy as i32).max(0).min(self.screen_height - 1);

        // Enqueue event
        let event = MouseEvent {
            dx,
            dy,
            scroll,
            buttons: self.buttons,
        };

        self.events[self.event_head] = Some(event);
        self.event_head = (self.event_head + 1) % self.events.len();
    }

    /// Dequeue a mouse event.
    pub fn poll(&mut self) -> Option<MouseEvent> {
        if self.event_tail == self.event_head {
            return None;
        }
        let event = self.events[self.event_tail].take();
        self.event_tail = (self.event_tail + 1) % self.events.len();
        event
    }

    /// Get cursor position.
    pub fn position(&self) -> (i32, i32) {
        (self.cursor_x, self.cursor_y)
    }
}

/// Initialize the PS/2 mouse.
pub fn init(screen_width: i32, screen_height: i32) {
    // Enable the PS/2 auxiliary device
    unsafe {
        // Wait for controller input buffer to be empty
        while inb(0x64) & 0x02 != 0 {
            core::hint::spin_loop();
        }

        // Enable auxiliary device (command 0xA8)
        outb(0x64, 0xA8);

        // Enable IRQ12 (command 0x20 to read, then write with bit 1 set)
        wait_ready();
        outb(0x64, 0x20);
        wait_output();
        let status = inb(0x60);
        wait_ready();
        outb(0x64, 0x60);
        wait_ready();
        outb(0x60, status | 0x02); // Enable IRQ12

        // Set default settings (command 0xF6)
        mouse_write(0xF6);
        mouse_read(); // ACK

        // Enable data reporting (command 0xF4)
        mouse_write(0xF4);
        mouse_read(); // ACK
    }

    *MOUSE.lock() = Some(MouseDriver::new(screen_width, screen_height));
    crate::serial_println!("[OK] PS/2 mouse initialized ({}×{})", screen_width, screen_height);
}

pub fn irq_handler() {
    let byte = unsafe { inb(0x60) };
    if let Some(ref mut driver) = *MOUSE.lock() {
        driver.process_byte(byte);
    }
}

/// Pop the next mouse event from the global buffer.
pub fn poll_event() -> Option<MouseEvent> {
    if let Some(ref mut driver) = *MOUSE.lock() {
        driver.poll()
    } else {
        None
    }
}

/// Get the current absolute cursor position.
pub fn get_position() -> (i32, i32) {
    if let Some(ref driver) = *MOUSE.lock() {
        driver.position()
    } else {
        (0, 0)
    }
}

// Port I/O helpers
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("al") val, in("dx") port, options(nomem, nostack));
}

unsafe fn wait_ready() {
    while inb(0x64) & 0x02 != 0 { core::hint::spin_loop(); }
}

unsafe fn wait_output() {
    while inb(0x64) & 0x01 == 0 { core::hint::spin_loop(); }
}

unsafe fn mouse_write(byte: u8) {
    wait_ready();
    outb(0x64, 0xD4); // Send to auxiliary device
    wait_ready();
    outb(0x60, byte);
}

unsafe fn mouse_read() -> u8 {
    wait_output();
    inb(0x60)
}
