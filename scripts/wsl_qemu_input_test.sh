#!/usr/bin/env bash
# WSL2 QEMU aarch64 — click simulation + before/after screendump analysis.
#
# Boots kei, captures a baseline screendump, simulates a mouse click via the
# QEMU monitor (mouse_move + mouse_button), captures a second screendump, and
# compares pixel stats to verify the input path works end-to-end.
#
# Usage: bash scripts/wsl_qemu_input_test.sh [boot_secs]
set -u

KEI="${KEI:-$HOME/celestia/kei}"
BOOT_SECS="${1:-105}"
MON_PORT="${MON_PORT:-55555}"
INITRAMFS="${INITRAMFS:-$KEI/tests/initramfs/build/initramfs_render_new.cpio.gz}"
KERNEL="${KERNEL:-$KEI/target/osdk/aster-kernel/aster-kernel-osdk-bin.image}"
SERIAL_LOG="$KEI/target/wsl_input_serial.log"
BEFORE_PPM="$KEI/target/wsl_input_before.ppm"
AFTER_PPM="$KEI/target/wsl_input_after.ppm"

cd "$KEI" || exit 1
mkdir -p target

echo "[input-test] boot ${BOOT_SECS}s, monitor=$MON_PORT"

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot \
  -display none \
  -device virtio-gpu-device \
  -device virtio-keyboard-device \
  -device virtio-mouse-device \
  -serial "file:$SERIAL_LOG" \
  -monitor "tcp:127.0.0.1:$MON_PORT,server,nowait" \
  -netdev "user,id=net0,hostfwd=tcp::2222-:22" \
  -device virtio-net-device,netdev=net0 \
  -kernel "$KERNEL" \
  -initrd "$INITRAMFS" \
  -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin" \
  >/tmp/kei_input_stdout.log 2>&1 &
QEMU_PID=$!

# Wait for monitor
for i in $(seq 1 50); do
  (exec 3<>/dev/tcp/127.0.0.1/$MON_PORT) 2>/dev/null && { exec 3>&- 3<&-; break; }
  sleep 0.2
done

cleanup() {
  { echo "quit"; sleep 0.3; } | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1
  kill -9 "$QEMU_PID" 2>/dev/null
  wait "$QEMU_PID" 2>/dev/null
}
trap cleanup EXIT

# Monitor command helper
mon() { { echo "$1"; sleep 0.3; } | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1; }

echo "[input-test] booting ${BOOT_SECS}s..."
sleep "$BOOT_SECS"

# Baseline screendump
echo "[input-test] capturing BEFORE screendump"
mon "screendump $BEFORE_PPM"
sleep 1

# Simulate mouse click at center of screen (320, 240 for 640x480)
echo "[input-test] simulating mouse move to (320,240) + click"
mon "mouse_move 320 240"
sleep 0.3
mon "mouse_button 1"
sleep 0.3
mon "mouse_button 0"
sleep 0.5

# Also send a keyboard key (space) via the monitor
echo "[input-test] sending keyboard 'space' event"
mon "sendkey spc"
sleep 0.5

# After screendump
echo "[input-test] capturing AFTER screendump"
mon "screendump $AFTER_PPM"
sleep 1

# Compare
echo "[input-test] === pixel stats ==="
for ppm in "$BEFORE_PPM" "$AFTER_PPM"; do
  python3 - "$ppm" <<'PY'
import sys
p = sys.argv[1]
try:
    with open(p,'rb') as f: data = f.read()
    idx=0; fields=[]
    while len(fields)<4:
        while data[idx] in b' \t\n\r': idx+=1
        if data[idx]==0x23:
            while data[idx]!=0x0A: idx+=1
            continue
        j=idx
        while data[j] not in b' \t\n\r': j+=1
        fields.append(data[idx:j]); idx=j
    idx+=1
    w=int(fields[1]); h=int(fields[2]); px=data[idx:]
    n=len(px)//3
    nb=sum(1 for i in range(0,min(len(px),n*3),3) if px[i] or px[i+1] or px[i+2])
    import os
    print(f"  {os.path.basename(p)}: {w}x{h} nonblack={nb}/{n} ({100*nb/max(n,1):.1f}%)")
except Exception as e:
    print(f"  {p}: ERROR {e}")
PY
done

# Check input device registration
echo "[input-test] === input device registration ==="
grep -a "input device\|virtio.*keyboard\|virtio.*mouse\|registered device" "$SERIAL_LOG" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | head -5

echo "[input-test] done"
