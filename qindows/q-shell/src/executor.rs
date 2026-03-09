//! # Q-Shell Execution Engine
//!
//! Executes parsed command pipelines by routing them through
//! the appropriate subsystem (Prism, Aether, Nexus, etc.)
//! via Q-Ring IPC.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;
use prism::{PrismGraph, QNode, ObjectMetadata, OID};
use nexus::{QNexus, PeerIdentity, HardwareProfile, SerializedFiber};
use nexus::sentinel::Sentinel;

static PRISM_GRAPH: Mutex<Option<PrismGraph>> = Mutex::new(None);

fn get_prism() -> spin::MutexGuard<'static, Option<PrismGraph>> {
    let mut guard = PRISM_GRAPH.lock();
    if guard.is_none() {
        *guard = Some(PrismGraph::new());
    }
    guard
}

fn hash_string(s: &str) -> OID {
    let mut oid = [0u8; 32];
    for (i, b) in s.bytes().enumerate() {
        oid[i % 32] ^= b;
        oid[(i + 7) % 32] = oid[(i + 7) % 32].wrapping_add(b);
    }
    oid
}

fn format_oid(oid: &OID) -> String {
    alloc::format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", 
        oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7])
}

static NEXUS_ENGINE: Mutex<Option<QNexus>> = Mutex::new(None);

fn get_nexus() -> spin::MutexGuard<'static, Option<QNexus>> {
    let guard = NEXUS_ENGINE.lock();
    if guard.is_none() {
        // Safety: we hold the mutex, so we can safely re-lock as a write.
        // We drop the read guard and re-acquire as mutable.
        drop(guard);
        let mut wguard = NEXUS_ENGINE.lock();
        if wguard.is_none() {
            let local_identity = PeerIdentity {
                node_id: [0; 32],
                alias: String::from("qindows-local-host"),
                capabilities: HardwareProfile {
                    cpu_cores: 4,
                    gpu_units: 1,
                    has_npu: false,
                    ram_mb: 512,
                    bandwidth_mbps: 1000,
                },
                availability: 0.8,
                reputation: 100,
            };

            let mut nexus = QNexus {
                peers: alloc::vec![],
                local_identity,
                offloaded_tasks: alloc::vec![],
                credits_earned: 0,
                fibers_processed: 0,
            };

            nexus.peers.push(PeerIdentity {
                node_id: [1; 32],
                alias: String::from("node-alpha-7a3f"),
                capabilities: HardwareProfile { cpu_cores: 32, gpu_units: 4, has_npu: true, ram_mb: 65536, bandwidth_mbps: 10000 },
                availability: 0.9,
                reputation: 98,
            });
            nexus.peers.push(PeerIdentity {
                node_id: [2; 32],
                alias: String::from("node-beta-2c8d"),
                capabilities: HardwareProfile { cpu_cores: 8, gpu_units: 0, has_npu: false, ram_mb: 16384, bandwidth_mbps: 1000 },
                availability: 0.4,
                reputation: 95,
            });
            nexus.peers.push(PeerIdentity {
                node_id: [3; 32],
                alias: String::from("node-gamma-9e1f"),
                capabilities: HardwareProfile { cpu_cores: 16, gpu_units: 1, has_npu: false, ram_mb: 32768, bandwidth_mbps: 500 },
                availability: 0.1,
                reputation: 72,
            });

            // ← Bug 1 Fix: Store the fully constructed nexus into the global.
            *wguard = Some(nexus);
        }
        return wguard;
    }
    guard
}

static SENTINEL_ENGINE: Mutex<Option<Sentinel>> = Mutex::new(None);

