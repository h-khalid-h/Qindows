//! # Qernel Wasm Sandbox
//!
//! Runs untrusted WebAssembly code inside a hardware-isolated sandbox
//! with fine-grained capability gates. Used by the Q-Ledger to execute
//! Wasm apps, and by the Sentinel to quarantine suspicious binaries.
//!
//! Isolation layers:
//! 1. **Wasm memory model**: Linear memory, no raw pointer access
//! 2. **Capability gating**: Each import is a checked capability
//! 3. **Silo isolation**: Sandbox runs inside its own Q-Silo
//! 4. **Resource limits**: CPU ticks, memory pages, I/O ops capped
//! 5. **Instruction metering**: Every Wasm op costs fuel

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Sandbox state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// Created, not yet started
    Created,
    /// Wasm module loaded and validated
    Loaded,
    /// Currently executing
    Running,
    /// Paused (fuel exhausted, waiting for refuel)
    Paused,
    /// Completed successfully
    Finished,
    /// Trapped (runtime error)
    Trapped,
    /// Killed by Sentinel
    Killed,
}

/// A Wasm import — a host function the sandbox can call.
#[derive(Debug, Clone)]
pub struct WasmImport {
    /// Module name (e.g., "qindows")
    pub module: String,
    /// Function name (e.g., "print")
    pub name: String,
    /// Required capability to call this import
    pub required_capability: u64,
    /// Call count
    pub call_count: u64,
}

/// Resource limits for a sandbox.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum linear memory pages (64 KiB each)
    pub max_memory_pages: u32,
    /// Maximum fuel (instruction budget)
    pub max_fuel: u64,
    /// Maximum call stack depth
    pub max_stack_depth: u32,
    /// Maximum I/O operations
    pub max_io_ops: u64,
    /// Maximum execution time (ms)
    pub max_time_ms: u64,
    /// Can access network?
    pub allow_network: bool,
    /// Can access filesystem?
    pub allow_filesystem: bool,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        ResourceLimits {
            max_memory_pages: 256,      // 16 MiB
            max_fuel: 10_000_000,       // ~10M instructions
            max_stack_depth: 1024,
            max_io_ops: 1000,
            max_time_ms: 30_000,        // 30 seconds
            allow_network: false,
            allow_filesystem: false,
        }
    }
}

/// Resource usage counters.
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    pub memory_pages: u32,
    pub fuel_consumed: u64,
    pub stack_depth: u32,
    pub io_ops: u64,
    pub elapsed_ms: u64,
}

/// A Wasm function export.
#[derive(Debug, Clone)]
pub struct WasmExport {
    pub name: String,
    pub param_count: u32,
    pub result_count: u32,
}

/// Trap reason (why the sandbox halted).
#[derive(Debug, Clone)]
pub enum TrapReason {
    /// Out of fuel
    FuelExhausted,
    /// Memory limit exceeded
    OutOfMemory,
    /// Stack overflow
    StackOverflow,
    /// I/O limit exceeded
    IoLimitExceeded,
    /// Time limit exceeded
    Timeout,
    /// Capability denied
    CapabilityDenied(String),
    /// Invalid memory access
    MemoryAccessViolation,
    /// Unreachable instruction
    Unreachable,
    /// Division by zero
    DivisionByZero,
    /// Integer overflow
    IntegerOverflow,
    /// Killed by Sentinel
    KilledBySentinel,
}

/// A Wasm sandbox instance.
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Sandbox ID
    pub id: u64,
    /// State
    pub state: SandboxState,
    /// Silo ID (isolation boundary)
    pub silo_id: u64,
    /// Module name
    pub module_name: String,
    /// Module hash
    pub module_hash: [u8; 32],
    /// Linear memory (simulated)
    pub memory_pages: u32,
    /// Resource limits
    pub limits: ResourceLimits,
    /// Resource usage
    pub usage: ResourceUsage,
    /// Available imports
    pub imports: Vec<WasmImport>,
    /// Exported functions
    pub exports: Vec<WasmExport>,
    /// Granted capabilities (bitmask)
    pub capabilities: u64,
    /// Return value (after completion)
    pub return_value: Option<i64>,
    /// Trap reason (if trapped)
    pub trap: Option<TrapReason>,
}

