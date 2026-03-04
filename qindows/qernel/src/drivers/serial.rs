//! # Serial Port Driver (COM1)
//!
//! Debug output via the x86 serial port (0x3F8).
//! Used for boot logging and Sentinel diagnostics.

use core::fmt;
use spin::Mutex;

const COM1: u16 = 0x3F8;

/// Serial port writer
pub struct SerialWriter;

impl SerialWriter {
    /// Initialize the serial port.
    pub fn init() {
        unsafe {
            // Disable interrupts
            port_out(COM1 + 1, 0x00);
            // Set baud rate to 115200 (divisor = 1)
            port_out(COM1 + 3, 0x80); // Enable DLAB
            port_out(COM1 + 0, 0x01); // Low byte
            port_out(COM1 + 1, 0x00); // High byte
            // 8 bits, no parity, one stop bit
            port_out(COM1 + 3, 0x03);
            // Enable FIFO
            port_out(COM1 + 2, 0xC7);
            // RTS/DSR set
            port_out(COM1 + 4, 0x0B);
        }
    }

    /// Write a single byte to the serial port.
    fn write_byte(&self, byte: u8) {
        unsafe {
            // Wait for transmit holding register empty
            while port_in(COM1 + 5) & 0x20 == 0 {}
            port_out(COM1, byte);
        }
    }
}

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

/// Global serial writer
static SERIAL: Mutex<SerialWriter> = Mutex::new(SerialWriter);

/// Print to the serial port (used by serial_print! macro).
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    SERIAL.lock().write_fmt(args).unwrap();
}

/// Write a byte to an I/O port.
#[inline(always)]
unsafe fn port_out(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

/// Read a byte from an I/O port.
#[inline(always)]
unsafe fn port_in(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    val
}
