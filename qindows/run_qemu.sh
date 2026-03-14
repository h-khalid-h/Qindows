#!/bin/bash
set -e

WORKSPACE_ROOT="$(pwd)"

echo "== [1/5] Clean-building Q-Shell (Ring 3 Payload) at 256MB..."
RUSTFLAGS="-C link-arg=-Tq-shell/linker.ld -C target-feature=-redzone -C relocation-model=static" cargo build --manifest-path q-shell/Cargo.toml --release --target x86_64-unknown-none

echo "== [2/5] Clean-building Qernel (bare-metal x86_64-unknown-none)..."
cargo build -p qernel --release 2>&1

echo "== [2/5] Copying fresh Qernel ELF to bootloader blob..."
cp target/x86_64-unknown-none/release/qernel bootloader/blob/qernel.elf
touch bootloader/src/main.rs  # Force bootloader re-embed

echo "== [3/5] Building UEFI Bootloader (x86_64-unknown-uefi)..."
cargo build -p qindows-bootloader --target x86_64-unknown-uefi --release

echo "== [4/5] Assembling EFI System Partition (ESP)..."
mkdir -p esp/EFI/BOOT
cp target/x86_64-unknown-uefi/release/qindows-bootloader.efi esp/EFI/BOOT/BOOTX64.EFI

echo "== [5/5] Launching QEMU (headless, serial → qemu_output.log)..."
killall qemu-system-x86_64 2>/dev/null || true
rm -f qemu_output.log
qemu-system-x86_64 \
  -m 512M \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive format=raw,file=fat:rw:esp \
  -serial file:qemu_output.log \
  -display none \
  -no-reboot \
  -device isa-debug-exit &

QEMU_PID=$!
echo "QEMU PID: $QEMU_PID — waiting 25s for boot to complete..."
sleep 25
echo ""
echo "=== Boot Log (qemu_output.log) ==="
cat qemu_output.log | strings

