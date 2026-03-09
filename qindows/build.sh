#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════
#  QINDOWS — Full Bare-Metal Build & Boot Script
#  Builds the kernel + bootloader, creates EFI image, runs QEMU
# ═══════════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

OVMF="/usr/local/share/qemu/edk2-x86_64-code.fd"
BUILD_DIR="build"
KERNEL_BIN="target/x86_64-unknown-none/release/qernel.bin"
BOOTLOADER_EFI="target/x86_64-unknown-uefi/release/qindows-bootloader.efi"

mkdir -p "$BUILD_DIR"

echo ""
echo "╔══════════════════════════════════════╗"
echo "║  QINDOWS BUILD SYSTEM               ║"
echo "╚══════════════════════════════════════╝"
echo ""

# ── Step 1: Build Qernel ────────────────────────────────────
echo "[1/5] Building Qernel for x86_64 bare-metal..."
cargo +nightly build -p qernel \
  --target x86_64-unknown-none \
  --release \
  -Z build-std=core,alloc 2>&1 | tail -3

# ── Step 2: Convert ELF → flat binary ──────────────────────
echo "[2/5] Converting kernel ELF → flat binary..."
rust-objcopy -O binary \
  target/x86_64-unknown-none/release/qernel \
  "$KERNEL_BIN"

KERNEL_SIZE=$(wc -c < "$KERNEL_BIN" | tr -d ' ')
echo "       Kernel: ${KERNEL_SIZE} bytes"

# Copy to bootloader blob directory
mkdir -p bootloader/blob
cp "$KERNEL_BIN" bootloader/blob/qernel.bin

# ── Step 3: Build Bootloader (embeds kernel) ────────────────
echo "[3/5] Building UEFI Bootloader (embedding kernel)..."
cargo build -p qindows-bootloader \
  --target x86_64-unknown-uefi \
  --release 2>&1 | tail -3

BOOT_SIZE=$(wc -c < "$BOOTLOADER_EFI" | tr -d ' ')
echo "       Bootloader: ${BOOT_SIZE} bytes (includes ${KERNEL_SIZE}b kernel)"

# ── Step 4: Create EFI System Partition image ───────────────
echo "[4/5] Creating bootable EFI disk image..."
ESP_IMG="$BUILD_DIR/qindows-efi.img"

dd if=/dev/zero of="$ESP_IMG" bs=1M count=64 2>/dev/null
/usr/local/opt/dosfstools/sbin/mkfs.fat -F 32 "$ESP_IMG" >/dev/null 2>&1
mmd -i "$ESP_IMG" ::/EFI
mmd -i "$ESP_IMG" ::/EFI/BOOT
mcopy -i "$ESP_IMG" "$BOOTLOADER_EFI" ::/EFI/BOOT/BOOTX64.EFI

echo "       ESP image: $ESP_IMG (64MB FAT32)"

# ── Step 5: Launch QEMU ────────────────────────────────────
echo "[5/5] Launching QEMU with OVMF firmware..."
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  QEMU x86_64 — Qindows Boot         ║"
echo "║  Serial output below ↓              ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Create OVMF vars file
cp "$OVMF" "$BUILD_DIR/ovmf-vars-boot.fd" 2>/dev/null || true

exec qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF" \
  -drive format=raw,file="$ESP_IMG" \
  -m 256M \
  -serial stdio \
  -display none \
  -no-reboot \
  -no-shutdown