fn get_sentinel() -> spin::MutexGuard<'static, Option<Sentinel>> {
    let mut guard = SENTINEL_ENGINE.lock();
    if guard.is_none() {
        let mut sentinel = Sentinel::new();
        // Register default Silos
        sentinel.register_silo(1); // qernel
        sentinel.register_silo(2); // sentinel (self)
        sentinel.register_silo(3); // aether
        sentinel.register_silo(4); // q-shell
        sentinel.register_silo(5); // prism-daemon
        
        // Emulate some baseline profiles.
        // `now` is the boot epoch tick — use 1_000_000 as a sensible non-zero
        // baseline so the EMA timestamps are representatively seeded.
        let boot_now: u64 = 1_000_000;
        sentinel.update_profile(1, 1000.0, 12_582_912, 0, 500.0, boot_now);
        sentinel.update_profile(3, 150.0, 33_554_432, 0, 250.0, boot_now);
        sentinel.update_profile(4, 10.0, 4_194_304, 0, 15.0, boot_now);
        
        // Simulate a past isolated threat
        sentinel.events.push(nexus::sentinel::SecurityEvent {
            id: 1,
            event_type: nexus::sentinel::EventType::AnomalousSyscall,
            severity: nexus::sentinel::Severity::Medium,
            silo_id: 14,
            timestamp: 21044,
            description: String::from("Excessive MapShared calls detected"),
            mitigated: true,
            action: nexus::sentinel::ResponseAction::RateLimit,
        });
        sentinel.stats.events_logged += 1;
        sentinel.stats.anomalies_detected += 1;
        sentinel.stats.silos_rate_limited += 1;

        *guard = Some(sentinel);
    }
    guard
}

/// Result of executing a command.
#[derive(Debug)]
pub enum CommandResult {
    /// Success with optional output text
    Success(Option<String>),
    /// Error with message
    Error(String),
    /// Output is a list of items (for piping to next command)
    List(Vec<String>),
    /// Output is structured data (for ~> flow operator)
    Data(Vec<(String, String)>),
    /// Command did not produce output (side effect only)
    Silent,
    /// Request to exit Q-Shell
    Exit,
}

/// Built-in command handlers.
pub fn execute_builtin(cmd: &str, args: &[&str]) -> CommandResult {
    match cmd {
        "help" => cmd_help(),
        "version" => cmd_version(),
        "silo" => cmd_silo(args),
        "prism" => cmd_prism(args),
        "mesh" => cmd_mesh(args),
        "sentinel" => cmd_sentinel(args),
        "power" => cmd_power(args),
        "pci" => cmd_pci(args),
        "memory" => cmd_memory(args),
        "genesis" => cmd_genesis(),
        "clear" => CommandResult::Silent,
        "exit" | "quit" => CommandResult::Exit,
        unknown => CommandResult::Error(
            alloc::format!("Unknown command: '{}'. Type 'help' for available commands.", unknown),
        ),
    }
}

fn cmd_help() -> CommandResult {
    CommandResult::Success(Some(String::from(
        "Q-Shell v1.0.0 — Semantic Command Palette\n\
         \n\
         BUILT-IN COMMANDS:\n\
         \n\
         SYSTEM:\n\
         │ help           Show this help\n\
         │ version        Display Qindows version\n\
         │ clear          Clear console output\n\
         │ exit           Exit Q-Shell\n\
         \n\
         PRISM (Object Storage):\n\
         │ prism find     Semantic search objects\n\
         │ prism get      Retrieve object by OID\n\
         │ prism store    Store a new object\n\
         │ prism history  Show version lineage\n\
         │ prism stats    Storage statistics\n\
         \n\
         SILO (Process Isolation):\n\
         │ silo list      List active Q-Silos\n\
         │ silo spawn     Launch a new Silo\n\
         │ silo inspect   Detailed Silo info\n\
         │ silo vaporize  Remove a Silo\n\
         \n\
         MESH (Global Network):\n\
         │ mesh status    Network mesh status\n\
         │ mesh peers     List connected peers\n\
         │ mesh ping      Ping a mesh node\n\
         │ mesh credits   View Q-Credits balance\n\
         \n\
         SENTINEL (Security):\n\
         │ sentinel status  AI auditor status\n\
         │ sentinel log     View violation log\n\
         │ sentinel laws    Display Q-Manifest laws\n\
         \n\
         HARDWARE:\n\
         │ pci list       List PCI devices\n\
         │ memory stats   Memory usage\n\
         │ power status   Power/battery status\n\
         \n\
         FLOW OPERATOR:\n\
         │ Use ~> to chain commands:\n\
         │ prism find photos ~> sort date ~> take 5",
    )))
}

