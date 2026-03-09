#!/bin/bash
# ═══════════════════════════════════════════════════════════════
#  QINDOWS — Bare-Metal Boot Builder
#  Builds the Qernel + UEFI bootloader and boots on QEMU
# ═══════════════════════════════════════════════════════════════
set -e

QINDOWS_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$QINDOWS_ROOT"

echo "╔══════════════════════════════════════╗"
echo "║   QINDOWS BUILD SYSTEM               ║"
echo "╚══════════════════════════════════════╝"
echo ""

# ── Step 1: Build the Qernel ─────────────────────────────────
echo "→ Building Qernel (bare-metal x86_64)..."
  cargo +nightly build -p qernel \
    --target x86_64-qindows.json \
    --release \
    -Z json-target-spec -Z build-std=core,alloc
echo ""

# ── Step 2: Copy kernel ELF to bootloader blob ──────────────
echo "→ Copying Qernel ELF to bootloader..."
mkdir -p bootloader/blob
cp target/x86_64-qindows/release/qernel bootloader/blob/qernel.elf
echo "  $(ls -lh bootloader/blob/qernel.elf | awk '{print $5}') kernel ELF"
echo ""

# ── Step 3: Build the UEFI bootloader (embeds kernel) ───────
echo "→ Building UEFI bootloader..."
cargo build -p qindows-bootloader \
  --target x86_64-unknown-uefi \
  --release
echo "  $(ls -lh target/x86_64-unknown-uefi/release/qindows-bootloader.efi | awk '{print $5}') bootloader EFI"
echo ""

# ── Step 4: Create bootable EFI disk image ──────────────────
echo "→ Creating EFI disk image..."
mkdir -p build
rm -f build/qindows-efi.img
dd if=/dev/zero of=build/qindows-efi.img bs=1048576 count=16 status=none

# Find dosfstools
MKFS_FAT=""
if command -v mkfs.fat &>/dev/null; then
  MKFS_FAT="mkfs.fat"
elif [ -x /usr/local/opt/dosfstools/sbin/mkfs.fat ]; then
  MKFS_FAT="/usr/local/opt/dosfstools/sbin/mkfs.fat"
else
  echo "ERROR: mkfs.fat not found. Install dosfstools."
  exit 1
fi

$MKFS_FAT -F 12 build/qindows-efi.img >/dev/null 2>&1
mmd -i build/qindows-efi.img ::/EFI ::/EFI/BOOT
mcopy -i build/qindows-efi.img \
  target/x86_64-unknown-uefi/release/qindows-bootloader.efi \
  ::/EFI/BOOT/BOOTX64.EFI
echo "  16 MiB EFI disk image ready"
echo ""

# ── Step 5: Launch QEMU ─────────────────────────────────────
OVMF="/usr/local/share/qemu/edk2-x86_64-code.fd"
if [ ! -f "$OVMF" ]; then
  # Try common alternative paths
  for alt in /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
    [ -f "$alt" ] && OVMF="$alt" && break
  done
fi

echo "═══════════════════════════════════════"
echo "  BOOTING QINDOWS ON QEMU"
echo "  OVMF: $OVMF"
echo "  RAM: 512 MiB"
echo "  Serial: stdio"
echo "═══════════════════════════════════════"
echo ""

exec qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF" \
  -drive format=raw,file=build/qindows-efi.img \
  -m 512M \
  -serial stdio \
  -vga std \
  -display cocoa \
  -no-reboot \
  -no-shutdown
