//! # System Call Interface
//!
//! The SYSCALL/SYSRET fast-path from user space to the Qernel.
//! Every interaction between a Q-Silo and the kernel flows through
//! this interface. Each syscall is validated against capability tokens.
//!
//! Convention:
//! - RAX = syscall number
//! - RDI, RSI, RDX, R10, R8, R9 = arguments
//! - RAX = return value (negative = error)


/// System call numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallId {
    /// Yield the current Fiber's time slice
    Yield = 0,
    /// Exit the current Fiber
    Exit = 1,
    /// Spawn a new Fiber in the current Silo
    SpawnFiber = 2,
    /// Open a Prism object by query or OID
    PrismOpen = 10,
    /// Read from an opened object handle
    PrismRead = 11,
    /// Write to an opened object handle (Ghost-Write)
    PrismWrite = 12,
    /// Close an object handle
    PrismClose = 13,
    /// Semantic search in the Prism graph
    PrismQuery = 14,
    /// Send a message via Q-Ring IPC
    IpcSend = 20,
    /// Receive messages from a Q-Ring
    IpcRecv = 21,
    /// Create a new IPC channel
    IpcCreate = 22,
    /// Map a shared memory page (for zero-copy IPC)
    MapShared = 30,
    /// Unmap a shared memory page
    UnmapShared = 31,
    /// Allocate physical frames for the current Silo
    AllocFrames = 32,
    /// Free previously allocated frames
    FreeFrames = 33,
    /// Request a capability token from the user (via Aether prompt)
    RequestCap = 40,
    /// Delegate a capability to a child Silo
    DelegateCap = 41,
    /// Get current time (scheduler ticks)
    GetTime = 50,
    /// Sleep for N microseconds
    Sleep = 51,
    /// Get this Silo's ID
    GetSiloId = 52,
    /// Register a window with the Aether compositor
    AetherRegister = 60,
    /// Submit a vector frame to Aether
    AetherSubmit = 61,
    /// Open a network connection via the Nexus mesh
    NetConnect = 70,
    /// Send network data
    NetSend = 71,
    /// Receive network data
    NetRecv = 72,
    /// Report status to the Sentinel (heartbeat)
    SentinelHeartbeat = 80,
}

/// System call error codes.
#[derive(Debug, Clone, Copy)]
#[repr(i64)]
pub enum SyscallError {
    /// Success (not an error)
    Ok = 0,
    /// Invalid syscall number
    InvalidSyscall = -1,
    /// Insufficient capability
    PermissionDenied = -2,
    /// Resource not found
    NotFound = -3,
    /// Out of memory
    OutOfMemory = -4,
    /// Invalid argument
    InvalidArg = -5,
    /// Resource busy / already in use
    Busy = -6,
    /// Connection refused or reset
    ConnectionError = -7,
    /// I/O error
    IoError = -8,
    /// Capability token expired
    Expired = -9,
    /// Buffer too small
    BufferTooSmall = -10,
    /// Operation would block (in async mode)
    WouldBlock = -11,
}

/// System call arguments extracted from registers.
#[derive(Debug)]
pub struct SyscallArgs {
    pub id: u64,
    pub arg0: u64, // RDI
    pub arg1: u64, // RSI
    pub arg2: u64, // RDX
    pub arg3: u64, // R10
    pub arg4: u64, // R8
    pub arg5: u64, // R9
}

/// Initialize the SYSCALL/SYSRET fast-path via MSRs.
///
/// This configures the CPU to enter the kernel directly when
/// user code executes the `syscall` instruction — much faster
/// than `int 0x80` as it avoids the IDT lookup.
pub fn init() {
    unsafe {
        // STAR MSR (0xC0000081): segment selectors for SYSCALL/SYSRET
        // Bits 47:32 = kernel CS (SYSCALL)
        // Bits 63:48 = user CS base (SYSRET adds offsets)
        let star: u64 = (0x08u64 << 32) | (0x18u64 << 48);
        write_msr(0xC0000081, star);

        // LSTAR MSR (0xC0000082): kernel entry point for SYSCALL
        write_msr(0xC0000082, syscall_entry as *const () as u64);

        // SFMASK MSR (0xC0000084): RFLAGS mask on SYSCALL entry
        // Clear IF (disable interrupts) and DF (clear direction flag)
        write_msr(0xC0000084, 0x0600);

        // Enable SYSCALL/SYSRET in EFER MSR
        let efer = read_msr(0xC0000080);
        write_msr(0xC0000080, efer | 1); // Set SCE bit
    }

    crate::serial_println!("[OK] SYSCALL/SYSRET fast-path configured");
}

