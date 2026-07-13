#!/bin/bash
# Run the kei aarch64 kernel in QEMU with virtio-mmio tracing enabled.
# Usage: tools/run_aarch64_trace.sh [timeout_seconds]
# Captures kernel serial output + QEMU virtio-mmio trace to target/trace.log

set -e
cd /opt/kei

KERNEL=target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf
TIMEOUT=${1:-25}

if [ ! -f "$KERNEL" ]; then
    echo "ERROR: kernel not found at $KERNEL"
    echo "Run: cargo osdk build --target-arch aarch64 --scheme aarch64 --release"
    exit 1
fi

mkdir -p target

echo "[run] kernel: $KERNEL"
echo "[run] timeout: ${TIMEOUT}s"
echo "[run] trace:  target/trace.log"

# Build the virtio trace events list
TRACE_EVENTS="virtio_mmio_read,virtio_mmio_write_offset,virtio_mmio_queue_write,virtio_queue_notify,virtio_set_status,virtio_mmio_setting_irq,virtio_mmio_guest_page,virtio_gpu_cmd_get_display_info,virtio_gpu_cmd_set_scanout,virtio_gpu_cmd_res_create_2d,virtio_gpu_cmd_res_back_attach,virtio_gpu_cmd_res_xfer_toh_2d,virtio_gpu_cmd_res_flush"

# Headless (no display) for tracing the boot sequence.
timeout ${TIMEOUT}s qemu-system-aarch64 \
    -cpu cortex-a72 \
    -machine virt,gic-version=3,virtualization=on \
    -m 2G \
    -smp 1 \
    --no-reboot \
    -nographic \
    -serial mon:stdio \
    -kernel "$KERNEL" \
    -device virtio-gpu-device \
    -device virtio-keyboard-device \
    -trace "${TRACE_EVENTS}" \
    2>&1 | tee target/trace.log | head -200

echo "[run] === full trace saved to target/trace.log ==="
echo "[run] virtio_mmio write offsets seen:"
grep -oP 'virtio_mmio_write_offset.*?offset\s*0x[0-9a-f]+' target/trace.log 2>/dev/null | grep -oP '0x[0-9a-f]+' | sort | uniq -c | sort -rn || echo "(none)"
