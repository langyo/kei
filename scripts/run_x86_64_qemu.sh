#!/bin/bash
# Launch the kei x86_64 kernel under the Windows QEMU binary from the
# bootable GRUB ISO built by scripts/build_x86_64_iso.sh.
#
# Usage:
#   bash scripts/run_x86_64_qemu.sh [SECS]     # default 120 seconds
#
# Serial output is written to target/qemu_x86_64_serial.log and printed at
# the end. QEMU always runs under `timeout` so it can never hang the caller.
#
# NOTE: Windows QEMU cannot open files under non-ASCII (e.g. CJK) paths —
# both -kernel/-initrd and the file: chardev fail with "open ... failed".
# The ISO and serial log are therefore staged in an ASCII-only temporary
# directory, and the serial log is copied back into target/ after the run.
#
# Boot path note: the bzImage/microvm path is a dead end — the kei bzImage
# stub only implements the EFI handover entry (its real-mode setup area is
# zeroed), QEMU -kernel refuses ELF64 multiboot images, and this Windows
# QEMU has no OVMF. GRUB multiboot2 from the ISO is the working path.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ISO="$ROOT/target/x86_64-iso/kei.iso"
LOG="$ROOT/target/qemu_x86_64_serial.log"
QEMU_BIN="qemu-system-x86_64"
if ! command -v "$QEMU_BIN" &>/dev/null; then
    QEMU_BIN="/c/Program Files/qemu/qemu-system-x86_64.exe"
fi
SECS="${1:-120}"

if [ ! -f "$ISO" ]; then
    echo "ERROR: missing $ISO"
    echo "Build first (in WSL): bash scripts/build_x86_64_iso.sh"
    exit 1
fi
mkdir -p "$ROOT/target"

# Stage into an ASCII-only directory (see the NOTE above). TMPDIR overrides
# the Windows %TMP%/%TEMP% when set.
STAGE_POSIX="$(cygpath "${TMPDIR:-${TMP:-${TEMP:-/tmp}}}")/kei_x64_qemu"
if LC_ALL=C grep -q '[^ -~]' <<<"$STAGE_POSIX"; then
    echo "ERROR: staging path is not ASCII-only: $STAGE_POSIX"
    echo "Set TMPDIR to an ASCII-only directory and retry."
    exit 1
fi
mkdir -p "$STAGE_POSIX"
cp -f "$ISO" "$STAGE_POSIX/kei.iso"
: > "$STAGE_POSIX/serial.log"

WINISO=$(cygpath -w "$STAGE_POSIX/kei.iso")
WINLOG=$(cygpath -w "$STAGE_POSIX/serial.log")

echo "[run-x86_64] iso: $ISO"
echo "[run-x86_64] staging: $STAGE_POSIX"
echo "[run-x86_64] serial log: $LOG (running ${SECS}s)"

# MSYS_NO_PATHCONV=1 keeps Git Bash from mangling QEMU arguments.
# Default pc machine + SeaBIOS boots the CD-ROM via GRUB. virtio devices
# must use the -pci variants on this machine (virtio-*-device needs the
# virtio-mmio bus, which only exists on microvm/virt).
MSYS_NO_PATHCONV=1 timeout --signal=KILL "$SECS" "$QEMU_BIN" \
    -cpu Icelake-Server,+x2apic \
    -m "${MEM:-2G}" \
    -smp "${SMP:-1}" \
    --no-reboot \
    -display none \
    -cdrom "$WINISO" \
    -boot d \
    -serial file:"$WINLOG" \
    -monitor none \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -netdev user,id=net0,hostfwd=tcp::2223-:22 \
    -device virtio-net-pci,netdev=net0
RC=$?

cp -f "$STAGE_POSIX/serial.log" "$LOG" 2>/dev/null || true

echo "[run-x86_64] qemu exited with code $RC (124/137 = timeout reached)"
echo "===== serial log ====="
cat "$LOG" 2>/dev/null || echo "(no serial output captured)"