fn cmd_version() -> CommandResult {
    CommandResult::Success(Some(String::from(
        "Qindows v1.0.0 Genesis Alpha\n\
         Qernel: Microkernel RS-1.0\n\
         Prism: Semantic Object Storage\n\
         Aether: Vector Compositor\n\
         Built: March 4, 2026"
    )))
}

fn cmd_silo(args: &[&str]) -> CommandResult {
    let mut guard = get_sentinel();
    let sentinel = guard.as_mut().unwrap();

    match args.first() {
        Some(&"list") => {
            let silo_names: &[(u64, &str, &str)] = &[
                (1, "qernel",       "Ring-0"),
                (2, "sentinel",     "Ring-0"),
                (3, "aether",       "Ring-3"),
                (4, "q-shell",      "Ring-3"),
                (5, "prism-daemon", "Ring-3"),
            ];
            let mut lines = Vec::new();
            for &(id, name, ring) in silo_names {
                let trust = sentinel.trust_score(id);
                let bar_fill = (trust as usize * 12) / 100;
                let bar: String = core::iter::repeat('█').take(bar_fill)
                    .chain(core::iter::repeat('░').take(12 - bar_fill))
                    .collect();
                lines.push(alloc::format!(
                    "[{:04}] {:<14} ACTIVE  {}  {} {:>3}%",
                    id, name, ring, bar, trust
                ));
            }
            CommandResult::List(lines)
        }
        Some(&"inspect") => {
            let id_str = args.get(1).unwrap_or(&"1");
            let id: u64 = id_str.parse().unwrap_or(1);
            let names: &[(u64, &str)] = &[(1,"qernel"),(2,"sentinel"),(3,"aether"),(4,"q-shell"),(5,"prism-daemon")];
            let name = names.iter().find(|(i,_)| *i == id).map(|(_,n)| *n).unwrap_or("unknown");

            if let Some(profile) = sentinel.profiles.get(&id) {
                CommandResult::Success(Some(alloc::format!(
                    "Silo {} ({}) — Inspection Report\n\
                     ─────────────────────────\n\
                     State:      ACTIVE\n\
                     Ring:       {}\n\
                     Syscalls:   {:.1}/sec (EMA baseline)\n\
                     Memory:     {} KiB\n\
                     IPC Rate:   {:.1} msg/sec\n\
                     Trust:      {}/100\n\
                     Violations: {}",
                    id, name,
                    if id <= 2 { "0 (Kernel Mode)" } else { "3 (User Mode)" },
                    profile.avg_syscalls_per_sec,
                    profile.avg_memory / 1024,
                    profile.avg_ipc_per_sec,
                    profile.trust_score,
                    profile.violations
                )))
            } else {
                CommandResult::Error(alloc::format!("Silo {} not registered with Sentinel.", id))
            }
        }
        Some(&"spawn") => CommandResult::Success(Some(String::from(
            "Spawning new Q-Silo... [OK] Silo #0006 created"
        ))),
        Some(&"vaporize") => CommandResult::Success(Some(String::from(
            "⚠ Vaporizing Silo... pages zeroed, caps revoked. [OK]"
        ))),
        _ => CommandResult::Error(String::from("Usage: silo [list|spawn|inspect|vaporize]")),
    }
}

