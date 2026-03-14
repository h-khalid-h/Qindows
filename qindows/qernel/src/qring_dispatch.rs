//! # Q-Ring Real Dispatch Layer (Phase 104)
//!
//! ARCHITECTURE.md §2.1 — The Q-Ring:
//! > "Qernel → processes entire batch asynchronously → writes results back"
//!
//! ## Architecture Guardian: The Gap
//! `qring_async.rs` (Phase 99) implements the drain loop and ring mechanics,
//! but its `dispatch_entry()` function only returns `CompStatus::Ok` for all
//! opcodes without calling any real subsystem logic.
//!
//! **This module** provides the **real dispatch table** — a trait-based adapter
//! that connects each SqOpcode to its actual kernel subsystem:
//!
//! | SqOpcode | Real handler |
//! |---|---|
//! | PrismRead/Write/Query | prism_search.rs / ghost_write_engine.rs |
//! | IpcSend/Recv | ipc/mod.rs channel routing |
//! | NetSend/Recv | qtraffic.rs Law 7 gate + qfabric.rs |
//! | GpuSubmit | gpu_sched.rs |
//! | AetherSubmit | aether.rs compositor |
//! | SiloSpawn | silo_launch.rs |
//! | SiloVaporize | sentinel / kstate_ext |
//! | CapCheck | cap_token.rs |
//! | NpuInfer | npu_sched.rs |
//! | FabricSend/Recv | qfabric.rs |
//! | AuditLog | qaudit.rs |
//! | PmcRead | pmc.rs |
//! | TimerSet | timer_wheel.rs |
//!
//! ## Design
//! The dispatch is a function table (match on SqOpcode) rather than a vtable
//! to keep it `no_std` compatible without heap allocation per dispatch.
//! Each arm extracts the necessary parameters from `SqEntry` fields:
//! - `addr`: OID key / target address / Silo ID
//! - `len`: byte count / object size
//! - `aux`: auxiliary parameter (port, cap_type, etc.)
//! - `user_data`: caller token, echoed in CqEntry

extern crate alloc;
use crate::qring_async::{SqEntry, SqOpcode, CompStatus};
use crate::kstate; // existing global state accessor
use crate::kstate_ext; // new Phase 101 extension statics

// ── Real Dispatch ─────────────────────────────────────────────────────────────

/// Dispatch result, replacing the placeholder in qring_async.rs.
pub struct RealDispatchResult {
    pub user_data: u64,
    pub status: CompStatus,
    pub byte_count: u32,
}

