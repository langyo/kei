#!/usr/bin/env python3
"""kei — create initramfs for kernel build and boot.

The initramfs is required by `cargo osdk build` (referenced in OSDK.toml).
Asterinas's own initramfs uses Nix (heavyweight); this script creates a
lightweight, reproducible alternative.

VDSO note:
  The vDSO module is cfg-gated to x86_64 and riscv64 only
  (kernel/src/lib.rs: #[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))] mod vdso).
  For aarch64, the entire module is excluded — no prebuilt .so files needed.
  The Makefile's unconditional `check_vdso` target is an upstream design issue
  we sidestep by creating the initramfs directly.

Usage:
    python3 scripts/initramfs.py [--arch aarch64] [--force]
"""
from __future__ import annotations

import gzip
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import build_env
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent
INITRAMFS_BUILD_DIR = PROJECT_ROOT / "tests" / "initramfs" / "build"
INITRAMFS_GZ = INITRAMFS_BUILD_DIR / "initramfs.cpio.gz"


def _inject_cpio_devnode(cpio_data: bytes, name: str, major: int, minor: int) -> bytes:
    """Inject a character device node into a newc-format cpio archive.

    Used when running as non-root (mknod requires CAP_MKNOD).
    Adds the device node entry before the trailer.
    """
    mode = 0o620 | 0o20000  # S_IFCHR | rw-rw----
    ino = abs(hash(name)) & 0xFFFFFFFF

    def h(v):
        return f"{v:08x}".encode()

    hdr = b"070701" + h(ino) + h(mode) + h(0) + h(0) + h(1) + h(0) + h(0) + h(0) + h(0) + h(major) + h(minor) + h(len(name) + 1) + h(0)
    name_bytes = name.encode() + b"\x00"
    name_pad = (4 - (len(hdr) + len(name_bytes)) % 4) % 4
    entry = hdr + name_bytes + b"\x00" * name_pad

    # Insert before the trailer entry
    trailer_name = b"TRAILER!!!\x00"
    insert_pos = cpio_data.rfind(trailer_name)
    if insert_pos > 0:
        insert_pos -= 110  # back up past trailer's 110-byte header
    else:
        insert_pos = len(cpio_data)

    result = cpio_data[:insert_pos] + entry + cpio_data[insert_pos:]
    cf.ok(f"injected /dev/{name} (char {major}:{minor}) into cpio archive")
    return result


# Init script — runs as PID 1 inside the booted kernel.
INIT_SCRIPT = """#!/bin/sh

mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
mount -t devtmpfs none /dev 2>/dev/null

echo ""
echo "=== kei ignition ==="
echo "Kernel booted successfully"
echo ""

# Write boot banner to framebuffer if available
if [ -w /dev/fb0 ]; then
    printf "\\033[2J\\033[H" > /dev/fb0 2>/dev/null
    echo "=== kei OS ===" > /dev/fb0 2>/dev/null
    echo "NanoPi R3S" > /dev/fb0 2>/dev/null
    echo "" > /dev/fb0 2>/dev/null
    echo "Kernel booted successfully" > /dev/fb0 2>/dev/null
fi

# Detect network interfaces (the ignition test checks for these)
echo "Network interfaces:"
for iface in /sys/class/net/*; do
    [ -d "$iface" ] || continue
    name=$(basename "$iface")
    mac=$(cat "$iface/address" 2>/dev/null || echo "??:??:??:??:??:??")
    echo "  $name  mac=$mac"
done
echo ""

# Bring up loopback + all ethernet interfaces
if command -v ip >/dev/null 2>&1; then
    ip link set lo up 2>/dev/null
    for iface in /sys/class/net/*; do
        name=$(basename "$iface")
        [ "$name" = "lo" ] && continue
        ip link set "$name" up 2>/dev/null
        echo "  brought up $name"
    done
fi

echo ""
echo "Boot complete."
exec /bin/sh
"""