fn cmd_prism(args: &[&str]) -> CommandResult {
    let mut guard = get_prism();
    let graph = guard.as_mut().unwrap();

    match args.first() {
        Some(&"store") => {
            let label = args.get(1).unwrap_or(&"unlabeled");
            let content = args.get(2..).map(|a| a.join(" ")).unwrap_or_default();
            
            let oid = hash_string(&content);
            let node = QNode {
                oid,
                entropy_key: [0; 32],
                vector_hash: {
                    // Compute semantic vector from label via FNV-1a hash
                    let mut h: u64 = 0xcbf29ce484222325;
                    for b in label.bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    // Split 64-bit hash into 3 normalized [0.0, 1.0] floats
                    [
                        ((h & 0xFFFFF) as f32) / 0xFFFFF as f32,
                        (((h >> 20) & 0xFFFFF) as f32) / 0xFFFFF as f32,
                        (((h >> 40) & 0xFFFFF) as f32) / 0xFFFFF as f32,
                    ]
                },
                lineage: None,
                metadata: ObjectMetadata {
                    label: String::from(*label),
                    tags: alloc::vec![],
                    created_at: 0,
                    accessed_at: 0,
                    size_bytes: content.len() as u64,
                    content_type: String::from("text/plain"),
                    creator_silo: 6,
                },
            };
            
            graph.store(node);
            CommandResult::Success(Some(alloc::format!("Object stored! [OID:{}]", format_oid(&oid))))
        }
        Some(&"find") => {
            let query = args.get(1..).map(|a| a.join(" ")).unwrap_or_default();
            let results = graph.resolve_intent(&query, 5);
            
            if results.is_empty() {
                return CommandResult::Success(Some(String::from("No objects found in PrismGraph.")));
            }
            
            let mut lines = alloc::vec![alloc::format!("🔍 Semantic Search: \"{}\"", query)];
            for r in results {
                lines.push(alloc::format!("   [OID:{}] {} ({} bytes)", format_oid(&r.oid), r.metadata.label, r.metadata.size_bytes));
            }
            CommandResult::List(lines)
        }
        Some(&"history") => {
            let target_str = args.get(1).unwrap_or(&"");
            // Parse hex OID prefix and search lineage
            if target_str.is_empty() {
                return CommandResult::Error(String::from("Usage: prism history <oid-prefix>"));
            }
            // Find first object whose formatted OID starts with the target prefix
            let mut found_oid: Option<OID> = None;
            for node in graph.resolve_intent("", 100) {
                if format_oid(&node.oid).starts_with(target_str) {
                    found_oid = Some(node.oid);
                    break;
                }
            }
            match found_oid {
                Some(oid) => {
                    let chain = graph.get_lineage(&oid);
                    if chain.is_empty() {
                        CommandResult::Success(Some(String::from("No lineage found.")))
                    } else {
                        let mut lines = alloc::vec![alloc::format!("Version lineage for [OID:{}]", format_oid(&oid))];
                        for (i, node) in chain.iter().enumerate() {
                            lines.push(alloc::format!("  v{}: [{}] {} ({} bytes)",
                                chain.len() - i,
                                format_oid(&node.oid),
                                node.metadata.label,
                                node.metadata.size_bytes));
                        }
                        CommandResult::List(lines)
                    }
                }
                None => CommandResult::Error(alloc::format!("No object with OID prefix '{}' found.", target_str)),
            }
        }
        Some(&"stats") => {
            // Bug 11 Fix: query the live graph for the real object count.
            let count = graph.object_count();
            // Compute approximate log2 depth via integer arithmetic (no trait import needed).
            let depth = if count == 0 { 0 } else {
                let mut d = 0usize;
                let mut n = count;
                while n > 1 { n >>= 1; d += 1; }
                d + 1
            };
            CommandResult::Data(alloc::vec![
                (String::from("Objects"), alloc::format!("{}", count)),
                (String::from("B-Tree Depth"), alloc::format!("{} levels", depth)),
                (String::from("Journal"), String::from("Live in memory")),
            ])
        }
        _ => CommandResult::Error(String::from("Usage: prism [find|store|history|stats]")),
    }
}

