#!/usr/bin/env python3
"""Build an aarch64 initramfs containing an aris-render binary.

Produces a newc-format cpio.gz with:
  /init        — the render binary itself (DIRECT_INIT) or a shell script
  /kei_tty     — aris-render vtty console binary (default)
  /bin/busybox — minimal shell + utilities
  /dev/console, /dev/null, /dev/tty, /dev/zero, /dev/urandom — device nodes

The aris repository location is resolved from (in priority order):
  1. The ARIS_REPO environment variable (set in .env, auto-loaded by just)
  2. A sibling ``aris`` directory next to kei (legacy default)

Usage: python3 build_render_initramfs.py [binary_name] [--build]
  binary_name defaults to 'kei_tty' (Linux-kernel-console-style vtty;
  use 'kei_ui' for the legacy runtime-rendered UI, 'kei_fbtest' for the
  test pattern)
  --build compiles the aris binary before packaging

The script reuses the cpio builder from tests/initramfs/build_aarch64_cpio.py.
"""
import os
import re
import sys
import gzip
import shutil
import tempfile
import subprocess

KEI = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Resolve the aris repo path: ARIS_REPO env var takes priority, then sibling fallback.
ARIS = os.environ.get("ARIS_REPO") or os.path.join(os.path.dirname(KEI), "aris")
BUILD_DIR = os.path.join(KEI, "tests", "initramfs", "build")
BUSYBOX = os.path.join(KEI, "tests", "initramfs", "busybox-aarch64")
sys.path.insert(0, os.path.join(KEI, "tests", "initramfs"))
from build_aarch64_cpio import build  # noqa: E402

# The musl target triple for aarch64 cross-compilation.
TARGET_TRIPLE = "aarch64-unknown-linux-musl"

# Per-binary cargo feature set. Every kei-target binary is built with
# --no-default-features so desktop-only deps (winit/reqwest/fontconfig)
# never enter the musl cross build.
BIN_FEATURES = {
    "kei_tty": "png",            # vtty: embedded pre-rendered console PNG
    "kei_ui": "render",          # legacy runtime Blitz renderer
    "kei_fbtest": "",            # pure libc pixel test
    "kei_minimal": "",           # hello-world syscall test
}

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

# When DIRECT_INIT is set, /init is a copy of the render binary itself (not a
# shell script). This avoids depending on busybox/sh, which can crash on kei
# due to TLS/musl-runtime issues. The kernel execve's /init directly.
DIRECT_INIT = True


def _build_aris_binary(bin_name: str) -> str:
    """Cross-compile the aris-render binary for aarch64 musl.

    Runs ``cargo build --release --target aarch64-unknown-linux-musl --bin <name>``
    inside the aris repo (at ARIS). On Windows this re-execs through WSL.
    """
    features = BIN_FEATURES.get(bin_name, "")
    feat_args = f'--no-default-features --features "{features}"' if features else "--no-default-features"
    print(f"[build-aris] compiling {bin_name} for {TARGET_TRIPLE} in {ARIS} ({feat_args})")
    if not os.path.isdir(ARIS):
        print(f"[err] aris repo not found at {ARIS}")
        print("      Set ARIS_REPO in .env (see .env.example).")
        sys.exit(1)

    is_windows = sys.platform == "win32"
    if is_windows:
        aris_wsl = ARIS.replace("\\", "/")
        aris_wsl = re.sub(r"^([A-Za-z]):", lambda m: f"/mnt/{m.group(1).lower()}", aris_wsl)
        cmd = [
            "wsl", "-d", "Ubuntu-24.04", "--", "bash", "-lc",
            f'cd "{aris_wsl}" && source ~/.cargo/env 2>/dev/null && '
            f'cargo build --release --target {TARGET_TRIPLE} --bin {bin_name} {feat_args}'
        ]
    else:
        cmd = [
            "cargo", "build", "--release",
            "--target", TARGET_TRIPLE, "--bin", bin_name,
            *feat_args.replace('"', '').split(),
        ]

    result = subprocess.run(cmd, capture_output=True, text=True, timeout=600,
                            cwd=ARIS if not is_windows else None)
    if result.returncode != 0:
        print(f"[build-aris] FAILED (exit {result.returncode})")
        print(result.stdout[-2000:] if result.stdout else "")
        print(result.stderr[-2000:] if result.stderr else "")
        sys.exit(1)

    bin_path = os.path.join(ARIS, "target", TARGET_TRIPLE, "release", bin_name)
    if not os.path.exists(bin_path):
        print(f"[err] binary not found after build: {bin_path}")
        sys.exit(1)
    print(f"[build-aris] OK: {bin_path} ({os.path.getsize(bin_path)} bytes)")
    return bin_path


def main():
    args = sys.argv[1:]
    do_build = "--build" in args
    args = [a for a in args if a != "--build"]
    bin_name = args[0] if args else "kei_tty"

    bin_path = os.path.join(ARIS, "target", TARGET_TRIPLE, "release", bin_name)

    # Compile aris if --build was requested or binary is missing.
    if do_build or not os.path.exists(bin_path):
        bin_path = _build_aris_binary(bin_name)

    if not os.path.exists(bin_path):
        print(f"[err] binary not found: {bin_path}")
        print("      build it first: cd aris && cargo build --release "
              f"--target {TARGET_TRIPLE} --bin {bin_name}")
        sys.exit(1)

    print(f"[initramfs] binary: {bin_path} ({os.path.getsize(bin_path)} bytes)")

    with tempfile.TemporaryDirectory(prefix="kei-rootfs-") as rootfs:
        # Directory structure
        for d in ("bin", "dev", "proc", "sys", "tmp", "root", "etc"):
            os.makedirs(os.path.join(rootfs, d), exist_ok=True)

        # init: either the render binary directly (DIRECT_INIT) or a shell script.
        # Direct init avoids the busybox/sh dependency, which crashes on kei
        # due to musl runtime TLS issues with the larger busybox binary.
        init_path = os.path.join(rootfs, "init")
        if DIRECT_INIT:
            # /init IS the render binary — kernel execve's it directly.
            shutil.copy2(bin_path, init_path)
            os.chmod(init_path, 0o755)
            print(f"[initramfs] DIRECT_INIT: /init = {bin_name} ELF")
        else:
            with open(init_path, "w", newline="\n") as f:
                f.write(INIT_TEMPLATE.format(bin_name=bin_name))
            os.chmod(init_path, 0o755)

            # The render binary (only needed when init is a script)
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
        os.makedirs(BUILD_DIR, exist_ok=True)
        out_path = os.path.join(BUILD_DIR, out_name)
        build(rootfs, out_path)
        print(f"[initramfs] wrote {out_path} ({os.path.getsize(out_path)} bytes)")
        print(f"[initramfs] use with: -initrd tests/initramfs/build/{out_name}")


if __name__ == "__main__":
    main()
