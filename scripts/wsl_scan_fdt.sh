#!/usr/bin/env bash
# Scan QEMU RAM for the FDT magic (0xd00dfeed) to locate the device tree.
# Run inside WSL.
set -u
KEI="/mnt/d/源代码/工程项目/celestia/kei"
KERNEL="$KEI/target/kernels/aarch64/aster-kernel-osdk-bin.qemu_elf"
MON=55558

pkill -9 -f qemu-system-aarch64 2>/dev/null
sleep 1

# Launch QEMU with monitor (no initrd, to isolate FDT placement).
qemu-system-aarch64 \
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
  -m 2G -smp 1 --no-reboot -display none \
  -device virtio-gpu-device \
  -kernel "$KERNEL" \
  -serial file:/tmp/fdt_scan_serial.log \
  -monitor tcp:127.0.0.1:$MON,server,nowait \
  >/dev/null 2>&1 &
QP=$!
echo "[scan] QEMU pid=$QP"

# Wait for monitor
for i in $(seq 1 40); do
  nc -z 127.0.0.1 $MON 2>/dev/null && { echo "[scan] monitor up"; break; }
  sleep 0.2
done

# Scan from 0xBFFF0000 down to 0x40000000 in 1MB steps for FDT magic.
echo "[scan] searching RAM for FDT magic 0xd00dfeed..."
{
  # Search high RAM first (where QEMU normally puts DTB)
  for addr in 0xBFFFF000 0xBFFEF000 0xBFFDF000 0xBFFCF000 0xBFFBF000 \
              0xBFFAF000 0xBFF9F000 0xBFF8F000 0xBFF7F000 0xBFF6F000 \
              0xBFF5F000 0xBFF4F000 0xBFF3F000 0xBFF2F000 0xBFF1F000 \
              0xBFF0F000 0xBFEFF000 0xBFDEF000 0xBFCFF000 0xBFAFF000 \
              0xBF8FF000 0xBF6FF000 0xBF4FF000 0xBF2FF000 0xBF0FF000 \
              0xBEFFF000 0xBDEFF000 0xBCEFF000 0xBBEFF000 0xBAEFF000; do
    echo "xp /1wx $addr"
    sleep 0.05
  done
  # Also check near kernel (0x40080000 + some offset)
  for addr in 0x40100000 0x40200000 0x40300000 0x40400000 0x40500000 \
              0x41000000 0x42000000 0x48000000 0x50000000; do
    echo "xp /1wx $addr"
    sleep 0.05
  done
  echo "quit"
  sleep 0.5
} | nc -q 1 127.0.0.1 $MON 2>/dev/null > /tmp/fdt_scan_out.txt
echo "[scan] raw monitor output (first 60 lines):"
head -60 /tmp/fdt_scan_out.txt
echo "[scan] lines with d00dfeed:"
grep -i "d00dfeed" /tmp/fdt_scan_out.txt | head

kill -9 $QP 2>/dev/null
wait $QP 2>/dev/null
echo "[scan] done"
echo "---serial log (first 12 lines)---"
head -12 /tmp/fdt_scan_serial.log 2>&1