/// Sandbox manager statistics.
#[derive(Debug, Clone, Default)]
pub struct SandboxStats {
    pub sandboxes_created: u64,
    pub sandboxes_completed: u64,
    pub sandboxes_trapped: u64,
    pub sandboxes_killed: u64,
    pub total_fuel_consumed: u64,
    pub total_memory_allocated: u64,
}

/// The Sandbox Manager.
pub struct SandboxManager {
    /// Active sandboxes
    pub sandboxes: BTreeMap<u64, Sandbox>,
    /// Next sandbox ID
    next_id: u64,
    /// Default resource limits
    pub default_limits: ResourceLimits,
    /// Statistics
    pub stats: SandboxStats,
}

impl SandboxManager {
    pub fn new() -> Self {
        SandboxManager {
            sandboxes: BTreeMap::new(),
            next_id: 1,
            default_limits: ResourceLimits::default(),
            stats: SandboxStats::default(),
        }
    }

    /// Create a new sandbox for a Wasm module.
    pub fn create(
        &mut self,
        module_name: &str,
        module_hash: [u8; 32],
        silo_id: u64,
        capabilities: u64,
        limits: Option<ResourceLimits>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let lim = limits.unwrap_or_else(|| self.default_limits.clone());

        self.sandboxes.insert(id, Sandbox {
            id,
            state: SandboxState::Created,
            silo_id,
            module_name: String::from(module_name),
            module_hash,
            memory_pages: 0,
            limits: lim,
            usage: ResourceUsage::default(),
            imports: Vec::new(),
            exports: Vec::new(),
            capabilities,
            return_value: None,
            trap: None,
        });

        self.stats.sandboxes_created += 1;
        id
    }

    /// Load and validate a Wasm module.
    pub fn load(&mut self, sandbox_id: u64) -> Result<(), &'static str> {
        let sb = self.sandboxes.get_mut(&sandbox_id)
            .ok_or("Sandbox not found")?;

        if sb.state != SandboxState::Created {
            return Err("Sandbox not in created state");
        }

        // In production: parse Wasm binary, validate types, compile
        sb.memory_pages = 1; // Initial page
        sb.exports.push(WasmExport {
            name: String::from("_start"),
            param_count: 0,
            result_count: 1,
        });

        sb.state = SandboxState::Loaded;
        Ok(())
    }

    /// Execute a sandbox (run to completion or trap).
    pub fn run(&mut self, sandbox_id: u64) -> Result<i64, TrapReason> {
        let sb = self.sandboxes.get_mut(&sandbox_id)
            .ok_or(TrapReason::Unreachable)?;

        if sb.state != SandboxState::Loaded && sb.state != SandboxState::Paused {
            return Err(TrapReason::Unreachable);
        }

        sb.state = SandboxState::Running;

        // Simulate execution with fuel metering
        let fuel_per_step = 1000u64;
        let steps = 100u64;

        for _ in 0..steps {
            sb.usage.fuel_consumed += fuel_per_step;

            if sb.usage.fuel_consumed > sb.limits.max_fuel {
                sb.state = SandboxState::Trapped;
                sb.trap = Some(TrapReason::FuelExhausted);
                self.stats.sandboxes_trapped += 1;
                return Err(TrapReason::FuelExhausted);
            }
        }

        // Successful completion
        sb.state = SandboxState::Finished;
        sb.return_value = Some(0);
        self.stats.sandboxes_completed += 1;
        self.stats.total_fuel_consumed += sb.usage.fuel_consumed;
        Ok(0)
    }

    /// Kill a sandbox (Sentinel enforcement).
    pub fn kill(&mut self, sandbox_id: u64) {
        if let Some(sb) = self.sandboxes.get_mut(&sandbox_id) {
            sb.state = SandboxState::Killed;
            sb.trap = Some(TrapReason::KilledBySentinel);
            self.stats.sandboxes_killed += 1;
        }
    }

    /// Check if a sandbox can call a specific import.
    pub fn check_capability(&self, sandbox_id: u64, required: u64) -> bool {
        self.sandboxes.get(&sandbox_id)
            .map(|sb| sb.capabilities & required != 0)
            .unwrap_or(false)
    }

    /// Get active sandbox count.
    pub fn active_count(&self) -> usize {
        self.sandboxes.values()
            .filter(|sb| sb.state == SandboxState::Running || sb.state == SandboxState::Paused)
            .count()
    }
}
