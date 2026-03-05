//! # Genesis Protocol — First-Boot Initialization
//!
//! The Genesis Protocol runs exactly once — the first time a Qindows
//! system powers on. It probes hardware, creates the system identity,
//! establishes The Ledger root of trust, and bootstraps all subsystems.
//!
//! Genesis sequence (Section 12 of the spec):
//! 1. **Hardware Survey**: Probe CPU, RAM, GPU, NVMe, NIC via ACPI/SMBIOS
//! 2. **Identity Creation**: Generate Ed25519 keypair (device identity)
//! 3. **Ledger Init**: Create the root-of-trust Ledger entry
//! 4. **Prism Format**: Initialize the storage engine on primary NVMe
//! 5. **Silo Zero**: Create the System Silo (Ring 0 bootstrap)
//! 6. **Sentinel Boot**: Start the AI security overseer
//! 7. **Mesh Join**: Announce presence to the Global Mesh via mDNS
//! 8. **OOBE**: Launch the Out-of-Box Experience (user setup wizard)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Genesis phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenesisPhase {
    /// Not yet started
    PreGenesis,
    /// Step 1: Hardware survey
    HardwareSurvey,
    /// Step 2: Identity creation
    IdentityCreation,
    /// Step 3: Ledger initialization
    LedgerInit,
    /// Step 4: Prism format
    PrismFormat,
    /// Step 5: Silo Zero bootstrap
    SiloZero,
    /// Step 6: Sentinel boot
    SentinelBoot,
    /// Step 7: Mesh join
    MeshJoin,
    /// Step 8: Out-of-Box Experience
    OOBE,
    /// Genesis complete — normal boot from now on
    Complete,
    /// Genesis failed (critical error)
    Failed,
}

/// Hardware survey results.
#[derive(Debug, Clone)]
pub struct HardwareSurvey {
    /// CPU model name
    pub cpu_model: String,
    /// CPU cores (physical)
    pub cpu_cores: u32,
    /// CPU threads (logical)
    pub cpu_threads: u32,
    /// CPU features detected
    pub cpu_features: Vec<String>,
    /// Total RAM (bytes)
    pub ram_bytes: u64,
    /// NUMA nodes detected
    pub numa_nodes: u32,
    /// GPU model
    pub gpu_model: String,
    /// GPU VRAM (bytes)
    pub gpu_vram: u64,
    /// NVMe devices detected
    pub nvme_devices: Vec<StorageDevice>,
    /// Network interfaces
    pub nic_count: u32,
    /// TPM version (None if not present)
    pub tpm_version: Option<String>,
    /// Secure Boot capable
    pub secure_boot: bool,
}

/// A detected storage device.
#[derive(Debug, Clone)]
pub struct StorageDevice {
    pub name: String,
    pub capacity_bytes: u64,
    pub model: String,
    pub is_nvme: bool,
}

/// System identity (created during Genesis).
#[derive(Debug, Clone)]
pub struct SystemIdentity {
    /// Ed25519 public key (device identity on the mesh)
    pub public_key: [u8; 32],
    /// Device name (user-chosen during OOBE)
    pub device_name: String,
    /// Genesis timestamp
    pub genesis_time: u64,
    /// Hardware fingerprint (hash of survey)
    pub hw_fingerprint: [u8; 32],
    /// Qindows version string
    pub version: String,
}

/// Genesis progress tracking.
#[derive(Debug, Clone)]
pub struct GenesisProgress {
    pub phase: GenesisPhase,
    pub progress_pct: u8,
    pub message: String,
    pub errors: Vec<String>,
}

/// The Genesis Protocol Engine.
pub struct GenesisProtocol {
    /// Current phase
    pub phase: GenesisPhase,
    /// Hardware survey results
    pub hardware: Option<HardwareSurvey>,
    /// System identity
    pub identity: Option<SystemIdentity>,
    /// Progress log
    pub log: Vec<GenesisProgress>,
    /// Is this a fresh install? (false = already completed Genesis)
    pub is_fresh: bool,
}

impl GenesisProtocol {
    pub fn new() -> Self {
        GenesisProtocol {
            phase: GenesisPhase::PreGenesis,
            hardware: None,
            identity: None,
            log: Vec::new(),
            is_fresh: true,
        }
    }

    /// Check if Genesis has already been completed (read from Prism).
    pub fn check_completed(&mut self) -> bool {
        // In production: check for genesis marker in Prism
        // If found, skip Genesis entirely
        !self.is_fresh
    }