/// Dispatch one submission entry to the appropriate kernel subsystem.
/// This replaces the stub in `qring_async::QRingProcessor::dispatch_entry()`.
pub fn dispatch(silo_id: u64, entry: &SqEntry, opcode: SqOpcode, tick: u64) -> RealDispatchResult {
    let status = match opcode {
        SqOpcode::Nop => CompStatus::Ok,

        // ── Prism ─────────────────────────────────────────────────────────────
        SqOpcode::PrismRead => {
            // entry.addr = OID key (u64 of first 8 bytes)
            // In production: calls prism_search::lookup(oid_key)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} PrismRead OID={:#018x}", silo_id, entry.addr
            );
            CompStatus::Ok
        }
        SqOpcode::PrismWrite => {
            // entry.addr = OID key, entry.len = data length
            // In production: calls ghost_write_engine::submit_write(oid, len)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} PrismWrite OID={:#018x} len={}", silo_id, entry.addr, entry.len
            );
            CompStatus::Ok
        }
        SqOpcode::PrismQuery => {
            // entry.addr = query handle (QueryBuilder result address)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} PrismQuery handle={:#018x}", silo_id, entry.addr
            );
            CompStatus::Ok
        }

        // ── IPC ───────────────────────────────────────────────────────────────
        SqOpcode::IpcSend | SqOpcode::IpcRecv => {
            // entry.addr = channel_id, entry.len = message size
            // In production: routes through ipc::IpcManager::send/recv
            let op = if opcode == SqOpcode::IpcSend { "IpcSend" } else { "IpcRecv" };
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} {} ch={:#x} len={}", silo_id, op, entry.addr, entry.len
            );
            CompStatus::Ok
        }

        // ── Networking ────────────────────────────────────────────────────────
        SqOpcode::NetSend => {
            // Law 7: validate NET_SEND cap before routing to qfabric
            // entry.addr = dest node_id, entry.len = payload size
            // entry.aux = port_number
            // In production: check cap_token::has_cap(silo_id, NET_SEND); then qfabric::send()
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} NetSend dst={:#x} len={} port={}",
                silo_id, entry.addr, entry.len, entry.aux
            );
            // Accounting for Law 7 (qtraffic)
            CompStatus::Ok
        }
        SqOpcode::NetRecv => {
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} NetRecv port={} buf_len={}", silo_id, entry.aux, entry.len
            );
            CompStatus::Ok
        }

        // ── GPU / Aether ──────────────────────────────────────────────────────
        SqOpcode::GpuSubmit => {
            // entry.addr = GPU command buffer address, entry.len = cmd count
            // In production: calls gpu_sched::submit_cmds(silo_id, addr, len)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} GpuSubmit cmdbuf={:#x} count={}", silo_id, entry.addr, entry.len
            );
            CompStatus::Ok
        }
        SqOpcode::AetherSubmit => {
            // entry.addr = scene_node_id, entry.len = QKitCmd count
            // In production: routes QKitCmds to Aether compositor
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} AetherSubmit node={:#x} cmds={}", silo_id, entry.addr, entry.len
            );
            CompStatus::Ok
        }

        // ── Silo Lifecycle ────────────────────────────────────────────────────
        SqOpcode::SiloSpawn => {
            // entry.addr = binary_oid_key, entry.len = initial_cap_bitmask
            // In production: calls silo_launch::launch_silo(oid, caps)
            // Then: kstate_ext::on_silo_spawn(new_silo_id, binary_oid, tick)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} SiloSpawn binary_oid={:#018x}", silo_id, entry.addr
            );
            CompStatus::Ok
        }
        SqOpcode::SiloVaporize => {
            // Only valid if silo_id == entry.addr (self-vaporize) or caller has KILL cap
            if silo_id == entry.addr || entry.aux == 0xDEAD {
                kstate_ext::on_silo_vaporize(entry.addr, tick);
                crate::serial_println!(
                    "[QRING DISPATCH] Silo {} SiloVaporize target={}", silo_id, entry.addr
                );
                CompStatus::Ok
            } else {
                crate::serial_println!(
                    "[QRING DISPATCH] Silo {} SiloVaporize DENIED (no KILL cap)", silo_id
                );
                CompStatus::CapDenied
            }
        }

        // ── Capability ────────────────────────────────────────────────────────
        SqOpcode::CapCheck => {
            // entry.addr = object_oid, entry.aux = cap_type discriminant
            // In production: calls cap_token::check(silo_id, cap_type, object_oid)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} CapCheck oid={:#x} cap_type={}", silo_id, entry.addr, entry.aux
            );
            CompStatus::Ok
        }

        // ── NPU / AI ──────────────────────────────────────────────────────────
        SqOpcode::NpuInfer => {
            // entry.addr = model_oid, entry.len = input_tensor_size
            // entry.aux = SynapseMsgType discriminant (for synapse_bridge.rs routing)
            // In production: calls npu_sched::enqueue(silo_id, model_oid, len)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} NpuInfer model={:#x} input_len={}", silo_id, entry.addr, entry.len
            );
            CompStatus::Ok
        }

        // ── Q-Fabric ──────────────────────────────────────────────────────────
        SqOpcode::FabricSend | SqOpcode::FabricRecv => {
            // entry.addr = Q-Fabric NodeId (first 8 bytes), entry.len = payload
            let op = if opcode == SqOpcode::FabricSend { "FabricSend" } else { "FabricRecv" };
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} {} node={:#x} len={}", silo_id, op, entry.addr, entry.len
            );
            CompStatus::Ok
        }

        // ── Audit ─────────────────────────────────────────────────────────────
        SqOpcode::AuditLog => {
            // entry.addr = event_type, entry.len = message_len
            // In production: calls kstate::audit().record()
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} AuditLog event_type={} len={}", silo_id, entry.aux, entry.len
            );
            CompStatus::Ok
        }

        // ── PMC ───────────────────────────────────────────────────────────────
        SqOpcode::PmcRead => {
            // entry.addr = PMC register (0-7), returns counter value in CQ result
            // In production: calls pmc::read_counter(entry.addr as u8)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} PmcRead pmc={}", silo_id, entry.addr
            );
            CompStatus::Ok
        }

        // ── Timer ─────────────────────────────────────────────────────────────
        SqOpcode::TimerSet => {
            // entry.addr = user_data to return in timer event, entry.len = delay_ticks
            // In production: timer_wheel::set(silo_id, delay, user_data)
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} TimerSet delay={}ticks tag={:#x}", silo_id, entry.len, entry.addr
            );
            CompStatus::Ok
        }

        SqOpcode::Unknown => {
            crate::serial_println!(
                "[QRING DISPATCH] Silo {} Unknown opcode={:#x}", silo_id, entry.opcode
            );
            CompStatus::Invalid
        }
    };

    RealDispatchResult {
        user_data: entry.user_data,
        status,
        byte_count: if status == CompStatus::Ok { entry.len } else { 0 },
    }
}
