# Qindows

**The Final Operating System.**

A planetary-scale, intent-centric OS built on a Rust microkernel. Vector-native rendering. Neural integration. Zero-trust security.

> *Windows has ended. Qindows has begun.*

---

## 🏗️ Architecture

Qindows is designed as five concentric layers:

| Layer | Name | Purpose |
|-------|------|---------|
| 1 | **Qernel** | Minimal Rust microkernel (scheduling, IPC, memory mapping) |
| 2 | **Prism** | Semantic Object Graph (replaces files + registry) |
| 3 | **Sentinel / Synapse** | AI law enforcement + neural BCI integration |
| 4 | **Q-Silos** | Hardware-enforced sandboxes for every process |
| 5 | **Aether / Q-Shell** | GPU vector compositor + semantic command palette |

## 📦 Workspace Crates

```
qindows/
├── bootloader/    # UEFI bootloader (GOP, memory map, Qernel handoff)
├── qernel/        # Microkernel (Ring 0)
│   ├── memory/    # Frame allocator, paging, heap
│   ├── interrupts/# IDT, exceptions, syscalls
│   ├── scheduler/ # Fiber-based per-core scheduling
│   ├── capability/# Zero-trust capability tokens
│   ├── silo/      # Process isolation (Q-Silos)
│   ├── sentinel/  # AI law enforcement
│   └── drivers/   # GPU framebuffer, serial port
├── prism/         # Semantic Object Storage
├── aether/        # Vector Compositor Engine
├── q-shell/       # Semantic Command Palette
├── chimera/       # Win32/64 Legacy Bridge
├── synapse/       # Neural Integration (BCI)
└── nexus/         # Global Mesh Networking
```

## 🔧 Building

```bash
# Install nightly Rust with x86_64 bare-metal target
rustup toolchain install nightly
rustup target add x86_64-unknown-none

# Build the workspace
cd qindows
cargo build
```

## 🌐 Website

The showcase website is served from the project root:

```bash
npx serve -l 3456 .
```

Visit [http://localhost:3456](http://localhost:3456) to explore the architecture.

## 📜 The Q-Manifest

Ten hardware-enforced laws that govern every process:

1. **Zero Ambient Authority** — Apps launch with zero permissions
2. **Immutable Binaries** — No self-modifying code
3. **Asynchronous Everything** — All I/O through Q-Ring queues
4. **Vector Native UI** — No bitmaps, only SDF math
5. **Global Deduplication** — One copy, many views
6. **Silo Sandbox** — Every app in hardware isolation
7. **Telemetry Transparency** — No silent network calls
8. **Energy Proportionality** — Background processes deep-slept
9. **Universal Namespace** — Data location is transparent
10. **Graceful Degradation** — Offline-first design

---

**Version:** 1.0.0 Genesis Alpha  
**Date:** March 4, 2026  
**License:** MIT
