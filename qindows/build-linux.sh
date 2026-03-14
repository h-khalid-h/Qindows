#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════
#  QINDOWS — Linux Build & Boot Script
#  Builds kernel + bootloader, creates EFI image, runs QEMU
#  with NVMe, USB xHCI, and HDA Audio virtual devices.
# ═══════════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# ── Configuration ───────────────────────────────────────────────
OVMF_CODE="/usr/share/OVMF/OVMF_CODE_4M.fd"
OVMF_VARS="/usr/share/OVMF/OVMF_VARS_4M.fd"
BUILD_DIR="build"
KERNEL_ELF="target/x86_64-unknown-none/release/qernel"
BOOTLOADER_EFI="target/x86_64-unknown-uefi/release/qindows-bootloader.efi"
ESP_IMG="$BUILD_DIR/qindows-efi.img"
SERIAL_LOG="$BUILD_DIR/qemu_serial.log"
QEMU_RAM="256M"

# NOTE: .cargo/config.toml sets relocation-model=static automatically
# to produce a proper EXEC ELF (not PIE/DYN).

mkdir -p "$BUILD_DIR" bootloader/blob

echo ""
echo "╔══════════════════════════════════════╗"
echo "║  QINDOWS BUILD SYSTEM (Linux)       ║"
echo "╚══════════════════════════════════════╝"
echo ""

# ── Step 1: Build Qernel ────────────────────────────────────
echo "[1/4] Building Qernel for x86_64 bare-metal..."
cargo build -p qernel \
  --target x86_64-unknown-none \
  --release \
  -Z build-std=core,alloc 2>&1 | tail -3

KERNEL_SIZE=$(wc -c < "$KERNEL_ELF" | tr -d ' ')
echo "       Kernel: ${KERNEL_SIZE} bytes (ELF)"

# Verify it's a static EXEC (not PIE/DYN)
ELF_TYPE=$(file "$KERNEL_ELF" | grep -o 'executable\|shared object')
if [ "$ELF_TYPE" != "executable" ]; then
    echo "ERROR: Kernel is '$ELF_TYPE', expected 'executable'."
    echo "       RUSTFLAGS must include -C relocation-model=static"
    exit 1
fi

# ── Step 2: Build Bootloader (embeds kernel ELF) ────────────
echo "[2/4] Building UEFI Bootloader (embedding kernel)..."
cp "$KERNEL_ELF" bootloader/blob/qernel.elf
touch bootloader/src/main.rs  # Force re-embed
cargo build -p qindows-bootloader \
  --target x86_64-unknown-uefi \
  --release \
  -Z build-std=core,alloc 2>&1 | tail -3

BOOT_SIZE=$(wc -c < "$BOOTLOADER_EFI" | tr -d ' ')
echo "       Bootloader: ${BOOT_SIZE} bytes (includes ${KERNEL_SIZE}b kernel)"

# ── Step 3: Create EFI System Partition image ───────────────
echo "[3/4] Creating bootable EFI disk image..."
dd if=/dev/zero of="$ESP_IMG" bs=1M count=64 2>/dev/null
mkfs.fat -F 32 "$ESP_IMG" >/dev/null 2>&1
mmd -i "$ESP_IMG" ::/EFI
mmd -i "$ESP_IMG" ::/EFI/BOOT
mcopy -i "$ESP_IMG" "$BOOTLOADER_EFI" ::/EFI/BOOT/BOOTX64.EFI
echo "       ESP image: $ESP_IMG (64MB FAT32)"

# ── Step 4: Launch QEMU ────────────────────────────────────
echo "[4/4] Launching QEMU with OVMF firmware..."
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  QEMU x86_64 — Qindows Boot         ║"
echo "║  Serial output → $SERIAL_LOG        ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Kill any existing QEMU instances
killall -9 qemu-system-x86_64 2>/dev/null || true
sleep 1

# Create writable OVMF vars copy
cp "$OVMF_VARS" "$BUILD_DIR/ovmf-vars.fd"

rm -f "$SERIAL_LOG"

# QEMU flags:
#   -drive nvme       : Virtual NVMe SSD (exercises drivers::nvme)
#   -device xhci      : USB 3.0 controller (exercises drivers::usb_xhci)
#   -device ich9-hda  : Intel HDA audio (exercises drivers::audio_hda)
#   -device virtio-net: VirtIO NIC (exercises drivers::virtio_net)
timeout "${QEMU_TIMEOUT:-25}" qemu-system-x86_64 \
  -m "$QEMU_RAM" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$BUILD_DIR/ovmf-vars.fd" \
  -drive format=raw,file="$ESP_IMG" \
  -drive file=/dev/null,if=none,id=nvmedisk,format=raw \
  -device nvme,serial=QindowsNVMe,drive=nvmedisk \
  -device qemu-xhci,id=xhci \
  -device usb-kbd,bus=xhci.0 \
  -device ich9-intel-hda \
  -device hda-duplex \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0 \
  -chardev file,id=ser0,path="$SERIAL_LOG" \
  -serial chardev:ser0 \
  -display none \
  -no-reboot \
  -device isa-debug-exit \
  || true

echo ""
echo "╔══════════════════════════════════════╗"
echo "║  BOOT LOG                           ║"
echo "╚══════════════════════════════════════╝"
echo ""
strings "$SERIAL_LOG" 2>/dev/null || echo "(no serial output)"

# ── Summary ─────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════╗"
echo "║  BUILD COMPLETE                      ║"
echo "╚══════════════════════════════════════╝"
echo "  Kernel:     $KERNEL_ELF ($KERNEL_SIZE bytes)"
echo "  Bootloader: $BOOTLOADER_EFI ($BOOT_SIZE bytes)"
echo "  ESP Image:  $ESP_IMG"
echo "  Serial Log: $SERIAL_LOG"
