#!/bin/bash
# Launch QEMU aarch64 with SDL window display, serial to log file, SSH on port 2222.
# Usage: bash run_vm.sh [headless]
#   (no args)  — SDL window + serial to file
#   headless   — no display (for CI / SSH-only access)
#
# This script:
#   1. Builds the ARM64 Image from the ELF kernel (if needed)
#   2. Launches QEMU with the correct QEMU args for the Windows QEMU binary
#
# Key insight: QEMU must load the kernel as an ARM64 Image (not ELF) so that
# x0 is set to the FDT pointer. MSYS_NO_PATHCONV=1 prevents Git Bash from
# mangling the -append kernel cmdline (e.g. /init -> C:\Program Files\Git\init).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

LOG="$SCRIPT_DIR/target/qemu_serial.log"
PIDFILE="$SCRIPT_DIR/target/qemu.pid"
ELF="$SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf"
IMAGE="$SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.image"
INITRAMFS="$SCRIPT_DIR/test/initramfs/build/initramfs_aarch64.cpio.gz"
MAKE_IMAGE_SCRIPT="$SCRIPT_DIR/tools/make_arm64_image.py"

# Kill any existing instance
if [ -f "$PIDFILE" ]; then
    kill $(cat "$PIDFILE") 2>/dev/null
fi
pkill -9 -f qemu-system-aarch64 2>/dev/null
taskkill //F //IM qemu-system-aarch64.exe 2>/dev/null
sleep 1

# --- Step 1: Build ARM64 Image from ELF ---
if [ ! -f "$ELF" ]; then
    echo "ERROR: Kernel ELF not found at $ELF"
    echo "Run: cargo osdk build --scheme aarch64 --target-arch aarch64"
    exit 1
fi

# Rebuild image if ELF is newer
if [ ! -f "$IMAGE" ] || [ "$ELF" -nt "$IMAGE" ]; then
    echo "[run_vm] Building ARM64 Image from ELF..."
    if command -v wsl &>/dev/null; then
        WSL_SCRIPT_DIR=$(wsl -d Ubuntu-24.04 -- bash -c 'wslpath "$0"' "$SCRIPT_DIR" 2>/dev/null | tr -d '\r')
        if [ -z "$WSL_SCRIPT_DIR" ]; then
            WSL_SCRIPT_DIR=$(wsl -d Ubuntu-24.04 -- bash -c "readlink -f '$SCRIPT_DIR'" 2>/dev/null | tr -d '\r')
        fi
        wsl -d Ubuntu-24.04 -- bash -c \
            "python3 '$WSL_SCRIPT_DIR/tools/make_arm64_image.py' '$WSL_SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf' '$WSL_SCRIPT_DIR/target/osdk/aster-kernel/aster-kernel-osdk-bin.image'" \
            2>/dev/null | tail -1
    else
        python3 "$MAKE_IMAGE_SCRIPT" "$ELF" "$IMAGE" 2>/dev/null | tail -1
    fi
    if [ ! -f "$IMAGE" ]; then
        echo "ERROR: Failed to build ARM64 Image"
        exit 1
    fi
fi

# --- Step 2: Launch QEMU ---
DISPLAY_OPT="-display sdl"
if [ "$1" = "headless" ]; then
    DISPLAY_OPT="-display none"
fi

# Detect the QEMU binary.
QEMU_BIN="qemu-system-aarch64"
if ! command -v "$QEMU_BIN" &>/dev/null; then
    QEMU_BIN="/c/Program Files/qemu/qemu-system-aarch64.exe"
fi

# Convert paths to Windows format for the Windows QEMU binary.
case "$QEMU_BIN" in
    *.exe|/c/*|/C/*)
        WINIMAGE=$(cygpath -w "$IMAGE" 2>/dev/null || echo "$IMAGE")
        WININITRD=$(cygpath -w "$INITRAMFS" 2>/dev/null || echo "$INITRAMFS")
        WINLOG=$(cygpath -w "$LOG" 2>/dev/null || echo "$LOG")
        ;;
    *)
        WINIMAGE="$IMAGE"
        WININITRD="$INITRAMFS"
        WINLOG="$LOG"
        ;;
esac

# Kernel command line. On Git Bash, MSYS_NO_PATHCONV=1 is CRITICAL: without it,
# MSYS2 converts /init to C:\Program Files\Git\init, breaking boot.
KCMDLINE="init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin"

# Launch QEMU. The key to a stable window on Git Bash/Windows is running QEMU
# in the background (&) followed by `disown`, which removes it from the shell's
# job table so it survives when the launching shell exits. MSYS_NO_PATHCONV=1
# prevents MSYS2 from mangling the -append cmdline (/init -> C:\...\init).
MSYS_NO_PATHCONV=1 "$QEMU_BIN" \
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
    -append "$KCMDLINE" >/dev/null 2>&1 &
QEMU_PID=$!
disown $QEMU_PID 2>/dev/null
echo $QEMU_PID > "$PIDFILE"

echo "QEMU started"
echo "Kernel: ARM64 Image ($IMAGE)"
echo "Display: $DISPLAY_OPT"
echo "Serial log: $LOG"
echo "SSH: ssh -p 2222 root@127.0.0.1"
