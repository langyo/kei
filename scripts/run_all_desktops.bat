@echo off
REM run_all_desktops.bat — launch kei desktop QEMU windows for all architectures.
REM
REM Opens visible SDL windows:
REM   - aarch64:  full aris-render Windows-like desktop (shittim-chest wallpaper,
REM               taskbar, start menu, icons, window). Boots to userspace + renders.
REM   - riscv64:  kei kernel boots in S-mode (OpenSBI banner + ostd init).
REM               Display path not yet implemented; serial console shown.
REM   - x86_64:   kei kernel (multiboot1 ELF; QEMU -kernel cannot load 64-bit
REM               ELF directly, so this shows the boot attempt / serial).
REM
REM Usage: run_all_desktops.bat
setlocal

set KEI=%~dp0..
set QEMU_BASE=C:\Program Files\qemu

echo [desktops] launching kei QEMU windows...

REM === aarch64 (full desktop render) ===
start "kei-aarch64" "%QEMU_BASE%\qemu-system-aarch64.exe" ^
  -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on ^
  -m 2G -smp 1 --no-reboot ^
  -device virtio-gpu-device ^
  -device virtio-keyboard-device ^
  -device virtio-serial-device ^
  -serial file:"%KEI%\target\serial_aarch64.log" ^
  -netdev user,id=net0 -device virtio-net-device,netdev=net0 ^
  -kernel "%KEI%\target\kernels\aarch64\aster-kernel-osdk-bin.image" ^
  -initrd "%KEI%\tests\initramfs\build\initramfs_desktop_aarch64.cpio.gz" ^
  -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin" ^
  -display sdl

timeout /t 3 /nobreak >nul

REM === riscv64 (kernel boots to ostd init; serial console) ===
start "kei-riscv64" "%QEMU_BASE%\qemu-system-riscv64.exe" ^
  -cpu rv64,svpbmt=true,zkr=true ^
  -machine virt ^
  -m 2G -smp 1 --no-reboot ^
  -device virtio-gpu-device ^
  -device virtio-keyboard-device ^
  -device virtio-serial-device ^
  -serial file:"%KEI%\target\serial_riscv64.log" ^
  -netdev user,id=net0 -device virtio-net-device,netdev=net0 ^
  -kernel "%KEI%\target\kernels\riscv64\aster-kernel-osdk-bin.qemu_elf" ^
  -initrd "%KEI%\tests\initramfs\build\initramfs_desktop_riscv64.cpio.gz" ^
  -append "init=/init" ^
  -display sdl

timeout /t 3 /nobreak >nul

REM === x86_64 (kernel built; multiboot1 ELF needs GRUB loader, show boot attempt) ===
REM Note: QEMU -kernel cannot load a 64-bit ELF; this opens the window to show
REM the QEMU boot attempt. A GRUB ISO is needed for full boot (not available here).
start "kei-x86_64" "%QEMU_BASE%\qemu-system-x86_64.exe" ^
  -cpu qemu64 -machine q35 ^
  -m 2G -smp 1 --no-reboot ^
  -device virtio-gpu ^
  -device virtio-keyboard-pci ^
  -serial file:"%KEI%\target\serial_x86_64.log" ^
  -netdev user,id=net0 -device virtio-net-pci,netdev=net0 ^
  -kernel "%KEI%\target\kernels\x86_64\aster-kernel-osdk-bin.qemu_elf" ^
  -initrd "%KEI%\tests\initramfs\build\initramfs_desktop_x86_64.cpio.gz" ^
  -append "init=/init console=ttyS0" ^
  -display sdl

echo [desktops] windows launched. Serial logs: %KEI%\target\serial_*.log
echo [desktops] close the QEMU windows to stop the VMs.
endlocal
