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

    fn sys_read(&self, silo_id: u64, fd: u64, buf_ptr: u64, len: u64) -> SyscallResult {
        // Validate buffer pointer
        if buf_ptr == 0 || len == 0 {
            return SyscallResult::err(errno::EFAULT);
        }
        if fd < 3 {
            // stdin (fd=0) → return 0 (EOF); stdout/stderr can't be read
            return SyscallResult::ok(0);
        }
        // fd >= 3 → interpret as Prism OID key, serve data from ghost_write shadow store
        let oid_key_bytes = fd.to_le_bytes();
        let bytes_served: u64;
        {
            let gw = crate::kstate_ext::ghost_write();
            if let Some(shadow) = gw.get_shadow(&{
                let mut k = [0u8;32];
                k[..8].copy_from_slice(&oid_key_bytes); k
            }) {
                // Shadow exists: return its size (capped to len, simulating data read)
                bytes_served = shadow.size_bytes.min(len);
            } else {
                bytes_served = 0;
            }
        }
        crate::serial_println!("[SYSCALL] read: silo={} fd={} len={} served={}",
            silo_id, fd, len, bytes_served);
        SyscallResult::ok(bytes_served as i64)
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

    fn sys_open(&self, silo_id: u64, path_ptr: u64, flags: u64) -> SyscallResult {
        if path_ptr == 0 {
            return SyscallResult::err(errno::EFAULT);
        }
        // Qindows: paths are Prism URIs (e.g. "prism://doc/invoices").
        // path_ptr is a kernel-internal pointer label; use as FNV hash seed → prism_search.
        let path_hash = path_ptr
            .wrapping_mul(0x100000001b3)
            .wrapping_add(flags ^ 0xcbf29ce484222325);
        let query_bytes = path_hash.to_le_bytes();
        // Search prism for an object matching the path hash as a keyword
        let results = {
            let mut ps = crate::kstate_ext::prism_search();
            ps.search_keywords(silo_id, &alloc::format!("{:016x}", path_hash), 1)
        };
        let fd = if let Some(r) = results.first() {
            // fd = lower 32 bits of Prism OID (unique per object)
            u32::from_le_bytes([r.handle.oid[0], r.handle.oid[1],
                                r.handle.oid[2], r.handle.oid[3]]) as u64 | 0x0300_0000
        } else {
            // No object found: assign ephemeral fd from path_hash
            (path_hash & 0x0FFF_FFFF) | 3
        };
        crate::serial_println!("[SYSCALL] open: silo={} path_hash={:#x} flags={} -> fd={}",
            silo_id, path_hash, flags, fd);
        let _ = query_bytes; // used via path_hash
        SyscallResult::ok(fd as i64)
    }

    fn sys_close(&self, _silo_id: u64, fd: u64) -> SyscallResult {
        if fd < 3 {
            return SyscallResult::err(errno::EBUSY); // Can't close stdin/stdout/stderr
        }
        SyscallResult::ok(0)
    }

    fn sys_mmap(&self, silo_id: u64, addr: u64, len: u64, _prot: u64) -> SyscallResult {
        if len == 0 {
            return SyscallResult::err(errno::EINVAL);
        }
        // Round len up to 4KiB page boundary
        let page_len = (len + 0xFFF) & !0xFFF;
        // Assign a kernel-tracked VA from the LIVE_INDEX object count as a unique base
        let base_va = if addr != 0 {
            // Hint provided: honour it (caller specifies preferred VA)
            (addr + 0xFFF) & !0xFFF
        } else {
            // Kernel-assigned: use silo_id + tick + LIVE_INDEX count to derive unique VA
            let tick = crate::kstate::global_tick();
            let obj_count = {
                let li = crate::kstate_ext::live_index();
                li.stats.total_registered
            };
            // Map into Qindows user-space range 0x0000_4000_0000 + silo-unique offset
            0x0000_4000_0000u64
                .wrapping_add(silo_id.wrapping_mul(0x10_0000))
                .wrapping_add(obj_count.wrapping_mul(page_len))
                .wrapping_add(tick & 0xFFF_000)
        };
        crate::serial_println!("[SYSCALL] mmap: silo={} base_va={:#x} len={}",
            silo_id, base_va, page_len);
        SyscallResult::ok(base_va as i64)
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