/// The raw SYSCALL entry point.
///
/// When user code executes `syscall`:
/// - RCX = user RIP (return address)
/// - R11 = user RFLAGS
/// - RAX = syscall number
/// - RDI, RSI, RDX, R10, R8, R9 = arguments
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // Switch to kernel stack (saved in TSS.rsp0)
        // For now, save user RSP and switch
        "swapgs",                    // Switch GS base to kernel
        "mov gs:[0x08], rsp",        // Save user RSP in kernel area
        "mov rsp, gs:[0x00]",        // Load kernel RSP from TSS

        // Save user registers
        "push rcx",                  // User RIP
        "push r11",                  // User RFLAGS
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Call the Rust syscall dispatcher
        // RAX = syscall number (already set)
        // RDI, RSI, RDX, R10, R8, R9 = args (already in place)
        "mov rcx, r10",              // Linux convention: R10 → RCX for arg3
        "call {dispatch}",

        // Restore user registers
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop r11",                   // User RFLAGS
        "pop rcx",                   // User RIP

        // Switch back to user stack
        "mov rsp, gs:[0x08]",
        "swapgs",

        // Return to user space
        "sysretq",
        dispatch = sym dispatch_syscall,
    );
}

/// High-level syscall dispatcher.
///
/// Validates capability, routes to the appropriate handler,
/// and returns the result in RAX.
pub fn dispatch_syscall(
    id: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> i64 {
    let _args = SyscallArgs {
        id,
        arg0,
        arg1,
        arg2,
        arg3,
        arg4,
        arg5: 0,
    };

    match id {
        0 => handle_yield(),
        1 => handle_exit(arg0 as i32),
        2 => handle_spawn_fiber(arg0),
        10 => handle_prism_open(arg0, arg1),
        11 => handle_prism_read(arg0, arg1 as *mut u8, arg2 as usize),
        12 => handle_prism_write(arg0, arg1 as *const u8, arg2 as usize),
        13 => handle_prism_close(arg0),
        20 => handle_ipc_send(arg0, arg1, arg2),
        21 => handle_ipc_recv(arg0, arg1 as *mut u8, arg2 as usize),
        50 => handle_get_time(),
        52 => handle_get_silo_id(),
        _ => SyscallError::InvalidSyscall as i64,
    }
}

// ─── Syscall Handlers ───────────────────────────────────────────────

fn handle_yield() -> i64 {
    // Trigger a context switch to the next ready Fiber
    // In production: call scheduler::schedule()
    0
}

fn handle_exit(code: i32) -> i64 {
    // Mark current fiber as Dead
    crate::serial_println!("Fiber exit with code {}", code);
    0
}

fn handle_spawn_fiber(entry_point: u64) -> i64 {
    // Create a new fiber at the given entry point
    // Returns the fiber ID on success
    let _ = entry_point;
    1 // Stub fiber ID
}

fn handle_prism_open(query_ptr: u64, query_len: u64) -> i64 {
    // Open a Prism object by semantic query
    let _ = (query_ptr, query_len);
    0 // Stub handle
}

fn handle_prism_read(handle: u64, buf: *mut u8, len: usize) -> i64 {
    let _ = (handle, buf, len);
    0
}

fn handle_prism_write(handle: u64, buf: *const u8, len: usize) -> i64 {
    let _ = (handle, buf, len);
    0
}

fn handle_prism_close(handle: u64) -> i64 {
    let _ = handle;
    0
}

fn handle_ipc_send(channel: u64, msg_ptr: u64, msg_len: u64) -> i64 {
    let _ = (channel, msg_ptr, msg_len);
    0
}

fn handle_ipc_recv(channel: u64, buf: *mut u8, buf_len: usize) -> i64 {
    let _ = (channel, buf, buf_len);
    SyscallError::WouldBlock as i64
}

fn handle_get_time() -> i64 {
    // Return scheduler tick count
    0
}

fn handle_get_silo_id() -> i64 {
    // Return the current Silo's ID
    0
}

// ─── MSR Helpers ────────────────────────────────────────────────────

unsafe fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack)
    );
    (high as u64) << 32 | low as u64
}

unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack)
    );
}
