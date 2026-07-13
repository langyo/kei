#!/usr/bin/env bash
# wsl_qemu_desktop.sh — launch the kei desktop in WSL2 QEMU (headless) and
# capture a screendump, OR launch a visible window.
#
# Usage: wsl_qemu_desktop.sh <arch> [run_seconds] [mode]
#   arch        = aarch64 | riscv64 | x86_64
#   run_seconds = how long to let the VM run before screendump (default 30)
#   mode        = "headless" (default, screendump) | "window" (visible SDL)
#
# Produces:
#   target/serial_<arch>.log      — serial console log
#   target/screendump_<arch>.ppm  — framebuffer capture (headless mode)
#   target/screendump_<arch>.png  — converted PNG (if tools available)
set -u

ARCH="${1:-aarch64}"
RUN_SECS="${2:-30}"
MODE="${3:-headless}"

KEI="$HOME/celestia/kei"
MON_PORT="${MON_PORT:-55555}"
SERIAL_LOG="$KEI/target/serial_${ARCH}.log"
SCREEN_PPM="$KEI/target/screendump_${ARCH}.ppm"
SCREEN_PNG="$KEI/target/screendump_${ARCH}.png"

# Locate kernel + initramfs. OSDK writes the kernel to target/osdk/aster-kernel/.
# We save per-arch copies under target/kernels/<arch>/ to avoid overwrite.
# aarch64 uses the raw ARM64 Image format (.image) so QEMU generates the FDT
# and passes it via x0. Other archs use the ELF (.qemu_elf).
if [ "$ARCH" = "aarch64" ]; then
  KERNEL="$KEI/target/kernels/${ARCH}/aster-kernel-osdk-bin.image"
else
  KERNEL="$KEI/target/kernels/${ARCH}/aster-kernel-osdk-bin.qemu_elf"
fi
INITRAMFS="$KEI/tests/initramfs/build/initramfs_desktop_${ARCH}.cpio.gz"

if [ ! -f "$KERNEL" ]; then
  echo "[err] kernel not found: $KERNEL"
  echo "      build it via cargo osdk build in WSL, then copy to target/kernels/${ARCH}/"
  exit 1
fi
if [ ! -f "$INITRAMFS" ]; then
  echo "[err] initramfs not found: $INITRAMFS"
  exit 1
fi

pkill -9 -f "qemu-system-${ARCH}" 2>/dev/null
sleep 1

# Build QEMU args per architecture.
QEMU="qemu-system-${ARCH}"
COMMON_ARGS=(--no-reboot -device virtio-keyboard-device -device virtio-serial-device)
APPEND="init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin"

if [ "$ARCH" = "aarch64" ]; then
  ARCH_ARGS=(-cpu cortex-a72 -machine virt,gic-version=3,virtualization=on -m 2G -smp 1
             -device virtio-gpu-device
             -netdev user,id=net0 -device virtio-net-device,netdev=net0)
elif [ "$ARCH" = "riscv64" ]; then
  ARCH_ARGS=(-cpu rv64 -machine virt -m 2G -smp 1
             -device virtio-gpu-device
             -netdev user,id=net0 -device virtio-net-device,netdev=net0)
elif [ "$ARCH" = "x86_64" ]; then
  ARCH_ARGS=(-cpu qemu64 -machine q35 -m 2G -smp 1
             -device virtio-gpu
             -netdev user,id=net0 -device virtio-net-pci,netdev=net0)
  APPEND="$APPEND console=ttyS0"
else
  echo "[err] unknown arch: $ARCH"; exit 1
fi

echo "[desktop-qemu] ARCH=$ARCH MODE=$MODE kernel=$KERNEL"
echo "[desktop-qemu] running VM for ${RUN_SECS}s..."

rm -f "$SERIAL_LOG"

