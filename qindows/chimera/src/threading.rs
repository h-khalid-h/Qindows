//! # Chimera Win32 Threading Emulation
//!
//! Emulates the Windows threading model for legacy apps.
//! Maps Win32 threads to Qindows fibers, critical sections
//! to spinlocks, and events/mutexes to Qernel primitives.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Win32 thread handle.
pub type Handle = u64;

/// Thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Created,
    Running,
    Suspended,
    Waiting,
    Terminated,
}

/// A Win32 thread.
#[derive(Debug, Clone)]
pub struct Win32Thread {
    /// Thread handle
    pub handle: Handle,
    /// Thread ID
    pub thread_id: u32,
    /// State
    pub state: ThreadState,
    /// Entry point address
    pub entry_point: u64,
    /// Stack size
    pub stack_size: u64,
    /// Exit code
    pub exit_code: Option<u32>,
    /// Mapped Qindows fiber ID
    pub fiber_id: u64,
    /// Thread name (for debugging)
    pub name: String,
    /// Suspend count
    pub suspend_count: u32,
    /// Priority (THREAD_PRIORITY_*)
    pub priority: i32,
    /// Thread-local storage slots
    pub tls: BTreeMap<u32, u64>,
}

/// A critical section (lightweight mutex).
#[derive(Debug, Clone)]
pub struct CriticalSection {
    pub handle: Handle,
    /// Owning thread ID (0 = unlocked)
    pub owner: u32,
    /// Recursion count
    pub recursion_count: u32,
    /// Spin count before blocking
    pub spin_count: u32,
}

/// A Win32 event object.
#[derive(Debug, Clone)]
pub struct Win32Event {
    pub handle: Handle,
    pub name: Option<String>,
    pub signaled: bool,
    pub manual_reset: bool,
    /// Thread IDs waiting on this event
    pub waiters: Vec<u32>,
}

/// A Win32 mutex.
#[derive(Debug, Clone)]
pub struct Win32Mutex {
    pub handle: Handle,
    pub name: Option<String>,
    pub owner: u32,
    pub abandoned: bool,
}

/// A Win32 semaphore.
#[derive(Debug, Clone)]
pub struct Win32Semaphore {
    pub handle: Handle,
    pub name: Option<String>,
    pub count: i32,
    pub max_count: i32,
}

/// Wait result.
#[derive(Debug, Clone, Copy)]
pub enum WaitResult {
    Object0,       // WAIT_OBJECT_0
    Timeout,       // WAIT_TIMEOUT
    Abandoned,     // WAIT_ABANDONED
    Failed,        // WAIT_FAILED
}

/// The Win32 Threading Manager.
pub struct ThreadingManager {
    /// Active threads
    pub threads: BTreeMap<Handle, Win32Thread>,
    /// Critical sections
    pub critical_sections: BTreeMap<Handle, CriticalSection>,
    /// Events
    pub events: BTreeMap<Handle, Win32Event>,
    /// Mutexes
    pub mutexes: BTreeMap<Handle, Win32Mutex>,
    /// Semaphores
    pub semaphores: BTreeMap<Handle, Win32Semaphore>,
    /// Next handle
    next_handle: Handle,
    /// Next thread ID
    next_thread_id: u32,
    /// TLS index allocator
    next_tls_index: u32,
    /// Stats
    pub stats: ThreadingStats,
}

/// Threading statistics.
#[derive(Debug, Clone, Default)]
pub struct ThreadingStats {
    pub threads_created: u64,
    pub threads_terminated: u64,
    pub cs_enters: u64,
    pub cs_contentions: u64,
    pub events_set: u64,
    pub waits_completed: u64,
}

impl ThreadingManager {
    pub fn new() -> Self {
        ThreadingManager {
            threads: BTreeMap::new(),
            critical_sections: BTreeMap::new(),
            events: BTreeMap::new(),
            mutexes: BTreeMap::new(),
            semaphores: BTreeMap::new(),
            next_handle: 0x1000,
            next_thread_id: 1,
            next_tls_index: 0,
            stats: ThreadingStats::default(),
        }
    }

