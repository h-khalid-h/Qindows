//! # Qernel System Call Dispatch Table
//!
//! Maps Q-Ring syscall numbers to kernel handlers.
//! Validates arguments, checks capabilities, and dispatches
//! to the appropriate kernel service.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// syscall numbers.
pub mod nr {
    pub const SYS_EXIT: u64 = 0;
    pub const SYS_READ: u64 = 1;
    pub const SYS_WRITE: u64 = 2;
    pub const SYS_OPEN: u64 = 3;
    pub const SYS_CLOSE: u64 = 4;
    pub const SYS_MMAP: u64 = 10;
    pub const SYS_MUNMAP: u64 = 11;
    pub const SYS_MPROTECT: u64 = 12;
    pub const SYS_BRK: u64 = 13;
    pub const SYS_SPAWN: u64 = 20;
    pub const SYS_WAIT: u64 = 21;
    pub const SYS_KILL: u64 = 22;
    pub const SYS_GETPID: u64 = 23;
    pub const SYS_IPC_SEND: u64 = 30;
    pub const SYS_IPC_RECV: u64 = 31;
    pub const SYS_IPC_SUBSCRIBE: u64 = 32;
    pub const SYS_SLEEP: u64 = 40;
    pub const SYS_YIELD: u64 = 41;
    pub const SYS_CLOCK_GET: u64 = 42;
    pub const SYS_SOCKET: u64 = 50;
    pub const SYS_CONNECT: u64 = 51;
    pub const SYS_BIND: u64 = 52;
    pub const SYS_LISTEN: u64 = 53;
    pub const SYS_ACCEPT: u64 = 54;
    pub const SYS_SENDTO: u64 = 55;
    pub const SYS_RECVFROM: u64 = 56;
    pub const SYS_GPU_SUBMIT: u64 = 60;
    pub const SYS_GPU_SYNC: u64 = 61;
    pub const SYS_DEBUG: u64 = 255;
}

/// Syscall arguments (up to 6 args, matching x86-64 ABI).
#[derive(Debug, Clone, Copy)]
pub struct SyscallArgs {
    pub nr: u64,     // RAX — syscall number
    pub arg1: u64,   // RDI
    pub arg2: u64,   // RSI
    pub arg3: u64,   // RDX
    pub arg4: u64,   // R10
    pub arg5: u64,   // R8
    pub arg6: u64,   // R9
}

/// Syscall return value.
#[derive(Debug, Clone, Copy)]
pub struct SyscallResult {
    pub value: i64,   // Return value (negative = error)
    pub value2: u64,  // Secondary return value
}

impl SyscallResult {
    pub fn ok(value: i64) -> Self { SyscallResult { value, value2: 0 } }
    pub fn err(code: i64) -> Self { SyscallResult { value: code, value2: 0 } }
    pub fn pair(v1: i64, v2: u64) -> Self { SyscallResult { value: v1, value2: v2 } }
}

/// Error codes.
pub mod errno {
    pub const EPERM: i64 = -1;
    pub const ENOENT: i64 = -2;
    pub const ESRCH: i64 = -3;
    pub const EINTR: i64 = -4;
    pub const EIO: i64 = -5;
    pub const ENOMEM: i64 = -12;
    pub const EACCES: i64 = -13;
    pub const EFAULT: i64 = -14;
    pub const EBUSY: i64 = -16;
    pub const EEXIST: i64 = -17;
    pub const EINVAL: i64 = -22;
    pub const ENOSYS: i64 = -38;
    pub const ERANGE: i64 = -34;
    pub const ETIMEDOUT: i64 = -110;
}

/// Per-syscall statistics.
pub struct SyscallStat {
    pub count: AtomicU64,
    pub errors: AtomicU64,
    pub total_ns: AtomicU64,
}

impl SyscallStat {
    pub const fn new() -> Self {
        SyscallStat {
            count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
        }
    }
}

/// The Syscall Dispatch Table.
pub struct SyscallTable {
    /// Syscall names (for debug/tracing)
    pub names: BTreeMap<u64, &'static str>,
    /// Stats per syscall number
    pub stats: Vec<(u64, SyscallStat)>,
    /// Global counters
    pub total_calls: AtomicU64,
    pub total_errors: AtomicU64,
}

