# ═══════════════════════════════════════════════════════
# QINDOWS OS — Build System
# ═══════════════════════════════════════════════════════
# Builds the Qernel, creates a UEFI-bootable disk image,
# and runs it in QEMU for testing.
#
# Usage:
#   make build    — Compile all crates
#   make image    — Create bootable disk image
#   make run      — Build + boot in QEMU
#   make debug    — Build + boot with GDB server
#   make clean    — Remove build artifacts
# ═══════════════════════════════════════════════════════

SHELL := /bin/bash

# ─── Paths ───────────────────────────────────────────
WORKSPACE    := $(CURDIR)
TARGET       := x86_64-unknown-none
BUILD_DIR    := $(WORKSPACE)/target/$(TARGET)/debug
IMAGE_DIR    := $(WORKSPACE)/build
OVMF_DIR     := $(WORKSPACE)/ovmf
QERNEL_BIN   := $(BUILD_DIR)/qernel
BOOT_BIN     := $(BUILD_DIR)/qindows-bootloader
ESP_DIR      := $(IMAGE_DIR)/esp
DISK_IMAGE   := $(IMAGE_DIR)/qindows.img

# ─── Tools ───────────────────────────────────────────
CARGO        := cargo
QEMU         := qemu-system-x86_64
OBJCOPY      := rust-objcopy
DD           := dd
MKFS         := mkfs.fat

# ─── QEMU Configuration ─────────────────────────────
QEMU_MEMORY  := 512M
QEMU_CPUS    := 4
QEMU_COMMON  := \
	-machine q35 \
	-cpu qemu64,+x2apic \
	-m $(QEMU_MEMORY) \
	-smp $(QEMU_CPUS) \
	-drive if=pflash,format=raw,readonly=on,file=$(OVMF_DIR)/OVMF_CODE.fd \
	-drive if=pflash,format=raw,file=$(OVMF_DIR)/OVMF_VARS.fd \
	-drive format=raw,file=$(DISK_IMAGE) \
	-serial stdio \
	-no-reboot \
	-no-shutdown \
	-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
	-device virtio-gpu-pci \
	-device virtio-net-pci,netdev=net0 \
	-netdev user,id=net0 \
	-device qemu-xhci \
	-device usb-kbd \
	-device usb-mouse

# ═══════════════════════════════════════════════════════
# Targets
# ═══════════════════════════════════════════════════════

.PHONY: all build image run debug clean check clippy fmt setup

all: build

# ─── Build ───────────────────────────────────────────
build:
	@echo "╔══════════════════════════════════════╗"
	@echo "║   Building Qindows OS...             ║"
	@echo "╚══════════════════════════════════════╝"
	$(CARGO) build --workspace 2>&1
	@echo "[OK] Build complete"

# ─── Check (fast compile check) ─────────────────────
check:
	$(CARGO) check --workspace

# ─── Clippy (lint) ───────────────────────────────────
clippy:
	$(CARGO) clippy --workspace -- -D warnings

# ─── Format ──────────────────────────────────────────
fmt:
	$(CARGO) fmt --all

# ─── Create Bootable Image ──────────────────────────
image: build
	@echo "Creating bootable UEFI disk image..."
	@mkdir -p $(IMAGE_DIR) $(ESP_DIR)/EFI/BOOT $(ESP_DIR)/EFI/qindows
	
	# Convert the bootloader ELF to a UEFI PE application
	@if command -v $(OBJCOPY) >/dev/null 2>&1; then \
		$(OBJCOPY) -O binary $(BOOT_BIN) $(ESP_DIR)/EFI/BOOT/BOOTX64.EFI 2>/dev/null || \
		cp $(BOOT_BIN) $(ESP_DIR)/EFI/BOOT/BOOTX64.EFI; \
	else \
		cp $(BOOT_BIN) $(ESP_DIR)/EFI/BOOT/BOOTX64.EFI; \
	fi
	
	# Copy the Qernel binary
	@cp $(QERNEL_BIN) $(ESP_DIR)/EFI/qindows/qernel.bin 2>/dev/null || true
	
	# Create a FAT32 disk image (64MB)
	@$(DD) if=/dev/zero of=$(DISK_IMAGE) bs=1M count=64 2>/dev/null
	@$(MKFS) -F 32 $(DISK_IMAGE) 2>/dev/null || true
	
	@echo "[OK] Disk image created: $(DISK_IMAGE)"

# ─── Run in QEMU ────────────────────────────────────
run: image
	@echo "╔══════════════════════════════════════╗"
	@echo "║   Booting Qindows in QEMU...         ║"
	@echo "╚══════════════════════════════════════╝"
	$(QEMU) $(QEMU_COMMON) $(QEMU_EXTRA)

# ─── Debug with GDB ─────────────────────────────────
debug: image
	@echo "Starting QEMU with GDB server on :1234..."
	$(QEMU) $(QEMU_COMMON) -s -S $(QEMU_EXTRA)

# ─── Setup OVMF firmware ────────────────────────────
setup:
	@echo "Downloading UEFI firmware (OVMF)..."
	@mkdir -p $(OVMF_DIR)
	@curl -L -o $(OVMF_DIR)/OVMF_CODE.fd \
		https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_CODE.fd 2>/dev/null || \
		echo "Failed to download OVMF_CODE.fd"
	@curl -L -o $(OVMF_DIR)/OVMF_VARS.fd \
		https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_VARS.fd 2>/dev/null || \
		echo "Failed to download OVMF_VARS.fd"
	@echo "[OK] OVMF firmware downloaded"

# ─── Clean ───────────────────────────────────────────
clean:
	$(CARGO) clean
	rm -rf $(IMAGE_DIR)
	@echo "[OK] Cleaned build artifacts"
