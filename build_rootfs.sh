#!/bin/bash
set -e
KEI="/mnt/d/源代码/工程项目/celestia/kei"
DROPBEAR_SRC=/tmp/dropbear-2024.86
ROOTFS=/tmp/aarch64-rootfs

rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"/{bin,sbin,etc/dropbear,dev,proc,sys,tmp,root,run,var/log,lib}

# busybox (static)
cp "$KEI"/test/initramfs/busybox-aarch64 "$ROOTFS"/bin/busybox
chmod +x "$ROOTFS"/bin/busybox

# busybox applet symlinks
cd "$ROOTFS"/bin
for cmd in sh echo cat ls mount ip ifconfig sleep ps kill mkdir rm cp mv ln chmod chown id uname hostname pwd env true false test head tail wc grep sed awk vi ping udhcpc date whoami reboot poweroff; do
    ln -sf busybox "$cmd"
done
cd "$KEI"

# dropbear (dynamic)
cp "$DROPBEAR_SRC"/dropbear "$ROOTFS"/sbin/dropbear
cp "$DROPBEAR_SRC"/dropbearkey "$ROOTFS"/sbin/dropbearkey
chmod +x "$ROOTFS"/sbin/dropbear "$ROOTFS"/sbin/dropbearkey

# aarch64 shared libraries for dynamic dropbear
cp /usr/aarch64-linux-gnu/lib/libc.so.6 "$ROOTFS"/lib/ 2>/dev/null || true
for f in /usr/aarch64-linux-gnu/lib/ld-*.so.*; do
    cp "$f" "$ROOTFS"/lib/ 2>/dev/null || true
done
cd "$ROOTFS"/lib
for f in ld-*.so.*; do
    [ -f "$f" ] && ln -sf "$f" ld-linux-aarch64.so.1 && break
done
cd "$KEI"

# init script
cp "$KEI"/test/initramfs/src/init_aarch64 "$ROOTFS"/init
chmod +x "$ROOTFS"/init

# config files
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$ROOTFS"/etc/passwd
printf 'root:x:0:\n' > "$ROOTFS"/etc/group

# authorized_keys
if [ -f /tmp/client_ssh_key.pub ]; then
    cp /tmp/client_ssh_key.pub "$ROOTFS"/etc/dropbear/authorized_keys
    echo "authorized_keys installed"
fi

echo "=== rootfs ready ==="
ls "$ROOTFS"/sbin/
ls "$ROOTFS"/lib/
