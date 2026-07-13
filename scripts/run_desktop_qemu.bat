@echo off
REM run_desktop_qemu.bat — launch the kei desktop in QEMU for one architecture.
REM
REM Usage: run_desktop_qemu.bat <arch> [window]
REM   arch   = aarch64 | riscv64 | x86_64
REM   window = window title suffix (optional, defaults to arch)
REM
REM Launches a visible QEMU SDL window showing the kei desktop rendered by
REM aris-render (kei_desktop) at 800x600. The kernel + initramfs must already
REM be built (see scripts/build_desktop_initramfs.py and the kernel build).

setlocal enabledelayedexpansion

set ARCH=%~1
if "%ARCH%"=="" (
  echo Usage: %0 ^<aarch64^|riscv64^|x86_64^> [window_title]
  exit /b 1
)
set WINTITLE=%~2
if "%WINTITLE%"=="" set WINTITLE=%ARCH%

set KEI=%~dp0..
set QEMU="C:\Program Files\qemu\qemu-system-%ARCH%w.exe"
if %ARCH%==aarch64 set QEMU="C:\Program Files\qemu\qemu-system-aarch64.exe"
if %ARCH%==riscv64 set QEMU="C:\Program Files\qemu\qemu-system-riscv64.exe"
if %ARCH%==x86_64 set QEMU="C:\Program Files\qemu\qemu-system-x86_64.exe"

REM Locate the architecture-specific kernel + initramfs produced by OSDK/python.
if %ARCH%==aarch64 (
  set KERNEL=%KEI%\target\osdk\aster-kernel\aster-kernel-osdk-bin.qemu_elf
  set INITRAMFS=%KEI%\tests\initramfs\build\initramfs_desktop_aarch64.cpio.gz
)
if %ARCH%==riscv64 (
  set KERNEL=%KEI%\target\osdk\riscv\aster-kernel-osdk-bin.qemu_elf
  set INITRAMFS=%KEI%\tests\initramfs\build\initramfs_desktop_riscv64.cpio.gz
)
if %ARCH%==x86_64 (
  set KERNEL=%KEI%\target\osdk\x86_64\aster-kernel-osdk-bin.qemu_elf
  set INITRAMFS=%KEI%\tests\initramfs\build\initramfs_desktop_x86_64.cpio.gz
)

if not exist %KERNEL% (
  echo [err] kernel not found: %KERNEL%
  echo       build it first via cargo osdk in WSL.
  exit /b 1
)
if not exist %INITRAMFS% (
  echo [err] initramfs not found: %INITRAMFS%
  echo       build it first: python scripts\build_desktop_initramfs.py %ARCH%
  exit /b 1
)

echo [%ARCH%] kernel:    %KERNEL%
echo [%ARCH%] initramfs: %INITRAMFS%
echo [%ARCH%] launching QEMU window: "%WINTITLE%"

if %ARCH%==aarch64 (
  %QEMU% ^
    -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on ^
    -m 2G -smp 1 --no-reboot ^
    -device virtio-gpu-device ^
    -device virtio-keyboard-device ^
    -device virtio-serial-device ^
    -serial file:%KEI%\target\serial_%ARCH%.log ^
    -netdev user,id=net0 -device virtio-net-device,netdev=net0 ^
    -kernel %KERNEL% ^
    -initrd %INITRAMFS% ^
    -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin" ^
    -display sdl -windowtitle "%WINTITLE%"
)

if %ARCH%==riscv64 (
  %QEMU% ^
    -cpu rv64 -machine virt ^
    -m 2G -smp 1 --no-reboot ^
    -device virtio-gpu-device ^
    -device virtio-keyboard-device ^
    -device virtio-serial-device ^
    -serial file:%KEI%\target\serial_%ARCH%.log ^
    -netdev user,id=net0 -device virtio-net-device,netdev=net0 ^
    -kernel %KERNEL% ^
    -initrd %INITRAMFS% ^
    -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin" ^
    -display sdl -windowtitle "%WINTITLE%"
)

if %ARCH%==x86_64 (
  %QEMU% ^
    -cpu qemu64 -machine q35 ^
    -m 2G -smp 1 --no-reboot ^
    -device virtio-gpu ^
    -device virtio-keyboard-pci ^
    -serial file:%KEI%\target\serial_%ARCH%.log ^
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 ^
    -kernel %KERNEL% ^
    -initrd %INITRAMFS% ^
    -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin console=ttyS0" ^
    -display sdl -windowtitle "%WINTITLE%"
)

endlocal