def find_busybox(arch: str) -> Path | None:
    """Find a busybox binary for the target architecture.

    Search order:
      1. ARCH_BUSYBOX env var (explicit override)
      2. tests/initramfs/busybox-<arch> (pre-built per-arch binary)
      3. Host busybox (only when arch matches the host)
    """
    # 1. Explicit override
    env_path = os.environ.get("ARCH_BUSYBOX")
    if env_path and Path(env_path).exists():
        return Path(env_path)

    # 2. Pre-built per-arch binary
    prebuilt = PROJECT_ROOT / "tests" / "initramfs" / f"busybox-{arch}"
    if prebuilt.exists():
        return prebuilt

    # 3. Host busybox (only if arch matches host)
    host_arch = build_env.host_machine()
    if host_arch == arch or (host_arch == "x86_64" and arch == "x86_64"):
        bb = shutil.which("busybox")
        if bb:
            return Path(bb)

    return None


def create_initramfs(arch: str, force: bool = False) -> Path:
    """Create a minimal initramfs.cpio.gz for kernel boot."""
    if INITRAMFS_GZ.exists() and not force:
        cf.ok(f"initramfs exists ({INITRAMFS_GZ.stat().st_size} bytes)")
        return INITRAMFS_GZ

    INITRAMFS_BUILD_DIR.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="kei-initramfs-") as tmp:
        root = Path(tmp)

        # Directory structure
        for d in ("bin", "dev", "proc", "sys", "etc", "tmp", "run"):
            (root / d).mkdir(parents=True, exist_ok=True)

        # Create /dev/console (char major 5, minor 1) — required by the kernel
        # to connect stdin/stdout/stderr for the init process.
        # Linux creates this before starting init; without it, all user-space
        # writes to stdout/stderr silently fail (EBADF).
        os.makedirs(root / "dev", exist_ok=True)
        try:
            os.mknod(root / "dev" / "console", 0o620 | stat.S_IFCHR, os.makedev(5, 1))
        except (PermissionError, OSError):
            # mknod requires root; device node will be injected into cpio later.
            pass

        # Init script (PID 1)
        init = root / "init"
        init.write_text(INIT_SCRIPT)
        init.chmod(0o755)

        # Include busybox if available (for shell + network tools)
        busybox = find_busybox(arch)
        if busybox:
            shutil.copy2(busybox, root / "bin" / "busybox")
            for applet in ("sh", "ls", "cat", "ip", "mount", "echo",
                           "sleep", "ifconfig", "udhcpc", "ping"):
                link = root / "bin" / applet
                if not link.exists():
                    link.symlink_to("busybox")
            cf.ok("busybox included")
        else:
            cf.warn("busybox not found — minimal shell only")

        # Build cpio.gz
        cf.pending("creating cpio archive...")
        result = subprocess.run(
            ["sh", "-c", "find . | cpio -H newc -o"],
            cwd=root,
            capture_output=True,
        )
        if result.returncode != 0 or not result.stdout:
            cf.fail("cpio creation failed")
            cf.info(result.stderr.decode("utf-8", errors="replace"))
            return INITRAMFS_GZ

        cpio_data = result.stdout

        # If /dev/console doesn't exist or isn't a char device, inject it
        # directly into the cpio archive as a binary device node entry.
        console_path = root / "dev" / "console"
        need_inject = True
        if console_path.exists():
            try:
                need_inject = not stat.S_ISCHR(console_path.stat().st_mode)
            except OSError:
                need_inject = True
        if need_inject:
            cpio_data = _inject_cpio_devnode(cpio_data, "dev/console", 5, 1)

        with gzip.open(INITRAMFS_GZ, "wb") as f:
            f.write(cpio_data)

    size = INITRAMFS_GZ.stat().st_size
    cf.ok(f"initramfs created: {INITRAMFS_GZ.name} ({size} bytes)")
    return INITRAMFS_GZ


def main() -> int:
    if build_env.wsl_main_guard():
        return 0
    import argparse

    parser = argparse.ArgumentParser(description="Create initramfs for kei")
    parser.add_argument("--arch", default="aarch64")
    parser.add_argument("--force", action="store_true",
                        help="Rebuild even if initramfs exists")
    args = parser.parse_args()

    cf.section(f"kei initramfs ({args.arch})")
    create_initramfs(args.arch, args.force)
    cf.blank()
    cf.ok("Ready for: cargo osdk build --target-arch " + args.arch)
    return 0


if __name__ == "__main__":
    sys.exit(main())
