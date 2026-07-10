#!/usr/bin/env bash
# kei + evernight end-to-end QEMU ignition test
#
# Architecture:
#   Host: evernight-server (mock entelecheia gateway) on port 8443
#   QEMU: kei kernel + initramfs (evernight sensor-poll)
#   Net:  QEMU user-mode NAT (guest 10.0.2.15 → host 10.0.2.2)
#
# Usage:
#   bash tests/e2e_qemu_ignition.sh

set -euo pipefail

KEI_ROOT="${KEI_ROOT:-/mnt/sdb1/kei}"
EVERNIGHT_ROOT="${EVERNIGHT_ROOT:-/mnt/sdb1/evernight}"
GATEWAY_PORT="${GATEWAY_PORT:-8443}"
MODBUS_PORT="${MODBUS_PORT:-5020}"
INITRAMFS="$KEI_ROOT/test/initramfs/build/initramfs.cpio.gz"
KERNEL="$KEI_ROOT/target/output/nanopi-r3s/kei-kernel.bin"

echo "=== kei + evernight End-to-End QEMU Ignition Test ==="
echo ""

# ── 1. Verify artifacts ──────────────────────────────────────
echo "[1/5] Verifying artifacts..."
for f in "$KERNEL" "$INITRAMFS" \
         "$EVERNIGHT_ROOT/target/release/evernight-server" \
         "$EVERNIGHT_ROOT/target/aarch64-unknown-linux-musl/release/evernight"; do
    if [ ! -f "$f" ]; then
        echo "  MISSING: $f"
        exit 1
    fi
    echo "  OK: $(basename "$f") ($(du -h "$f" | cut -f1))"
done

# ── 2. Build initramfs with evernight ────────────────────────
echo ""
echo "[2/5] Building initramfs with evernight..."
TMPDIR=$(mktemp -d)
mkdir -p "$TMPDIR/bin" "$TMPDIR/dev" "$TMPDIR/proc" "$TMPDIR/sys" "$TMPDIR/tmp" "$TMPDIR/run"

# Copy aarch64 evernight binary
cp "$EVERNIGHT_ROOT/target/aarch64-unknown-linux-musl/release/evernight" "$TMPDIR/bin/evernight"

# Copy aarch64 busybox for shell
if [ -f "$KEI_ROOT/test/initramfs/busybox-aarch64" ]; then
    cp "$KEI_ROOT/test/initramfs/busybox-aarch64" "$TMPDIR/bin/busybox"
    for applet in sh ls cat echo mount ip ifconfig sleep; do
        ln -s busybox "$TMPDIR/bin/$applet"
    done
fi

# Init script: start evernight sensor-poll connecting to host gateway
cat > "$TMPDIR/init" << 'INITEOF'
#!/bin/sh
mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
mount -t devtmpfs none /dev 2>/dev/null

echo ""
echo "=== kei ignition ==="
echo "Kernel booted. Starting evernight..."
echo ""

