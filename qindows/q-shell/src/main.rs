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


#[no_mangle]
pub extern "C" fn _start() -> ! {
    print_str("\n\n=== Q-SHELL RING 3 INITIALIZATION ===\n");
    print_str("Memory Manager: StaticBumpAllocator (2MB)\n");
    print_str("IPC Link: Syscall 163 Ready\n");
    
    let mut shell = q_shell::repl::ShellSession::new();
    for line in q_shell::repl::ShellSession::banner() {
        print_str(&line);
        print_str("\n");
    }
    
    // Simulate Interactive IPC Session
    let test_cmds = [
        "help",
        "prism get secret",
        "mesh node status",
        "journal flush",
        "exit"
    ];
    
    for cmd in test_cmds {
        print_str("\x1b[36m~\u{276f}\x1b[0m ");
        print_str(cmd);
        print_str("\n");
        
        // This will parse into the SyscallBroker and route entirely across IPC Syscalls
        let output = shell.process_input(cmd);
        for line in output {
            print_str(&line);
            print_str("\n");
        }
    }
    
    print_str("=== Q-SHELL EXITED SAFELY ===\n");

    // Halt / Keep alive
    loop {
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 51, // Syscall::Sleep (100ms)
                in("rdi") 100, 
                out("rcx") _,
                out("r11") _,
            );
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print_str("\n\n!!! Q-SHELL RING 3 PANIC !!!\n");
    // Print panic message if possible (no formatted args easily without allocation)
    print_str("Application Halted.\n");
    loop {}
}