if [ "$MODE" = "window" ]; then
  # Visible window mode: SDL display, serial to file, runs until window closed.
  "$QEMU" "${ARCH_ARGS[@]}" "${COMMON_ARGS[@]}" \
    -serial "file:$SERIAL_LOG" \
    -kernel "$KERNEL" -initrd "$INITRAMFS" -append "$APPEND" \
    -display sdl
  exit 0
fi

# Headless mode: capture screendump after RUN_SECS.
"$QEMU" "${ARCH_ARGS[@]}" "${COMMON_ARGS[@]}" \
  -display none \
  -serial "file:$SERIAL_LOG" \
  -monitor "tcp:127.0.0.1:$MON_PORT,server,nowait" \
  -kernel "$KERNEL" -initrd "$INITRAMFS" -append "$APPEND" \
  >/tmp/kei_qemu_stdout.log 2>&1 &
QEMU_PID=$!
echo "[desktop-qemu] QEMU pid=$QEMU_PID monitor=tcp://127.0.0.1:$MON_PORT"

# Wait for monitor
for i in $(seq 1 50); do
  if (exec 3<>/dev/tcp/127.0.0.1/$MON_PORT) 2>/dev/null; then
    exec 3>&- 3<&-
    echo "[desktop-qemu] monitor up after ${i}x0.2s"
    break
  fi
  sleep 0.2
done

cleanup() {
  { echo "quit"; sleep 0.3; } | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1
  kill -9 "$QEMU_PID" 2>/dev/null
  wait "$QEMU_PID" 2>/dev/null
}
trap cleanup EXIT

echo "[desktop-qemu] waiting ${RUN_SECS}s for boot + render..."
sleep "$RUN_SECS"

echo "[desktop-qemu] capturing screendump -> $SCREEN_PPM"
{
  echo "screendump $SCREEN_PPM"
  sleep 1
  echo "quit"
  sleep 0.3
} | nc -q 1 127.0.0.1 "$MON_PORT" >/dev/null 2>&1

if [ -f "$SCREEN_PPM" ]; then
  SZ=$(stat -c %s "$SCREEN_PPM" 2>/dev/null)
  echo "[desktop-qemu] screendump size=${SZ} bytes"
  # Convert + pixel stats via the python helper
  python3 - "$SCREEN_PPM" "$SCREEN_PNG" "$ARCH" <<'PY'
import sys, zlib, struct
ppm, png, arch = sys.argv[1], sys.argv[2], sys.argv[3]
with open(ppm,'rb') as f:
    # P6 header: "P6\n<w> <h>\n<maxval>\n"
    assert f.readline().strip()==b'P6', "not P6"
    w,h = map(int, f.readline().split())
    assert int(f.readline())==255
    data = f.read()
print(f"[{arch}] screendump: {w}x{h}, {len(data)} bytes")
nonblack = sum(1 for i in range(0,len(data),3) if any(data[i:i+3]))
print(f"[{arch}] non-black pixels: {nonblack}/{w*h} ({100*nonblack/(w*h):.1f}%)")
if len(data)>=3:
    r,g,b = data[0],data[1],data[2]
    print(f"[{arch}] first pixel RGB=({r},{g},{b}) = #{r:02x}{g:02x}{b:02x}")
# PNG write
def chunk(t,d):
    c=t+d; return struct.pack(">I",len(d))+c+struct.pack(">I",zlib.crc32(c)&0xFFFFFFFF)
raw=bytearray()
for y in range(h):
    raw.append(0); raw.extend(data[y*w*3:(y+1)*w*3])
idat=zlib.compress(bytes(raw),9)
open(png,'wb').write(b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack(">IIBBBBB",w,h,8,2,0,0,0))+chunk(b"IDAT",idat)+chunk(b"IEND",b""))
print(f"[{arch}] wrote PNG: {png}")
PY
else
  echo "[desktop-qemu] no screendump produced"
fi
echo "=== SERIAL LOG (last 30 lines) ==="
tail -30 "$SERIAL_LOG" 2>&1