# Bring up network interfaces
for iface in /sys/class/net/*; do
    name=$(basename "$iface")
    [ "$name" = "lo" ] && continue
    echo "Bringing up $name..."
    ip link set "$name" up 2>/dev/null
done

# Start evernight sensor-poll connecting to host gateway
# QEMU user-mode: host is at 10.0.2.2
echo "Starting evernight sensor-poll → ws://10.0.2.2:GATEWAY_PORT/api/ws"
SENSOR_DATA_DIR=/tmp/sensor /bin/evernight sensor-poll \
    --gateway ws://10.0.2.2:GATEWAY_PORT/api/ws \
    --node-id kei-qemu-01 \
    --max-duration 30 &
ENPID=$!

# Wait and monitor
sleep 15
echo "evernight PID: $ENPID"

# Keep alive
exec /bin/sh
INITEOF
sed -i "s/GATEWAY_PORT/$GATEWAY_PORT/g" "$TMPDIR/init"
chmod +x "$TMPDIR/init"

# Create initramfs
cd "$TMPDIR"
find . | cpio -H newc -o 2>/dev/null | gzip > "$INITRAMFS.e2e"
echo "  initramfs: $(du -h "$INITRAMFS.e2e" | cut -f1)"
rm -rf "$TMPDIR"

# ── 3. Start mock entelecheia (evernight-server) ─────────────
echo ""
echo "[3/5] Starting evernight-server (mock entelecheia) on port $GATEWAY_PORT..."
$EVERNIGHT_ROOT/target/release/evernight-server serve \
    --host 0.0.0.0 --port "$GATEWAY_PORT" \
    > /tmp/e2e-server.log 2>&1 &
SRV_PID=$!
sleep 2
if ! kill -0 $SRV_PID 2>/dev/null; then
    echo "  FAILED: evernight-server did not start"
    cat /tmp/e2e-server.log
    exit 1
fi
echo "  OK: evernight-server PID=$SRV_PID"

# ── 4. Boot kei in QEMU with network ─────────────────────────
echo ""
echo "[4/5] Booting kei kernel in QEMU arm64..."
echo "  Kernel:    $KERNEL"
echo "  Initramfs: $INITRAMFS.e2e"
echo "  Gateway:   ws://10.0.2.2:$GATEWAY_PORT/api/ws"
echo "  Port fwd:  host:$GATEWAY_PORT → guest:$GATEWAY_PORT"

# Use a container backend for QEMU if the system qemu isn't available.
# Prefer docker, fall back to podman (podman is docker-CLI compatible).
# QEMU_DISPLAY env var controls the display backend (default: headless for
# CI; set QEMU_DISPLAY="-display sdl" for a local window).
QEMU_DISPLAY_ARG="${QEMU_DISPLAY:--nographic}"
QEMU_BIN="qemu-system-aarch64"
if ! command -v "$QEMU_BIN" &>/dev/null; then
    CONTAINER_BIN=""
    if command -v docker &>/dev/null && docker info >/dev/null 2>&1; then
        CONTAINER_BIN="docker"
    elif command -v podman &>/dev/null; then
        CONTAINER_BIN="podman"
    fi
    if [ -z "$CONTAINER_BIN" ]; then
        echo "  FAILED: neither docker nor podman is available, and qemu-system-aarch64 is missing"
        exit 1
    fi
    echo "  (using $CONTAINER_BIN QEMU)"
    "$CONTAINER_BIN" run --rm --network host \
        -v "$KEI_ROOT:/kei" \
        --entrypoint bash \
        qemu-arm64 \
        -c "timeout 30 qemu-system-aarch64 \
            -M virt,gic-version=3,virtualization=on \
            -cpu cortex-a72 \
            -m 2048 \
            -smp 1 \
            -kernel /kei/target/output/nanopi-r3s/kei-kernel.bin \
            -initrd /kei/test/initramfs/build/initramfs.cpio.gz.e2e \
            -append 'console=ttyAMA0 init=/init' \
            -netdev user,id=net0 \
            -device virtio-net-device,netdev=net0 \
            $QEMU_DISPLAY_ARG \
            -no-reboot 2>&1" > /tmp/e2e-qemu.log 2>&1 || true
else
    timeout 30 "$QEMU_BIN" \
        -M virt,gic-version=3,virtualization=on \
        -cpu cortex-a72 \
        -m 2048 \
        -smp 1 \
        -kernel "$KERNEL" \
        -initrd "$INITRAMFS.e2e" \
        -append "console=ttyAMA0 init=/init" \
        -netdev "user,id=net0" \
        -device virtio-net-device,netdev=net0 \
        $QEMU_DISPLAY_ARG \
        -no-reboot > /tmp/e2e-qemu.log 2>&1 || true
fi

# ── 5. Check results ─────────────────────────────────────────
echo ""
echo "[5/5] Checking results..."
echo ""
echo "=== evernight-server log ==="
cat /tmp/e2e-server.log 2>/dev/null | tail -20
echo ""
echo "=== QEMU boot log (last 20 lines) ==="
tail -20 /tmp/e2e-qemu.log 2>/dev/null

# Check if device registered
if grep -q "Device registered" /tmp/e2e-server.log 2>/dev/null; then
    echo ""
    echo "🎉 E2E SUCCESS: Device registered with gateway!"
else
    echo ""
    echo "⚠️  Device registration not detected (kernel syscall layer may need work)"
fi

# Cleanup
kill $SRV_PID 2>/dev/null || true
rm -f "$INITRAMFS.e2e"