fn cmd_mesh(args: &[&str]) -> CommandResult {
    let mut guard = get_nexus();
    let nexus = guard.as_mut().unwrap();

    match args.first() {
        Some(&"status") => CommandResult::Success(Some(alloc::format!(
            "⬡ Global Mesh Status\n\
             ────────────────────\n\
             Local Identity: {}\n\
             State:          CONNECTED\n\
             Peers:          {} active nodes\n\
             Fibers Pushed:  {} pipelines sent\n\
             Credits:        {} Q₵ earned",
             nexus.local_identity.alias,
             nexus.peers.len(),
             nexus.fibers_processed,
             nexus.credits_earned
        ))),
        Some(&"peers") => {
            let mut lines = alloc::vec![String::from("🔌 Active Q-Nexus Planetary Peers:")];
            for p in &nexus.peers {
                let status = if p.availability > 0.5 { "🟢" } else if p.availability > 0.2 { "🟡" } else { "🔴" };
                lines.push(alloc::format!("{} {:<18} Rep: {:<3} RAM: {:<6}MB GPU: {}",
                    status, p.alias, p.reputation, p.capabilities.ram_mb, p.capabilities.gpu_units));
            }
            CommandResult::List(lines)
        }
        Some(&"offload") => {
            let task_name = args.get(1).unwrap_or(&"unknown_task");
            // Simulate serializing a Q-Shell fiber task
            let sfiber = SerializedFiber {
                source_silo: 6,
                registers: alloc::vec![0; 128],
                memory_snapshot: alloc::vec![],
                required_caps: alloc::vec![],
            };

            if let Some(task_id) = nexus.offload_fiber(sfiber) {
                nexus.fibers_processed += 1;
                nexus.credits_earned += 12; // Simulate earning credits for sharing pipeline
                CommandResult::Success(Some(alloc::format!("🚀 Fiber '{}' successfully offloaded! [Task ID: {}]", task_name, task_id)))
            } else {
                CommandResult::Error(String::from("No peers available dynamically mapping required hardware caps."))
            }
        }
        _ => CommandResult::Error(String::from("Usage: mesh [status|peers|offload]")),
    }
}

fn cmd_sentinel(args: &[&str]) -> CommandResult {
    let mut guard = get_sentinel();
    let sentinel = guard.as_mut().unwrap();

    match args.first() {
        Some(&"status") => CommandResult::Success(Some(alloc::format!(
            "🛡 Sentinel AI Auditor — ACTIVE\n\
             ──────────────────────────────\n\
             Laws Enforced:    10/10\n\
             Silos Monitored:  {}\n\
             Events Logged:    {}\n\
             Anomalies:        {}\n\
             Silos Killed:     {}\n\
             False Positives:  {}",
             sentinel.profiles.len(),
             sentinel.stats.events_logged,
             sentinel.stats.anomalies_detected,
             sentinel.stats.silos_killed,
             sentinel.stats.false_positives
        ))),
        Some(&"laws") => CommandResult::List(alloc::vec![
            String::from("I.   Zero Ambient Authority — Apps launch with nothing"),
            String::from("II.  Immutable Binaries — No self-modifying code"),
            String::from("III. Asynchronous Everything — All I/O through Q-Ring"),
            String::from("IV.  Vector Native UI — No bitmaps, only SDF math"),
            String::from("V.   Global Deduplication — One copy, many views"),
            String::from("VI.  Silo Sandbox — Every app in hardware isolation"),
            String::from("VII. Telemetry Transparency — No silent network calls"),
            String::from("VIII.Energy Proportionality — Background deep-slept"),
            String::from("IX.  Universal Namespace — Location-transparent data"),
            String::from("X.   Graceful Degradation — Offline-first design"),
        ]),
        Some(&"log") => {
            if sentinel.events.is_empty() {
                CommandResult::Success(Some(String::from("No security events logged.")))
            } else {
                let mut lines = alloc::vec![String::from("📜 Sentinel Threat Log:")];
                for ev in &sentinel.events {
                    lines.push(alloc::format!("[{}] {:?} (Silo {}) -> {:?}", ev.timestamp, ev.event_type, ev.silo_id, ev.action));
                }
                CommandResult::List(lines)
            }
        }
        _ => CommandResult::Error(String::from("Usage: sentinel [status|log|laws]")),
    }
}

