#!/bin/bash
# Build the x86_64 serial-console initramfs (busybox + init) at
# tests/initramfs/build/initramfs.cpio.gz — the path referenced by the
# default [run.boot] section in OSDK.toml.
#
# Mirrors build_aarch64_rootfs.sh, minus dropbear (x86_64 uses the serial
# console shell instead of SSH).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
ROOTFS="$BUILD_DIR/rootfs_x86_64"
OUT="$BUILD_DIR/initramfs.cpio.gz"

BUSYBOX="$SCRIPT_DIR/busybox-x86_64"
if [ ! -f "$BUSYBOX" ]; then
    echo "[x86_64-rootfs] busybox-x86_64 not found, downloading static musl build..."
    curl -fsSL -o "$BUSYBOX" \
        "https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox"
    chmod +x "$BUSYBOX"
fi

rm -rf "$ROOTFS"
mkdir -p "$ROOTFS/bin" "$ROOTFS/dev" "$ROOTFS/proc" "$ROOTFS/sys" "$ROOTFS/etc" "$ROOTFS/tmp"

cp "$BUSYBOX" "$ROOTFS/bin/busybox"
chmod +x "$ROOTFS/bin/busybox"
for applet in sh ls cat mount echo sleep mkdir ip; do
    ln -sf busybox "$ROOTFS/bin/$applet"
done

cp "$SCRIPT_DIR/src/init_x86_64" "$ROOTFS/init"
chmod +x "$ROOTFS/init"

# Minimal user database so `id`/`login` lookups do not fail.
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$ROOTFS/etc/passwd"
printf 'root:x:0:\n' > "$ROOTFS/etc/group"

mkdir -p "$BUILD_DIR"
python3 "$SCRIPT_DIR/build_aarch64_cpio.py" "$ROOTFS" "$OUT"
echo "[x86_64-rootfs] wrote $OUT ($(stat -c%s "$OUT") bytes)"
