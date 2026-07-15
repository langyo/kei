#!/usr/bin/env bash
# Test riscv64 kei kernel boot (serial only, headless).
set -u
KEI="/mnt/d/源代码/工程项目/celestia/kei"
KERNEL="$KEI/target/kernels/riscv64/aster-kernel-osdk-bin.qemu_elf"
INITRAMFS="$KEI/tests/initramfs/build/initramfs_desktop_riscv64.cpio.gz"

pkill -9 -f qemu-system-riscv64 2>/dev/null
sleep 1

echo "[rv64] booting riscv64 kernel..."
# riscv64 virt uses the built-in OpenSBI BIOS which boots the kernel in S-mode.
# -bios default uses QEMU's fw_dynamic OpenSBI; -kernel loads our ELF.
timeout 30 qemu-system-riscv64 \
  -cpu rv64,svpbmt=true,zkr=true \
  -machine virt \
  -m 2G -smp 1 --no-reboot \
  -nographic \
  -serial file:/tmp/rv64_boot.log \
  -kernel "$KERNEL" \
  -initrd "$INITRAMFS" \
  -append "init=/init" \
  >/tmp/rv64_stdout.log 2>&1
RC=$?
echo "[rv64] QEMU exit code: $RC"
echo "[rv64] stdout/stderr:"
head -10 /tmp/rv64_stdout.log 2>&1
echo "[rv64] serial log (first 40 lines):"
head -40 /tmp/rv64_boot.log 2>&1
echo "[rv64] serial log size: $(wc -c < /tmp/rv64_boot.log 2>/dev/null) bytes"