    /// Execute the next Genesis step.
    pub fn step(&mut self, now: u64) -> GenesisPhase {
        match self.phase {
            GenesisPhase::PreGenesis => {
                self.log_progress(0, "Starting Genesis Protocol...");
                self.phase = GenesisPhase::HardwareSurvey;
            }
            GenesisPhase::HardwareSurvey => {
                self.log_progress(10, "Probing hardware via ACPI/SMBIOS...");
                self.hardware = Some(self.probe_hardware());
                self.phase = GenesisPhase::IdentityCreation;
            }
            GenesisPhase::IdentityCreation => {
                self.log_progress(25, "Generating device identity (Ed25519)...");
                self.identity = Some(SystemIdentity {
                    public_key: self.generate_keypair(now),
                    device_name: String::from("Qindows-Device"),
                    genesis_time: now,
                    hw_fingerprint: self.compute_hw_fingerprint(),
                    version: String::from("1.0.0-genesis"),
                });
                self.phase = GenesisPhase::LedgerInit;
            }
            GenesisPhase::LedgerInit => {
                self.log_progress(40, "Initializing The Ledger (root of trust)...");
                // In production: create signed Ledger entry with device key
                self.phase = GenesisPhase::PrismFormat;
            }
            GenesisPhase::PrismFormat => {
                self.log_progress(55, "Formatting primary NVMe with Prism...");
                // In production: initialize WAL, B-tree root, superblock
                self.phase = GenesisPhase::SiloZero;
            }
            GenesisPhase::SiloZero => {
                self.log_progress(70, "Creating System Silo (Ring 0)...");
                // In production: create the bootstrap Silo with full caps
                self.phase = GenesisPhase::SentinelBoot;
            }
            GenesisPhase::SentinelBoot => {
                self.log_progress(80, "Starting Sentinel AI security overseer...");
                // In production: load Sentinel ML models, set baseline
                self.phase = GenesisPhase::MeshJoin;
            }
            GenesisPhase::MeshJoin => {
                self.log_progress(90, "Joining the Global Mesh via mDNS...");
                // In production: broadcast identity, discover peers
                self.phase = GenesisPhase::OOBE;
            }
            GenesisPhase::OOBE => {
                self.log_progress(100, "Genesis complete. Launching OOBE...");
                self.is_fresh = false;
                self.phase = GenesisPhase::Complete;
            }
            GenesisPhase::Complete | GenesisPhase::Failed => {}
        }
        self.phase
    }

    /// Run the full Genesis sequence at once.
    pub fn run_full(&mut self, now: u64) -> Result<(), &'static str> {
        while self.phase != GenesisPhase::Complete && self.phase != GenesisPhase::Failed {
            self.step(now);
        }
        if self.phase == GenesisPhase::Failed {
            Err("Genesis failed")
        } else {
            Ok(())
        }
    }

    /// Probe hardware (simplified — production reads ACPI/SMBIOS/PCI).
    fn probe_hardware(&self) -> HardwareSurvey {
        HardwareSurvey {
            cpu_model: String::from("Qindows Virtual CPU"),
            cpu_cores: 8,
            cpu_threads: 16,
            cpu_features: alloc::vec![
                String::from("SSE4.2"), String::from("AVX2"),
                String::from("AES-NI"), String::from("RDRAND"),
                String::from("RDSEED"), String::from("x2APIC"),
            ],
            ram_bytes: 32 * 1024 * 1024 * 1024, // 32 GiB
            numa_nodes: 1,
            gpu_model: String::from("Qindows Virtual GPU"),
            gpu_vram: 8 * 1024 * 1024 * 1024, // 8 GiB
            nvme_devices: alloc::vec![StorageDevice {
                name: String::from("nvme0n1"),
                capacity_bytes: 1024 * 1024 * 1024 * 1024, // 1 TiB
                model: String::from("Qindows Virtual NVMe"),
                is_nvme: true,
            }],
            nic_count: 1,
            tpm_version: Some(String::from("3.0")),
            secure_boot: true,
        }
    }

    /// Generate Ed25519 keypair (simplified).
    fn generate_keypair(&self, seed: u64) -> [u8; 32] {
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = ((seed >> (i % 8)) as u8)
                .wrapping_mul(0x9E)
                .wrapping_add(i as u8);
        }
        key
    }

    /// Compute hardware fingerprint.
    fn compute_hw_fingerprint(&self) -> [u8; 32] {
        let mut hash = [0u8; 32];
        if let Some(hw) = &self.hardware {
            for (i, byte) in hw.cpu_model.bytes().enumerate() {
                hash[i % 32] ^= byte;
            }
            let ram_bytes = hw.ram_bytes.to_le_bytes();
            for i in 0..8 { hash[i + 16] ^= ram_bytes[i]; }
        }
        hash
    }

    fn log_progress(&mut self, pct: u8, msg: &str) {
        self.log.push(GenesisProgress {
            phase: self.phase,
            progress_pct: pct,
            message: String::from(msg),
            errors: Vec::new(),
        });
    }
}