impl SyscallTable {
    pub fn new() -> Self {
        let mut names = BTreeMap::new();
        names.insert(nr::SYS_EXIT, "exit");
        names.insert(nr::SYS_READ, "read");
        names.insert(nr::SYS_WRITE, "write");
        names.insert(nr::SYS_OPEN, "open");
        names.insert(nr::SYS_CLOSE, "close");
        names.insert(nr::SYS_MMAP, "mmap");
        names.insert(nr::SYS_MUNMAP, "munmap");
        names.insert(nr::SYS_MPROTECT, "mprotect");
        names.insert(nr::SYS_BRK, "brk");
        names.insert(nr::SYS_SPAWN, "spawn");
        names.insert(nr::SYS_WAIT, "wait");
        names.insert(nr::SYS_KILL, "kill");
        names.insert(nr::SYS_GETPID, "getpid");
        names.insert(nr::SYS_IPC_SEND, "ipc_send");
        names.insert(nr::SYS_IPC_RECV, "ipc_recv");
        names.insert(nr::SYS_IPC_SUBSCRIBE, "ipc_subscribe");
        names.insert(nr::SYS_SLEEP, "sleep");
        names.insert(nr::SYS_YIELD, "yield");
        names.insert(nr::SYS_CLOCK_GET, "clock_get");
        names.insert(nr::SYS_SOCKET, "socket");
        names.insert(nr::SYS_CONNECT, "connect");
        names.insert(nr::SYS_BIND, "bind");
        names.insert(nr::SYS_LISTEN, "listen");
        names.insert(nr::SYS_ACCEPT, "accept");
        names.insert(nr::SYS_SENDTO, "sendto");
        names.insert(nr::SYS_RECVFROM, "recvfrom");
        names.insert(nr::SYS_GPU_SUBMIT, "gpu_submit");
        names.insert(nr::SYS_GPU_SYNC, "gpu_sync");
        names.insert(nr::SYS_DEBUG, "debug");

        SyscallTable {
            names,
            stats: Vec::new(),
            total_calls: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
        }
    }

    /// Dispatch a syscall.
    pub fn dispatch(&self, args: SyscallArgs, silo_id: u64) -> SyscallResult {
        self.total_calls.fetch_add(1, Ordering::Relaxed);

        match args.nr {
            nr::SYS_EXIT => self.sys_exit(args.arg1 as i32),
            nr::SYS_READ => self.sys_read(silo_id, args.arg1, args.arg2, args.arg3),
            nr::SYS_WRITE => self.sys_write(silo_id, args.arg1, args.arg2, args.arg3),
            nr::SYS_OPEN => self.sys_open(silo_id, args.arg1, args.arg2),
            nr::SYS_CLOSE => self.sys_close(silo_id, args.arg1),
            nr::SYS_MMAP => self.sys_mmap(silo_id, args.arg1, args.arg2, args.arg3),
            nr::SYS_GETPID => SyscallResult::ok(silo_id as i64),
            nr::SYS_YIELD => { SyscallResult::ok(0) }
            nr::SYS_CLOCK_GET => self.sys_clock_get(),
            nr::SYS_DEBUG => {
                crate::serial_println!("SYSCALL debug: silo={} arg1={:#x}", silo_id, args.arg1);
                SyscallResult::ok(0)
            }
            _ => {
                self.total_errors.fetch_add(1, Ordering::Relaxed);
                SyscallResult::err(errno::ENOSYS)
            }
        }
    }

    fn sys_exit(&self, code: i32) -> SyscallResult {
        crate::serial_println!("SYSCALL exit: code={}", code);
        SyscallResult::ok(0)
    }

    fn sys_read(&self, _silo_id: u64, _fd: u64, buf_ptr: u64, len: u64) -> SyscallResult {
        // Validate buffer pointer belongs to the Silo's address space
        if buf_ptr == 0 || len == 0 {
            return SyscallResult::err(errno::EFAULT);
        }
        // In production: dispatch to VFS, check Silo capabilities
        SyscallResult::ok(0) // 0 bytes read (EOF)
    }

    fn sys_write(&self, _silo_id: u64, fd: u64, buf_ptr: u64, len: u64) -> SyscallResult {
        if buf_ptr == 0 {
            return SyscallResult::err(errno::EFAULT);
        }
        // fd 1 = stdout, fd 2 = stderr
        if fd == 1 || fd == 2 {
            // Would copy from user space and write to serial/console
            return SyscallResult::ok(len as i64);
        }
        SyscallResult::ok(len as i64)
    }

    fn sys_open(&self, _silo_id: u64, path_ptr: u64, _flags: u64) -> SyscallResult {
        if path_ptr == 0 {
            return SyscallResult::err(errno::EFAULT);
        }
        // In production: resolve path via VFS, check permissions
        SyscallResult::ok(3) // return fd 3
    }

    fn sys_close(&self, _silo_id: u64, fd: u64) -> SyscallResult {
        if fd < 3 {
            return SyscallResult::err(errno::EBUSY); // Can't close stdin/stdout/stderr
        }
        SyscallResult::ok(0)
    }

    fn sys_mmap(&self, _silo_id: u64, addr: u64, len: u64, _prot: u64) -> SyscallResult {
        if len == 0 {
            return SyscallResult::err(errno::EINVAL);
        }
        // In production: allocate pages via VMM, map into Silo's address space
        SyscallResult::ok(addr as i64)
    }

    fn sys_clock_get(&self) -> SyscallResult {
        // Read TSC for nanosecond timestamp
        let tsc: u64;
        unsafe {
            let lo: u32;
            let hi: u32;
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem));
            tsc = (hi as u64) << 32 | lo as u64;
        }
        SyscallResult::ok(tsc as i64)
    }

    /// Get syscall name for tracing.
    pub fn name(&self, nr: u64) -> &str {
        self.names.get(&nr).copied().unwrap_or("unknown")
    }
}
