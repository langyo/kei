#!/usr/bin/env bash
# Launch the kei QEMU demo end-to-end:
#   1. Build the firmware for thumbv7em-none-eabi
#   2. Build the host_gateway example
#   3. Start QEMU with the firmware, piping UART to the gateway
#
# Usage: ./run_demo.sh
#
# Requires: qemu-system-arm, rustup target thumbv7em-none-eabi
set -euo pipefail

cd "$(dirname "$0")"

echo "=== Building firmware (thumbv7em-none-eabi) ==="
cargo build --release 2>&1 | tail -3

ELF="target/thumbv7em-none-eabi/release/kei-qemu-demo"

if [ ! -f "$ELF" ]; then
    echo "ERROR: firmware not found at $ELF"
    exit 1
fi

echo "=== Building host gateway ==="
cargo build --example host_gateway --target x86_64-unknown-linux-gnu 2>&1 | tail -3 || \
    cargo build --example host_gateway 2>&1 | tail -3

GATEWAY="target/debug/examples/host_gateway"
GATEWAY_RELEASE="target/release/examples/host_gateway"
GW="${GATEWAY_RELEASE:-$GATEWAY}"
[ -f "$GATEWAY" ] && GW="$GATEWAY"

echo "=== Starting QEMU + gateway ==="
echo "Firmware: $ELF"
echo "Gateway:  $GW"
echo ""

# QEMU UART0 → stdout, gateway reads stdin.
# The firmware sends both human-readable debug lines AND binary wire frames.
# The gateway's FrameDecoder will skip non-frame bytes (no 0xCE magic).
qemu-system-arm \
    -M mps2-an386 \
    -cpu cortex-m4 \
    -m 4M \
    -nographic \
    -semihosting \
    -serial stdio \
    -kernel "$ELF" 2>&1 | "$GW"