fn cmd_genesis() -> CommandResult {
    let mut guard = get_nexus();
    let nexus = guard.as_mut().unwrap();

    let phases = nexus::initiate_genesis(nexus);

    let mut lines = alloc::vec![
        String::from("☉ Initiating GENESIS PROTOCOL — Planetary Mesh Activation"),
        String::from("──────────────────────────────────────────────────────────"),
    ];

    for (phase, status) in &phases {
        let phase_name = match phase {
            nexus::GenesisPhase::Beacon        => "Phase I:   Beacon         [Cryptographic Handshake]",
            nexus::GenesisPhase::AetherSync    => "Phase II:  Aether-Sync    [Global PTP Calibration]",
            nexus::GenesisPhase::PrismUnfold   => "Phase III: Prism-Unfold   [Planetary Deduplication]",
            nexus::GenesisPhase::SentinelShield => "Phase IV:  Sentinel-Shield [Immunity Propagation]",
        };
        let status_str = match status {
            nexus::GenesisStatus::Ok       => "[OK]      ",
            nexus::GenesisStatus::Degraded => "[DEGRADED]",
            nexus::GenesisStatus::Failed   => "[FAILED]  ",
        };
        lines.push(alloc::format!("{} {}", status_str, phase_name));
    }

    // Read back mesh state after protocol run
    let peers = nexus.peers.len();
    let credits = nexus.credits_earned;

    lines.push(String::from("──────────────────────────────────────────────────────────"));
    lines.push(alloc::format!("Mesh Nodes:  {} active peers recognized", peers));
    lines.push(alloc::format!("Q-Credits:   {} Q\u{20B5} earned this session", credits));
    lines.push(String::from(""));
    lines.push(String::from("+----------------------------------------------------------+"));
    lines.push(String::from("|        ★  THE MESH IS ALIVE. GENESIS COMPLETE.  ★        |"));
    lines.push(String::from("+----------------------------------------------------------+"));

    CommandResult::List(lines)
}

fn cmd_power(args: &[&str]) -> CommandResult {
    match args.first() {
        Some(&"status") => CommandResult::Data(alloc::vec![
            (String::from("State"), String::from("S0 (Active)")),
            (String::from("Policy"), String::from("Adaptive")),
            (String::from("CPU Freq"), String::from("3.2 GHz (scaled)")),
            (String::from("Temperature"), String::from("52°C")),
            (String::from("Power Draw"), String::from("28W")),
            (String::from("Battery"), String::from("N/A (AC Power)")),
        ]),
        _ => CommandResult::Error(String::from("Usage: power [status]")),
    }
}

fn cmd_pci(args: &[&str]) -> CommandResult {
    match args.first() {
        Some(&"list") => CommandResult::List(alloc::vec![
            String::from("00:00.0 Host Bridge              Intel Corp"),
            String::from("00:02.0 VGA Compatible Controller Intel UHD 770"),
            String::from("00:1f.0 ISA Bridge               Intel Q670"),
            String::from("00:1f.2 SATA Controller           Intel AHCI"),
            String::from("01:00.0 NVMe Controller           Samsung 990 PRO"),
            String::from("02:00.0 Ethernet Controller       Intel I225-V"),
            String::from("03:00.0 USB Controller            Intel xHCI"),
        ]),
        _ => CommandResult::Error(String::from("Usage: pci [list]")),
    }
}

