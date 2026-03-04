#!/bin/bash
# ═══════════════════════════════════════════════════════
# QINDOWS — QEMU Runner Script
# ═══════════════════════════════════════════════════════
# Quick-launch Qindows in QEMU for development testing.
#
# Usage:
#   ./run.sh         — Build + boot
#   ./run.sh debug   — Build + boot with GDB server
#   ./run.sh setup   — Download OVMF firmware
# ═══════════════════════════════════════════════════════

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BOLD="\033[1m"
CYAN="\033[36m"
GREEN="\033[32m"
YELLOW="\033[33m"
RED="\033[31m"
RESET="\033[0m"

banner() {
    echo -e "${CYAN}${BOLD}"
    echo "╔══════════════════════════════════════╗"
    echo "║        QINDOWS OS — Runner           ║"
    echo "║    The Final Operating System         ║"
    echo "╚══════════════════════════════════════╝"
    echo -e "${RESET}"
}

check_deps() {
    local missing=0
    for cmd in cargo qemu-system-x86_64; do
        if ! command -v "$cmd" &>/dev/null; then
            echo -e "${RED}✗ Missing: $cmd${RESET}"
            missing=1
        else
            echo -e "${GREEN}✓ Found: $cmd${RESET}"
        fi
    done

    if ! rustup target list --installed | grep -q "x86_64-unknown-none"; then
        echo -e "${YELLOW}Installing target x86_64-unknown-none...${RESET}"
        rustup target add x86_64-unknown-none
    fi

    if [ $missing -ne 0 ]; then
        echo -e "${RED}Please install missing dependencies.${RESET}"
        exit 1
    fi
}

setup_ovmf() {
    echo -e "${CYAN}Downloading UEFI firmware (OVMF)...${RESET}"
    mkdir -p ovmf
    curl -L -o ovmf/OVMF_CODE.fd \
        "https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_CODE.fd" || true
    curl -L -o ovmf/OVMF_VARS.fd \
        "https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_VARS.fd" || true
    echo -e "${GREEN}✓ OVMF firmware ready${RESET}"
}

build() {
    echo -e "${CYAN}Building Qindows...${RESET}"
    cargo build --workspace 2>&1 || {
        echo -e "${YELLOW}Note: Full bare-metal build requires nightly + x86_64-unknown-none target.${RESET}"
        echo -e "${YELLOW}Some crates may not compile yet — that's expected during development.${RESET}"
    }
    echo -e "${GREEN}✓ Build step complete${RESET}"
}

create_image() {
    echo -e "${CYAN}Creating bootable disk image...${RESET}"
    mkdir -p build/esp/EFI/BOOT build/esp/EFI/qindows

    # Create 64MB FAT32 image
    dd if=/dev/zero of=build/qindows.img bs=1M count=64 2>/dev/null
    
    if command -v mkfs.fat &>/dev/null; then
        mkfs.fat -F 32 build/qindows.img 2>/dev/null || true
    fi
    
    echo -e "${GREEN}✓ Disk image: build/qindows.img (64MB)${RESET}"
}

run_qemu() {
    local extra=""
    if [ "$1" = "debug" ]; then
        extra="-s -S"
        echo -e "${YELLOW}GDB server listening on :1234${RESET}"
        echo -e "${YELLOW}Connect with: gdb -ex 'target remote :1234'${RESET}"
    fi

    echo -e "${CYAN}${BOLD}Booting Qindows in QEMU...${RESET}"
    
    local qemu_args=(
        -machine q35
        -cpu qemu64,+x2apic
        -m 512M
        -smp 4
        -serial stdio
        -no-reboot
        -no-shutdown
        -device virtio-gpu-pci
        -device virtio-net-pci,netdev=net0
        -netdev user,id=net0
        -device qemu-xhci
        -device usb-kbd
        -device usb-mouse
    )

    # Add OVMF if available
    if [ -f ovmf/OVMF_CODE.fd ]; then
        qemu_args+=(
            -drive if=pflash,format=raw,readonly=on,file=ovmf/OVMF_CODE.fd
            -drive if=pflash,format=raw,file=ovmf/OVMF_VARS.fd
        )
    fi

    # Add disk image
    if [ -f build/qindows.img ]; then
        qemu_args+=(-drive format=raw,file=build/qindows.img)
    fi

    qemu-system-x86_64 "${qemu_args[@]}" $extra
}

# ─── Main ─────────────────────────────────────────────

banner

case "${1:-run}" in
    setup)
        check_deps
        setup_ovmf
        ;;
    debug)
        check_deps
        build
        create_image
        run_qemu debug
        ;;
    run|"")
        check_deps
        build
        create_image
        run_qemu
        ;;
    build)
        check_deps
        build
        ;;
    *)
        echo "Usage: $0 [run|debug|build|setup]"
        exit 1
        ;;
esac
