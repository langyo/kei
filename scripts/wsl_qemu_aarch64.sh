#!/usr/bin/env bash
# Launch kei aarch64 in WSL2 QEMU (headless), capture serial log + screendump.
# Usage (from WSL): bash scripts/wsl_qemu_aarch64.sh [run_seconds]
#
# This script is designed to run *inside* WSL2 (Ubuntu-24.04), driven either
# directly or via `wsl -d Ubuntu-24.04 -e bash -lc "...". It uses the ASCII
# symlink ~/celestia/kei to avoid the well-known CJK-path blocker.
set -u

KEI="${KEI:-$HOME/celestia/kei}"
RUN_SECS="${1:-25}"
MON_PORT="${MON_PORT:-55555}"
SERIAL_LOG="${SERIAL_LOG:-$KEI/target/wsl_qemu_serial.log}"
SCREEN_PPM="${SCREEN_PPM:-$KEI/target/wsl_screendump.ppm}"
SCREEN_PNG="${SCREEN_PNG:-$KEI/target/wsl_screendump.png}"
INITRAMFS="${INITRAMFS:-$KEI/tests/initramfs/build/initramfs_kei_tty.cpio.gz}"
KERNEL="${KERNEL:-$KEI/target/osdk/aster-kernel/aster-kernel-osdk-bin.image}"

cd "$KEI" || { echo "[err] KEI dir not found: $KEI"; exit 1; }
mkdir -p target

echo "[wsl-qemu] KEI=$KEI"
echo "[wsl-qemu] KERNEL=$KERNEL"
echo "[wsl-qemu] INITRAMFS=$INITRAMFS"
echo "[wsl-qemu] run for ${RUN_SECS}s"

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

# Launch QEMU headless (no SDL in WSL). virtio-gpu still scans out, we read it
# back via the HMP monitor `screendump` command. We tee serial to both a log
# file and our stdout so we can see OOPS/panic messages even if QEMU is killed.
# Use a FIFO so QEMU's -serial pipe never blocks on a slow reader.
rm -f "$SERIAL_LOG"
qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot \
  -display none \
  -device virtio-gpu-device \
  -device virtio-keyboard-device \
  -serial "file:$SERIAL_LOG" \
  -monitor "tcp:127.0.0.1:$MON_PORT,server,nowait" \
  -netdev "user,id=net0,hostfwd=tcp::2222-:22" \
  -device virtio-net-device,netdev=net0 \
  -kernel "$KERNEL" \
  -initrd "$INITRAMFS" \
  -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin" \
  >/tmp/kei_qemu_stdout.log 2>&1 &
QEMU_PID=$!
echo "[wsl-qemu] QEMU pid=$QEMU_PID, monitor=tcp://127.0.0.1:$MON_PORT"

# Wait for monitor port to accept connections
for i in $(seq 1 50); do
  if (exec 3<>/dev/tcp/127.0.0.1/$MON_PORT) 2>/dev/null; then
    exec 3>&- 3<&-
    echo "[wsl-qemu] monitor up after ${i}x0.2s"
    break
  fi
  sleep 0.2
done

cleanup() {
  echo "[wsl-qemu] cleaning up"
  { echo "quit"; sleep 0.3; } | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1
  kill -9 "$QEMU_PID" 2>/dev/null
  wait "$QEMU_PID" 2>/dev/null
}
trap cleanup EXIT

# Let the kernel boot + aris-render write pixels
echo "[wsl-qemu] waiting ${RUN_SECS}s for boot + render..."
sleep "$RUN_SECS"

# Capture screen via HMP `screendump`
echo "[wsl-qemu] capturing screendump -> $SCREEN_PPM"
{
  echo "screendump $SCREEN_PPM"
  sleep 1
  echo "info registers"
  sleep 0.3
  echo "quit"
  sleep 0.3
} | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1

if [ -f "$SCREEN_PPM" ]; then
  SZ=$(stat -c %s "$SCREEN_PPM" 2>/dev/null)
  echo "[wsl-qemu] screendump size=${SZ} bytes"
  # Convert to PNG if ImageMagick/pnmtopng available
  if command -v convert >/dev/null 2>&1; then
    convert "$SCREEN_PPM" "$SCREEN_PNG" 2>/dev/null && echo "[wsl-qemu] PNG: $SCREEN_PNG"
  elif command -v pnmtopng >/dev/null 2>&1; then
    pnmtopng < "$SCREEN_PPM" > "$SCREEN_PNG" 2>/dev/null && echo "[wsl-qemu] PNG: $SCREEN_PNG"
  elif command -v ffmpeg >/dev/null 2>&1; then
    ffmpeg -y -i "$SCREEN_PPM" "$SCREEN_PNG" </dev/null >/dev/null 2>&1 && echo "[wsl-qemu] PNG: $SCREEN_PNG"
  fi
  # Quick pixel stats: first 2 lines are P6 header, then raw RGB
  python3 - "$SCREEN_PPM" <<'PY'
import sys
p = sys.argv[1]
with open(p,'rb') as f:
    data = f.read()
# parse P6
idx = 0
fields = []
while len(fields) < 4:
    # skip whitespace
    while data[idx] in b' \t\n\r': idx += 1
    if data[idx] == ord('#'):
        while data[idx] != ord('\n'): idx += 1
        continue
    j = idx
    while data[j] not in b' \t\n\r': j += 1
    fields.append(data[idx:j]); idx = j
idx += 1
w = int(fields[1]); h = int(fields[2])
px = data[idx:]
n = len(px)//3
nonblack = sum(1 for i in range(0, min(len(px), n*3), 3) if px[i] or px[i+1] or px[i+2])
print(f"[px] {w}x{h} pixels={n} nonblack={nonblack} ({100*nonblack/max(n,1):.1f}%)")
if n:
    print(f"[px] first pixel RGB=({px[0]},{px[1]},{px[2]})")
    mid = (len(px)//6)*3
    print(f"[px] mid   pixel RGB=({px[mid]},{px[mid+1]},{px[mid+2]})")
PY
else
  echo "[wsl-qemu] NO screendump produced (virtio-gpu may not have scanout)"
fi

# Tail serial log
echo "[wsl-qemu] --- serial log tail (last 25 lines) ---"
tail -n 25 "$SERIAL_LOG" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g'
echo "[wsl-qemu] done"
