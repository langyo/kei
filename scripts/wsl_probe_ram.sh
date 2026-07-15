#!/usr/bin/env bash
# Probe QEMU RAM: check kernel region (should be non-zero) and scan for FDT.
set -u
KEI="/mnt/d/源代码/工程项目/celestia/kei"
MON=55560

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot -display none \
  -device virtio-gpu-device \
  -kernel "$KEI/target/kernels/aarch64/aster-kernel-osdk-bin.qemu_elf" \
  -initrd "$KEI/tests/initramfs/build/initramfs_desktop_aarch64.cpio.gz" \
  -append "init=/init" \
  -serial file:/tmp/probe_serial.log \
  -monitor tcp:127.0.0.1:$MON,server,nowait \
  >/dev/null 2>&1 &
QP=$!
echo "[probe] QEMU pid=$QP"

for i in $(seq 1 40); do
  nc -z 127.0.0.1 $MON 2>/dev/null && { echo "[probe] monitor up"; break; }
  sleep 0.2
done

echo "[probe] reading kernel region + scanning for FDT magic..."
{
  # kernel region (should be non-zero if xp works)
  echo "xp /8wx 0x40080000"
  sleep 0.2
  # Scan high RAM in larger steps for FDT magic
  for mb in 0 1 2 4 8 16 32 64 128 256 512 768 1024 1280 1536 1792; do
    addr=$(printf "0x%X" $((0xBFFF0000 - mb * 0x100000)))
    echo "xp /1wx $addr"
    sleep 0.05
  done
  echo "quit"
  sleep 0.5
} | nc -q 1 127.0.0.1 $MON 2>/dev/null > /tmp/probe_out.txt

echo "[probe] raw output:"
cat /tmp/probe_out.txt 2>&1 | tr -d '\r' | grep -E "^[0-9a-f]{16}:|d00dfeed" | head -30

kill -9 $QP 2>/dev/null
wait $QP 2>/dev/null
echo "---serial (first 8 lines)---"
head -8 /tmp/probe_serial.log 2>&1
