#!/bin/bash
# Build a bootable GRUB (multiboot2) ISO for the kei x86_64 kernel.
#
# Why an ISO: the kei bzImage stub only implements the EFI handover entry
# (its real-mode setup area is zeroed), so `qemu -kernel` cannot boot it on
# machines without OVMF, and `qemu -kernel` refuses ELF64 multiboot images.
# GRUB's multiboot2 path loads the OSDK ELF (which carries both multiboot1
# and multiboot2 headers) directly.
#
# This script is meant to run inside WSL (or any Linux) — it uses
# grub-mkimage and xorriso. On machines without sudo, it unpacks the
# required tools from Ubuntu .deb packages into $GRUBROOT (default
# ~/grubroot/root) with apt-get download + dpkg -x.
#
# Usage:
#   bash scripts/build_x86_64_iso.sh
#
# Inputs (must exist):
#   target/osdk/aster-kernel-osdk-bin.qemu_elf   (cargo osdk build --target-arch x86_64)
#   tests/initramfs/build/initramfs.cpio.gz      (tests/initramfs/build_x86_64_rootfs.sh)
# Output:
#   target/x86_64-iso/kei.iso
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

KERNEL_ELF="$ROOT/target/osdk/aster-kernel-osdk-bin.qemu_elf"
INITRD="$ROOT/tests/initramfs/build/initramfs.cpio.gz"
ISO_DIR="$ROOT/target/x86_64-iso"
GRUBROOT="${GRUBROOT:-$HOME/grubroot/root}"

for f in "$KERNEL_ELF" "$INITRD"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: missing $f"
        echo "  kernel:   cargo osdk build --target-arch x86_64 --scheme microvm --release"
        echo "  initrd:   bash tests/initramfs/build_x86_64_rootfs.sh"
        exit 1
    fi
done

# --- Ensure grub-mkimage / xorriso are available ---------------------------
need_grubroot=0
[ -x "$GRUBROOT/usr/bin/grub-mkimage" ] || need_grubroot=1
[ -x "$GRUBROOT/usr/bin/xorriso" ] || need_grubroot=1
if [ "$need_grubroot" -eq 1 ]; then
    echo "[iso] unpacking grub/xorriso tools into $GRUBROOT (no sudo needed)"
    PKGS="grub-common grub-pc-bin xorriso libisoburn1t64 libburn4t64 libisofs6t64"
    WORK="$(mktemp -d)"
    mkdir -p "$GRUBROOT"
    (cd "$WORK" && apt-get download $PKGS)
    for deb in "$WORK"/*.deb; do
        dpkg -x "$deb" "$GRUBROOT"
    done
    rm -rf "$WORK"
fi

MKIMAGE="$GRUBROOT/usr/bin/grub-mkimage"
GRUB_PC="$GRUBROOT/usr/lib/grub/i386-pc"
XORRISO="$GRUBROOT/usr/bin/xorriso"
export LD_LIBRARY_PATH="$GRUBROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# --- Stage the ISO tree -----------------------------------------------------
mkdir -p "$ISO_DIR/iso/boot/grub"
cp -f "$KERNEL_ELF" "$ISO_DIR/iso/boot/kei-kernel.elf"
cp -f "$INITRD" "$ISO_DIR/iso/boot/initramfs.cpio.gz"

# console=ttyS0 routes /dev/console to the UART so userspace output appears
# on the serial line (the VT console has no usable framebuffer here).
cat > "$ISO_DIR/iso/boot/grub/grub.cfg" <<'EOF'
set timeout=2
serial --unit=0 --speed=115200 --word=8 --parity=no --stop=1
terminal_input serial
terminal_output serial

menuentry 'kei' {
    multiboot2 /boot/kei-kernel.elf SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin init=/init console=ttyS0 -- sh -l
    module2 --nounzip /boot/initramfs.cpio.gz
    boot
}
EOF

# --- Build the El Torito boot image and the ISO -----------------------------
cd "$ISO_DIR"
"$MKIMAGE" -O i386-pc -d "$GRUB_PC" -o core.img -p /boot/grub \
    biosdisk iso9660 multiboot2 normal configfile echo serial ls test search all_video
cat "$GRUB_PC/cdboot.img" core.img > iso/boot/grub/boot.img
"$XORRISO" -as mkisofs -o kei.iso -V KEI -R \
    -b boot/grub/boot.img -no-emul-boot -boot-load-size 4 -boot-info-table \
    iso 2>&1 | tail -1 || true

echo "[iso] wrote $ISO_DIR/kei.iso ($(stat -c%s "$ISO_DIR/kei.iso") bytes)"
