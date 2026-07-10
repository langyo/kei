#!/bin/bash
set -e
ROOTFS=/tmp/aarch64-rootfs

# /init is a copy of busybox (argv[0]="/init" triggers busybox init applet)
cp "$ROOTFS"/bin/busybox "$ROOTFS"/init
chmod +x "$ROOTFS"/init

# busybox init reads /etc/inittab or runs /etc/init.d/rcS
mkdir -p "$ROOTFS"/etc/init.d

# rcS script that starts dropbear
cat > "$ROOTFS"/etc/init.d/rcS << 'RCEOF'
#!/bin/busybox sh
mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
mkdir -p /var/run /tmp
echo "=== kei ignition (aarch64) ==="
echo "Starting dropbear on port 22..."
/sbin/dropbear -F -E -R -p 22 &
echo "Boot complete. SSH on port 22 (host port 2222)."
RCEOF
chmod +x "$ROOTFS"/etc/init.d/rcS

# inittab tells busybox init to run rcS then keep alive
cat > "$ROOTFS"/etc/inittab << 'INITEOF'
::sysinit:/etc/init.d/rcS
::respawn:/bin/sh
INITEOF

echo "=== rootfs init structure ==="
file "$ROOTFS"/init
echo "=== inittab ==="
cat "$ROOTFS"/etc/inittab
echo "=== rcS ==="
cat "$ROOTFS"/etc/init.d/rcS
