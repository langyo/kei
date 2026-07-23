# U-Boot boot script for kei kernel on NanoPi R3S.
#
# Flashed at /boot.scr on the SD card.
# Compile with: mkimage -C none -A arm -T script -d boot.cmd boot.scr
#
# kei kernel is linked at KERNEL_LMA=0x40080000 (QEMU virt RAM base).
# On real RK3566, RAM starts at 0x00200000. We must explicitly tell
# U-Boot to load the kernel at its linked address (0x40080000) because
# the kei kernel is NOT position-independent.

test -n "${distro_bootpart}" || distro_bootpart=1

echo "Booting kei kernel on NanoPi R3S..."

# Load Armbian environment
if test -e ${devtype} ${devnum}:${distro_bootpart} /boot/armbianEnv.txt; then
    load ${devtype} ${devnum}:${distro_bootpart} ${load_addr} /boot/armbianEnv.txt
    env import -t ${load_addr} ${filesize}
fi

# kei kernel is linked for QEMU virt (KERNEL_LMA=0x40080000).
# Force load addresses to match.
setenv kernel_addr_r 0x40080000
setenv fdt_addr_r    0x50000000
setenv ramdisk_addr_r 0x51000000

setenv bootargs "console=ttyS2,1500000n8 earlycon init=/bin/sh"

echo "Loading kernel to ${kernel_addr_r}..."
load ${devtype} ${devnum}:${distro_bootpart} ${kernel_addr_r} /boot/kei-kernel.bin
echo "Loading DTB to ${fdt_addr_r}..."
load ${devtype} ${devnum}:${distro_bootpart} ${fdt_addr_r} /boot/board.dtb
echo "Loading initramfs to ${ramdisk_addr_r}..."
load ${devtype} ${devnum}:${distro_bootpart} ${ramdisk_addr_r} /boot/initramfs.cpio.gz

fdt addr ${fdt_addr_r}

booti ${kernel_addr_r} ${ramdisk_addr_r} ${fdt_addr_r}

# Recompile with:
# mkimage -C none -A arm -T script -d boot.cmd boot.scr
