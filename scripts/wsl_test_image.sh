#!/usr/bin/env bash
# Test booting the raw ARM64 Image format (QEMU generates FDT for this format).
set -u
KEI="/mnt/d/源代码/工程项目/celestia/kei"

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

echo "[img-test] booting raw ARM64 Image format..."
qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot -display none \
  -device virtio-gpu-device \
  -kernel "$KEI/target/kernels/aarch64/aster-kernel-osdk-bin.image" \
  -initrd "$KEI/tests/initramfs/build/initramfs_desktop_aarch64.cpio.gz" \
  -append "init=/init" \
  -serial file:/tmp/img_serial.log \
  >/tmp/img_stdout.log 2>&1 &
QP=$!
echo "[img-test] QEMU pid=$QP"
sleep 12
if kill -0 $QP 2>/dev/null; then
  echo "[img-test] QEMU still running, killing"
  kill -9 $QP 2>/dev/null
fi
wait $QP 2>/dev/null
echo "[img-test] QEMU stdout/stderr:"
head -10 /tmp/img_stdout.log 2>&1
echo "[img-test] serial log:"
head -25 /tmp/img_serial.log 2>&1
echo "[img-test] serial log size: $(wc -c < /tmp/img_serial.log 2>/dev/null) bytes"
