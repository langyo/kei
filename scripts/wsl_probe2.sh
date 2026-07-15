#!/usr/bin/env bash
# Probe2: check if QEMU generates FDT for ELF kernel WITHOUT initrd.
set -u
KEI="/mnt/d/源代码/工程项目/celestia/kei"
MON=55562

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot -display none \
  -device virtio-gpu-device \
  -kernel "$KEI/target/kernels/aarch64/aster-kernel-osdk-bin.qemu_elf" \
  -serial file:/tmp/probe2_serial.log \
  -monitor tcp:127.0.0.1:$MON,server,nowait \
  >/dev/null 2>&1 &
QP=$!
echo "[probe2] QEMU pid=$QP"
for i in $(seq 1 40); do
  nc -z 127.0.0.1 $MON 2>/dev/null && { echo "[probe2] monitor up"; break; }
  sleep 0.2
done

echo "[probe2] checking registers + RAM..."
{
  echo "info registers"
  sleep 0.3
  # Scan the top 1MB of RAM in 4KB steps for FDT magic
  for off in 0 4096 8192 12288 16384 20480 24576 28672 32768 40960 49152 57344 65536 73728 81920 90112 98304 106496 114688 122880 131072; do
    addr=$(printf "0x%X" $((0xC0000000 - off)))
    echo "xp /1wx $addr"
    sleep 0.04
  done
  echo "quit"
  sleep 0.5
} | nc -q 1 127.0.0.1 $MON 2>/dev/null | tr -d '\r' > /tmp/probe2_out.txt

echo "[probe2] x0 register (look for x0 value):"
grep -iE "^x0 |x0=0x" /tmp/probe2_out.txt | head -3
echo "[probe2] RAM reads with non-zero or FDT magic:"
grep -E "^[0-9a-f]{16}:" /tmp/probe2_out.txt | grep -viE ": 0x00000000$" | head -10
echo "[probe2] all RAM reads:"
grep -E "^[0-9a-f]{16}:" /tmp/probe2_out.txt | head -25

kill -9 $QP 2>/dev/null
wait $QP 2>/dev/null
echo "---serial (first 8 lines)---"
head -8 /tmp/probe2_serial.log 2>&1