    /// CreateThread
    pub fn create_thread(
        &mut self,
        entry_point: u64,
        stack_size: u64,
        suspended: bool,
        fiber_id: u64,
    ) -> (Handle, u32) {
        let handle = self.alloc_handle();
        let thread_id = self.next_thread_id;
        self.next_thread_id += 1;

        let thread = Win32Thread {
            handle,
            thread_id,
            state: if suspended { ThreadState::Suspended } else { ThreadState::Running },
            entry_point,
            stack_size: if stack_size == 0 { 1024 * 1024 } else { stack_size }, // Default 1 MiB
            exit_code: None,
            fiber_id,
            name: alloc::format!("Thread-{}", thread_id),
            suspend_count: if suspended { 1 } else { 0 },
            priority: 0, // THREAD_PRIORITY_NORMAL
            tls: BTreeMap::new(),
        };

        self.threads.insert(handle, thread);
        self.stats.threads_created += 1;
        (handle, thread_id)
    }

    /// ExitThread
    pub fn exit_thread(&mut self, handle: Handle, exit_code: u32) {
        if let Some(thread) = self.threads.get_mut(&handle) {
            thread.state = ThreadState::Terminated;
            thread.exit_code = Some(exit_code);
            self.stats.threads_terminated += 1;
        }
    }

    /// SuspendThread
    pub fn suspend_thread(&mut self, handle: Handle) -> Option<u32> {
        if let Some(thread) = self.threads.get_mut(&handle) {
            thread.suspend_count += 1;
            if thread.state == ThreadState::Running {
                thread.state = ThreadState::Suspended;
            }
            Some(thread.suspend_count - 1) // Previous suspend count
        } else { None }
    }

    /// ResumeThread
    pub fn resume_thread(&mut self, handle: Handle) -> Option<u32> {
        if let Some(thread) = self.threads.get_mut(&handle) {
            if thread.suspend_count > 0 {
                thread.suspend_count -= 1;
                if thread.suspend_count == 0 {
                    thread.state = ThreadState::Running;
                }
            }
            Some(thread.suspend_count)
        } else { None }
    }

    /// InitializeCriticalSection
    pub fn init_critical_section(&mut self) -> Handle {
        let handle = self.alloc_handle();
        self.critical_sections.insert(handle, CriticalSection {
            handle, owner: 0, recursion_count: 0, spin_count: 4000,
        });
        handle
    }

    /// EnterCriticalSection
    pub fn enter_critical_section(&mut self, handle: Handle, thread_id: u32) -> bool {
        self.stats.cs_enters += 1;
        if let Some(cs) = self.critical_sections.get_mut(&handle) {
            if cs.owner == 0 || cs.owner == thread_id {
                cs.owner = thread_id;
                cs.recursion_count += 1;
                true
            } else {
                self.stats.cs_contentions += 1;
                false // Would need to block
            }
        } else { false }
    }

    /// LeaveCriticalSection
    pub fn leave_critical_section(&mut self, handle: Handle, thread_id: u32) {
        if let Some(cs) = self.critical_sections.get_mut(&handle) {
            // Only the owner can leave
            if cs.owner != thread_id {
                return;
            }
            if cs.recursion_count > 0 {
                cs.recursion_count -= 1;
                if cs.recursion_count == 0 {
                    cs.owner = 0;
                }
            }
        }
    }

    /// CreateEvent
    pub fn create_event(&mut self, manual_reset: bool, initial_state: bool, name: Option<&str>) -> Handle {
        let handle = self.alloc_handle();
        self.events.insert(handle, Win32Event {
            handle,
            name: name.map(String::from),
            signaled: initial_state,
            manual_reset,
            waiters: Vec::new(),
        });
        handle
    }

    /// SetEvent
    pub fn set_event(&mut self, handle: Handle) {
        if let Some(event) = self.events.get_mut(&handle) {
            event.signaled = true;
            self.stats.events_set += 1;
        }
    }

    /// ResetEvent
    pub fn reset_event(&mut self, handle: Handle) {
        if let Some(event) = self.events.get_mut(&handle) {
            event.signaled = false;
        }
    }

    /// TlsAlloc
    pub fn tls_alloc(&mut self) -> u32 {
        let idx = self.next_tls_index;
        self.next_tls_index += 1;
        idx
    }

    /// TlsSetValue
    pub fn tls_set_value(&mut self, thread_handle: Handle, tls_index: u32, value: u64) {
        if let Some(thread) = self.threads.get_mut(&thread_handle) {
            thread.tls.insert(tls_index, value);
        }
    }

    /// TlsGetValue
    pub fn tls_get_value(&self, thread_handle: Handle, tls_index: u32) -> u64 {
        self.threads.get(&thread_handle)
            .and_then(|t| t.tls.get(&tls_index))
            .copied()
            .unwrap_or(0)
    }

    fn alloc_handle(&mut self) -> Handle {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }
}
