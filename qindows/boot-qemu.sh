#!/bin/bash
# ═══════════════════════════════════════════════════════════
# Qindows — QEMU Boot Script
# Boots the real UEFI bootloader in QEMU with OVMF firmware
# ═══════════════════════════════════════════════════════════

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
OVMF_CODE="/usr/local/share/qemu/edk2-x86_64-code.fd"
OVMF_VARS_TEMPLATE="/usr/local/share/qemu/edk2-i386-vars.fd"

# Build the bootloader
echo "Building Qindows UEFI Bootloader..."
cargo build --release --target x86_64-unknown-uefi \
  -p qindows-bootloader \
  -Zbuild-std=core,compiler_builtins,alloc \
  -Zbuild-std-features=compiler-builtins-mem

# Create EFI disk image
mkdir -p "$BUILD_DIR/esp/EFI/BOOT"
cp target/x86_64-unknown-uefi/release/qindows-bootloader.efi \
   "$BUILD_DIR/esp/EFI/BOOT/BOOTX64.EFI"

dd if=/dev/zero of="$BUILD_DIR/qindows-efi.img" bs=1M count=64 2>/dev/null
/usr/local/Cellar/dosfstools/4.2/sbin/mkfs.fat -F 32 "$BUILD_DIR/qindows-efi.img" >/dev/null
mmd -i "$BUILD_DIR/qindows-efi.img" ::/EFI
mmd -i "$BUILD_DIR/qindows-efi.img" ::/EFI/BOOT
mcopy -i "$BUILD_DIR/qindows-efi.img" \
  "$BUILD_DIR/esp/EFI/BOOT/BOOTX64.EFI" ::/EFI/BOOT/BOOTX64.EFI

# Create writable NVRAM copy
cp "$OVMF_VARS_TEMPLATE" "$BUILD_DIR/ovmf-vars.fd"

echo ""
echo "╔══════════════════════════════════════╗"
echo "║   QINDOWS QEMU LAUNCHER             ║"
echo "║   Press Ctrl+A then X to exit QEMU   ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Launch QEMU with GUI display
qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$BUILD_DIR/ovmf-vars.fd" \
  -drive format=raw,file="$BUILD_DIR/qindows-efi.img" \
  -m 512M \
  -no-reboot \
  "$@"