fn cmd_memory(args: &[&str]) -> CommandResult {
    // Real heap constants from qernel/src/memory/heap.rs
    let heap_size_kib: u64 = 4 * 1024; // 4 MiB = 4096 KiB
    match args.first() {
        Some(&"stats") => {
            let graph_guard = get_prism();
            let graph = graph_guard.as_ref();
            let obj_count = graph.map(|g| g.object_count()).unwrap_or(0);
            // Estimate used heap from object count (~512 bytes per QNode)
            let estimated_used_kib = (obj_count as u64 * 512) / 1024 + 64; // +64 KiB base overhead
            let free_kib = heap_size_kib.saturating_sub(estimated_used_kib);
            let used_pct = if heap_size_kib > 0 { estimated_used_kib * 100 / heap_size_kib } else { 0 };

            CommandResult::Data(alloc::vec![
                (String::from("Heap Total"), alloc::format!("{} KiB", heap_size_kib)),
                (String::from("Heap Used"), alloc::format!("~{} KiB ({}%)", estimated_used_kib, used_pct)),
                (String::from("Heap Free"), alloc::format!("~{} KiB", free_kib)),
                (String::from("Prism Objects"), alloc::format!("{}", obj_count)),
                (String::from("Heap Start"), String::from("0x0100_0000 (16 MiB)")),
                (String::from("Allocator"), String::from("LinkedList (first-fit)")),
            ])
        }
        _ => CommandResult::Error(String::from("Usage: memory [stats]")),
    }
}

/// Execute a full pipeline and return the consolidated string output (Fix #11).
///
/// The `~>` (flow) operator passes the output of each stage as an implicit
/// first argument to the next stage, enabling semantic chaining:
///
///   `prism find "invoices" ~> q_analyze summarize --format:csv ~> vault export`
///
/// Each stage receives the previous output as an extra leading argument.
/// If any stage returns an error, the pipeline short-circuits.
pub fn execute_pipeline(pipeline: &crate::Pipeline) -> String {
    if pipeline.stages.is_empty() {
        return String::new();
    }

    // Accumulator: the output of the previous stage (starts empty)
    let mut pipe_input: Option<String> = None;

    for stage in &pipeline.stages {
        // Build argument list for this stage
        let mut args: Vec<String> = Vec::new();

        // Pipe the previous stage's output as the first arg (the ~> semantics)
        if let Some(ref prev_out) = pipe_input {
            if !prev_out.is_empty() {
                args.push(prev_out.clone());
            }
        }

        // Append sub_command and explicit args
        if let Some(ref sub) = stage.sub_command {
            args.push(sub.clone());
        }
        for a in &stage.args {
            args.push(a.clone());
        }

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let result = execute_builtin(&stage.command, &arg_refs);

        // Convert result to string; short-circuit on error
        let stage_output = match result {
            CommandResult::Success(Some(msg)) => msg,
            CommandResult::Success(None) => String::new(),
            CommandResult::Error(e) => return alloc::format!("Pipeline error at '{}': {}", stage.command, e),
            CommandResult::List(items) => {
                let mut out = String::new();
                for (i, item) in items.iter().enumerate() {
                    out.push_str(item);
                    if i < items.len() - 1 { out.push('\n'); }
                }
                out
            }
            CommandResult::Data(pairs) => {
                let mut out = String::new();
                for (i, (k, v)) in pairs.iter().enumerate() {
                    out.push_str(&alloc::format!("{:<15} {}", k, v));
                    if i < pairs.len() - 1 { out.push('\n'); }
                }
                out
            }
            CommandResult::Silent => String::new(),
            CommandResult::Exit => return String::from("Q-Shell termination requested."),
        };

        pipe_input = Some(stage_output);
    }

    pipe_input.unwrap_or_default()
}
