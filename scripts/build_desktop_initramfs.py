#!/usr/bin/env python3
"""Build per-architecture initramfs containing the kei_desktop binary.

For each target architecture, produces a newc-format cpio.gz where:
  /init  = the kei_desktop ELF (kernel execve's it directly, DIRECT_INIT)
  /dev, /proc, /sys, /tmp, /etc dirs exist

No busybox/sh is required: kei_desktop is the init and writes to /dev/fb0
directly. This avoids the musl/TLS crashes that busybox triggers on kei.

Usage:
    python3 build_desktop_initramfs.py aarch64
    python3 build_desktop_initramfs.py riscv64
    python3 build_desktop_initramfs.py x86_64
    python3 build_desktop_initramfs.py all      # build all three
"""
import os
import sys
import gzip
import shutil
import tempfile

KEI = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARIS = os.path.join(os.path.dirname(KEI), "aris")
BUILD_DIR = os.path.join(KEI, "tests", "initramfs", "build")
sys.path.insert(0, os.path.join(KEI, "tests", "initramfs"))
from build_aarch64_cpio import build  # noqa: E402

# Map architecture -> (aris cargo target triple, output suffix).
ARCHES = {
    "aarch64": "aarch64-unknown-linux-musl",
    "riscv64": "riscv64gc-unknown-linux-musl",
    "x86_64": "x86_64-unknown-linux-musl",
}


def build_one(arch: str) -> str:
    triple = ARCHES[arch]
    bin_path = os.path.join(ARIS, "target", triple, "release", "kei_desktop")

    with tempfile.TemporaryDirectory(prefix=f"kei-rootfs-{arch}-") as rootfs:
        for d in ("bin", "dev", "proc", "sys", "tmp", "root", "etc"):
            os.makedirs(os.path.join(rootfs, d), exist_ok=True)

        # The kernel draws the desktop at boot (in the virtio-gpu probe), so
        # the init process does NOT need to write /dev/fb0. We provide a tiny
        # static init that just sleeps forever (keeping PID 1 alive so the
        # kernel doesn't panic). This avoids the slow/crash-prone fb write_at
        # path entirely.
        init_src = os.path.join(os.path.dirname(__file__), "init_idle.c")
        if os.path.exists(init_src):
            # Cross-compile the idle init for the target arch.
            idle_init = _compile_idle_init(init_src, arch, rootfs)
        else:
            # Fallback: use kei_desktop if present (it will try to write fb0,
            # which is slow but harmless since the kernel already drew the frame).
            if not os.path.exists(bin_path):
                print(f"[{arch}] WARN: neither init_idle.c nor kei_desktop found; "
                      "creating a dummy init")
                idle_init = os.path.join(rootfs, "init")
                with open(idle_init, "w") as f:
                    f.write("#!/bin/sh\nwhile true; do sleep 3600; done\n")
                os.chmod(idle_init, 0o755)
            else:
                idle_init = bin_path

        init_path = os.path.join(rootfs, "init")
        shutil.copy2(idle_init, init_path)
        os.chmod(init_path, 0o755)
        print(f"[{arch}] /init = idle loop (kernel drew desktop at boot)")

        # minimal /etc/passwd + group
        with open(os.path.join(rootfs, "etc", "passwd"), "w") as f:
            f.write("root:x:0:0:root:/root:/bin/sh\n")
        with open(os.path.join(rootfs, "etc", "group"), "w") as f:
            f.write("root:x:0:\n")

        out_name = f"initramfs_desktop_{arch}.cpio.gz"
        out_path = os.path.join(BUILD_DIR, out_name)
        build(rootfs, out_path)
        print(f"[{arch}] wrote {out_path} ({os.path.getsize(out_path)} bytes)")
        return out_path


def _compile_idle_init(src: str, arch: str, rootfs: str) -> str:
    """Cross-compile the idle init C source for the target arch."""
    import subprocess
    # Use the musl cross-compiler if available, else gcc.
    cc_map = {
        "aarch64": "aarch64-linux-gnu-gcc",
        "riscv64": "riscv64-linux-gnu-gcc",
        "x86_64": "x86_64-linux-gnu-gcc",
    }
    cc = cc_map.get(arch, "gcc")
    out = os.path.join(rootfs, "init_idle")
    try:
        subprocess.run(
            [cc, "-static", "-O2", "-s", "-o", out, src],
            check=True, capture_output=True, timeout=30,
        )
        return out
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        # Fallback: try the musl.cc toolchain or host gcc.
        for alt_cc in [f"{arch}-linux-musl-gcc", "gcc"]:
            try:
                subprocess.run(
                    [alt_cc, "-static", "-O2", "-s", "-o", out, src],
                    check=True, capture_output=True, timeout=30,
                )
                return out
            except Exception:
                continue
        raise RuntimeError(f"could not compile init_idle.c for {arch}")


def main():
    os.makedirs(BUILD_DIR, exist_ok=True)
    args = sys.argv[1:] or ["all"]
    if "all" in args:
        args = list(ARCHES.keys())
    ok = []
    for arch in args:
        if arch not in ARCHES:
            print(f"[err] unknown arch: {arch}; choices: {list(ARCHES)}")
            continue
        out = build_one(arch)
        if out:
            ok.append(out)
    print("---")
    print(f"built {len(ok)} initramfs image(s):")
    for o in ok:
        print(f"  {o}")


if __name__ == "__main__":
    main()
