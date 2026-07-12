#!/usr/bin/env python3
"""Build an aarch64 initramfs containing the aris-render kei_ui binary.

Produces a newc-format cpio.gz with:
  /init        — shell script that runs /kei_ui
  /kei_ui      — aris-render full UI binary (Blitz + Vello CPU)
  /bin/busybox — minimal shell + utilities
  /dev/console, /dev/null, /dev/tty, /dev/zero, /dev/urandom — device nodes

Usage: python3 build_render_initramfs.py [binary_name]
  binary_name defaults to 'kei_ui' (use 'kei_fbtest' for the test pattern)

The script reuses the cpio builder from tests/initramfs/build_aarch64_cpio.py.
"""
import os
import sys
import gzip
import shutil
import tempfile
import subprocess

KEI = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARIS = os.path.join(os.path.dirname(KEI), "aris")
BUILD_DIR = os.path.join(KEI, "tests", "initramfs", "build")
BUSYBOX = os.path.join(KEI, "tests", "initramfs", "busybox-aarch64")
sys.path.insert(0, os.path.join(KEI, "tests", "initramfs"))
from build_aarch64_cpio import build  # noqa: E402

INIT_TEMPLATE = """#!/bin/sh
mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
echo ""
echo "=== kei {bin_name} UI (aarch64) ==="
echo "Starting aris-render {bin_name} on /dev/fb0..."
/{bin_name} /dev/fb0
echo "{bin_name} exited with $?"
echo "Done. Keeping system alive."
while true; do sleep 10; done
"""


def main():
    bin_name = sys.argv[1] if len(sys.argv) > 1 else "kei_ui"
    bin_path = os.path.join(
        ARIS, "target", "aarch64-unknown-linux-musl", "release", bin_name
    )
    if not os.path.exists(bin_path):
        print(f"[err] binary not found: {bin_path}")
        print("      build it first: cd aris && cargo build --release "
              "--target aarch64-unknown-linux-musl --bin " + bin_name)
        sys.exit(1)

    print(f"[initramfs] binary: {bin_path} ({os.path.getsize(bin_path)} bytes)")

    with tempfile.TemporaryDirectory(prefix="kei-rootfs-") as rootfs:
        # Directory structure
        for d in ("bin", "dev", "proc", "sys", "tmp", "root", "etc"):
            os.makedirs(os.path.join(rootfs, d), exist_ok=True)

        # init script
        init_path = os.path.join(rootfs, "init")
        with open(init_path, "w", newline="\n") as f:
            f.write(INIT_TEMPLATE.format(bin_name=bin_name))
        os.chmod(init_path, 0o755)

        # The render binary
        shutil.copy2(bin_path, os.path.join(rootfs, bin_name))
        os.chmod(os.path.join(rootfs, bin_name), 0o755)

        # busybox + applet symlinks
        if os.path.exists(BUSYBOX):
            shutil.copy2(BUSYBOX, os.path.join(rootfs, "bin", "busybox"))
            os.chmod(os.path.join(rootfs, "bin", "busybox"), 0o755)
            for applet in ("sh", "echo", "cat", "ls", "mount", "sleep"):
                link = os.path.join(rootfs, "bin", applet)
                if not os.path.exists(link):
                    os.symlink("busybox", link)

        # passwd/group for musl
        with open(os.path.join(rootfs, "etc", "passwd"), "w") as f:
            f.write("root:x:0:0:root:/root:/bin/sh\n")
        with open(os.path.join(rootfs, "etc", "group"), "w") as f:
            f.write("root:x:0:\n")

        # Build cpio.gz
        out_name = f"initramfs_{bin_name}.cpio.gz"
        out_path = os.path.join(BUILD_DIR, out_name)
        build(rootfs, out_path)
        print(f"[initramfs] wrote {out_path} ({os.path.getsize(out_path)} bytes)")
        print(f"[initramfs] use with: -initrd tests/initramfs/build/{out_name}")


if __name__ == "__main__":
    main()
