#![no_std]
#![no_main]

extern crate q_shell;
extern crate alloc;

use core::panic::PanicInfo;
use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

/// A simple 2MB static bump allocator since we are now a standalone
/// no_std Ring 3 binary and need to use alloc::string::String for parsing.
struct StaticAllocator(Mutex<usize>);

static mut HEAP_MEM: [u8; 2 * 1024 * 1024] = [0; 2 * 1024 * 1024];

unsafe impl GlobalAlloc for StaticAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut offset = self.0.lock();
        let alloc_start = (*offset + layout.align() - 1) & !(layout.align() - 1);
        let alloc_end = alloc_start.saturating_add(layout.size());
        
        if alloc_end <= HEAP_MEM.len() {
            *offset = alloc_end;
            HEAP_MEM.as_mut_ptr().add(alloc_start)
        } else {
            core::ptr::null_mut()
        }
    }
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator never frees (fine for a short-lived test)
    }
}

#[global_allocator]
static ALLOCATOR: StaticAllocator = StaticAllocator(Mutex::new(0));


/// Print a string directly to the Kernel Console via Syscall 300 (SysPrint)
fn print_str(s: &str) {
    let ptr = s.as_ptr() as u64;
    let len = s.len() as u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 300, // Syscall::SysPrint
            in("rdi") ptr,
            in("rsi") len,
            out("rcx") _,
            out("r11") _,
        );
    }
}

/// Gap 16.3 — Poll one character from the PS/2 keyboard ring buffer.
///
/// Calls Syscall 301 (SysReadKey). Returns:
///   Some(ch) — ASCII character available
///   None     — buffer empty (caller should Yield then retry)
fn poll_key() -> Option<u8> {
    let result: i64;
    unsafe {
        // SYSCALL ABI: rax = syscall number on entry, rax = return value on exit.
        // We use `in("rax") 301` so the compiler knows we need 301 in rax before
        // the instruction fires, then `lateout("rax") result` to capture the return
        // value — lateout tells the compiler rax is written only AFTER all inputs
        // are consumed, which is exactly how SYSCALL works.
        core::arch::asm!(
            "syscall",
            in("rax") 301u64,   // Syscall::SysReadKey
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
        );
    }
    if result > 0 {
        Some(result as u8)
    } else {
        None
    }
}

/// Yield this fiber's time slice (Syscall 0) to avoid busy-spinning.
fn yield_cpu() {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 0u64,  // Syscall::Yield
            out("rcx") _,
            out("r11") _,
        );
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print_str("\n\n=== Q-SHELL RING 3 INITIALIZATION ===\n");
    print_str("Memory Manager: StaticBumpAllocator (2MB)\n");
    print_str("IPC Link: Syscall 163 Ready\n");
    print_str("Keyboard: SysReadKey (Syscall 301) Active\n");
    
    let mut shell = q_shell::repl::ShellSession::new();
    for line in q_shell::repl::ShellSession::banner() {
        print_str(&line);
        print_str("\n");
    }
    
    // ── Gap 16.3: Real interactive readline loop ─────────────────────────────
    // Previously this ran a pre-baked array of test commands. Now it polls
    // the PS/2 keyboard ring buffer (populated by interrupts/handlers.rs IRQ 33)
    // via Syscall 301 (SysReadKey) and accumulates characters into a line buffer.
    //
    // Protocol:
    //   - Printable ASCII (0x20-0x7E): append to line_buf, echo to console
    //   - Enter (0x0D / '\r'): dispatch line_buf to shell, clear buffer
    //   - Backspace (0x08): pop last char from line_buf, echo backspace-space-backspace
    //   - Yield (Syscall 0): called when buffer is empty to avoid CPU spin
    //
    // The line buffer is a fixed 256-byte static array to avoid heap pressure.
    let mut line_buf: [u8; 256] = [0u8; 256];
    let mut line_len: usize = 0;

    // Print initial prompt
    print_str("\x1b[36m~›\x1b[0m ");

    loop {
        match poll_key() {
            None => {
                // Buffer empty — yield the CPU core to the scheduler
                yield_cpu();
            }
            Some(ch) => {
                match ch {
                    b'\r' | b'\n' => {
                        // Enter: dispatch the accumulated line
                        print_str("\n");
                        if line_len > 0 {
                            // Convert buf slice to str — only ASCII so safe
                            if let Ok(cmd) = core::str::from_utf8(&line_buf[..line_len]) {
                                let output = shell.process_input(cmd);
                                for out_line in output {
                                    print_str(&out_line);
                                    print_str("\n");
                                }
                            }
                            line_len = 0;
                        }
                        // Print next prompt
                        print_str("\x1b[36m~›\x1b[0m ");
                    }
                    0x08 | 0x7F => {
                        // Backspace / DEL: remove last char
                        if line_len > 0 {
                            line_len -= 1;
                            // Visual backspace: BS + space + BS
                            print_str("\x08 \x08");
                        }
                    }
                    0x20..=0x7E => {
                        // Printable ASCII
                        if line_len < line_buf.len() - 1 {
                            line_buf[line_len] = ch;
                            line_len += 1;
                            // Echo character back
                            let s = unsafe {
                                core::str::from_utf8_unchecked(core::slice::from_raw_parts(&ch as *const u8, 1))
                            };
                            print_str(s);
                        }
                        // Silently drop characters that would overflow the buffer
                    }
                    _ => { /* Ignore non-printable characters, escape sequences, etc. */ }
                }
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print_str("\n\n!!! Q-SHELL RING 3 PANIC !!!\n");
    print_str("Application Halted.\n");
    loop {}
}

