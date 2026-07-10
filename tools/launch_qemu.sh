#!/bin/bash
# Launch QEMU aarch64 with SDL window display, serial to log file, SSH on port 2222.
# Usage: bash tools/launch_qemu.sh [headless]
#
# This script is called by `just run`. It:
#   1. Builds the ARM64 Image from the ELF kernel (if needed)
#   2. Launches QEMU detached so the caller returns immediately
#
# The key trick for Git Bash on Windows: use nohup + & + disown to detach
# QEMU from the calling process. Combined with redirecting all stdio to
# /dev/null, QEMU survives even when the parent shell (or `just`) exits.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOG="$SCRIPT_DIR/target/qemu_serial.log"
PIDFILE="$SCRIPT_DIR/target/qemu.pid"
ELF="$SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf"
IMAGE="$SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.image"
INITRAMFS="$SCRIPT_DIR/test/initramfs/build/initramfs_aarch64.cpio.gz"
MAKE_IMAGE_SCRIPT="$SCRIPT_DIR/tools/make_arm64_image.py"

# Kill any existing instance
taskkill //F //IM qemu-system-aarch64.exe 2>/dev/null || true
pkill -9 -f qemu-system-aarch64 2>/dev/null || true
sleep 1

DISPLAY_OPT="-display sdl"
if [ "$1" = "headless" ]; then
    DISPLAY_OPT="-display none"
fi

# --- Step 1: Build ARM64 Image from ELF ---
if [ ! -f "$ELF" ]; then
    echo "ERROR: Kernel ELF not found at $ELF"
    echo "Run: just build-arch aarch64"
    exit 1
fi

if [ ! -f "$IMAGE" ] || [ "$ELF" -nt "$IMAGE" ]; then
    echo "[launch] Building ARM64 Image from ELF..."
    WSD=$(wsl -d Ubuntu-24.04 -- bash -c "readlink -f '$SCRIPT_DIR'" 2>/dev/null | tr -d '\r')
    if [ -n "$WSD" ]; then
        wsl -d Ubuntu-24.04 -- bash -c \
            "python3 '$WSD/tools/make_arm64_image.py' '$WSD/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf' '$WSD/target/osdk/aster-kernel/aster-kernel-osdk-bin.image'" \
            2>/dev/null | tail -1
    fi
    if [ ! -f "$IMAGE" ]; then
        echo "ERROR: Failed to build ARM64 Image"
        exit 1
    fi
fi

# --- Step 2: Convert paths for Windows QEMU ---
WINIMAGE=$(cygpath -w "$IMAGE" 2>/dev/null || echo "$IMAGE")
WININITRD=$(cygpath -w "$INITRAMFS" 2>/dev/null || echo "$INITRAMFS")
WINLOG=$(cygpath -w "$LOG" 2>/dev/null || echo "$LOG")

# --- Step 3: Launch QEMU ---
KCMDLINE="init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin"

# nohup + & + disown detaches QEMU so it survives the parent shell exit.
# MSYS_NO_PATHCONV=1 prevents Git Bash from mangling /init in -append.
MSYS_NO_PATHCONV=1 nohup "/c/Program Files/qemu/qemu-system-aarch64.exe" \
    -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
    -m 2G -smp 1 --no-reboot \
    $DISPLAY_OPT \
    -device virtio-gpu-device \
    -device virtio-keyboard-device \
    -serial file:"$WINLOG" \
    -netdev user,id=net0,hostfwd=tcp::2222-:22 \
    -device virtio-net-device,netdev=net0 \
    -kernel "$WINIMAGE" \
    -initrd "$WININITRD" \
    -append "$KCMDLINE" \
    >/dev/null 2>&1 &

echo $! > "$PIDFILE"
disown 2>/dev/null || true
