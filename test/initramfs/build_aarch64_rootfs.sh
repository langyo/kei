#!/bin/bash
# Builds the aarch64 initramfs rootfs directory.
# Run on the WSL/Linux host: bash build_aarch64_rootfs.sh
set -e

ROOTFS=/tmp/aarch64-rootfs
KEI_ROOT="/mnt/d/源代码/工程项目/celestia/kei"
DROPBEAR_SRC=/tmp/dropbear-2022.83

rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"/{bin,sbin,etc/dropbear,dev,proc,sys,tmp,root,run,var/log}

# busybox
cp "$KEI_ROOT"/test/initramfs/busybox-aarch64 "$ROOTFS"/bin/busybox
chmod +x "$ROOTFS"/bin/busybox

# busybox applet symlinks
cd "$ROOTFS"/bin
APPLETS="sh echo cat ls mount ip ifconfig sleep ps kill mkdir rm cp mv ln chmod chown id uname hostname pwd env true false test head tail wc grep sed awk vi ping udhcpc date uname whoami reboot poweroff"
for cmd in $APPLETS; do
    ln -sf busybox "$cmd"
done
cd -

# dropbear + dropbearkey
cp "$DROPBEAR_SRC"/dropbear "$ROOTFS"/sbin/dropbear
cp "$DROPBEAR_SRC"/dropbearkey "$ROOTFS"/sbin/dropbearkey
chmod +x "$ROOTFS"/sbin/dropbear "$ROOTFS"/sbin/dropbearkey

# init script
cp "$KEI_ROOT"/test/initramfs/src/init_aarch64 "$ROOTFS"/init
chmod +x "$ROOTFS"/init

# config files (dropbear needs getpwnam("root"))
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$ROOTFS"/etc/passwd
printf 'root:x:0:\n' > "$ROOTFS"/etc/group

# authorized_keys (if a client key was generated)
if [ -f /tmp/client_ssh_key.pub ]; then
    cp /tmp/client_ssh_key.pub "$ROOTFS"/etc/dropbear/authorized_keys
fi

echo "=== bin symlinks ==="
ls "$ROOTFS"/bin/ | tr '\n' ' '; echo
echo "=== sbin ==="
ls "$ROOTFS"/sbin/
echo "=== rootfs ready at $ROOTFS ==="
