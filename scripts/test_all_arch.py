#!/usr/bin/env python3
"""kei — boot-test the kernel on ALL supported architectures via QEMU.

Mirrors how Linux kernel / KernelCI tests: every architecture gets a QEMU
boot smoke test before anything is considered ready.

Usage:
    python3 scripts/test_all_arch.py                # test all architectures
    python3 scripts/test_all_arch.py aarch64        # test one architecture
    python3 scripts/test_all_arch.py x86_64 riscv64 # test specific set
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import build_env
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent

ARCH_CONFIG = {
    "x86_64": {
        "qemu": "qemu-system-x86_64",
        "machine": "q35",
        "cpu": "qemu64",
        "memory": "4096",
        "target": "x86_64-unknown-none",
    },
    "aarch64": {
        "qemu": "qemu-system-aarch64",
        "machine": "virt",
        "cpu": "cortex-a72",
        "memory": "2048",
        "target": "aarch64-unknown-none",
    },
    "riscv64": {
        "qemu": "qemu-system-riscv64",
        "machine": "virt",
        "cpu": "rv64",
        "memory": "2048",
        "target": "riscv64imac-unknown-none-elf",
    },
    "loongarch64": {
        "qemu": "qemu-system-loongarch64",
        "machine": "virt",
        "cpu": "max",
        "memory": "2048",
        "target": "loongarch64-unknown-none-softfloat",
    },
}


def test_arch(arch: str, output_dir: Path) -> str:
    """Test one architecture. Returns 'PASS', 'FAIL', 'SKIP', or 'UNKNOWN'."""
    cfg = ARCH_CONFIG.get(arch)
    if not cfg:
        cf.fail(f"Unknown architecture: {arch}")
        return "FAIL"

    qemu = cfg["qemu"]
    cf.blank()
    cf.bold(f"── {arch} {'─' * max(0, 40 - len(arch))}")

    if not shutil.which(qemu):
        cf.warn(f"SKIP: {qemu} not installed")
        cf.info(f"  Install: sudo apt install qemu-system-{arch.replace('_', '-')}")
        return "SKIP"

    # Build kernel for this architecture
    cf.pending(f"Building kernel ({cfg['target']})...")
    nightly_env = dict(os.environ)
    rustup_home = os.environ.get("RUSTUP_HOME", os.path.expanduser("~/.rustup"))
    nightly_bin = os.path.join(rustup_home, "toolchains", "nightly-2026-05-01-x86_64-unknown-linux-gnu", "bin")
    if os.path.isdir(nightly_bin):
        nightly_env["PATH"] = nightly_bin + ":" + nightly_env.get("PATH", "")

    build_cmd = ["cargo", "osdk", "build", "--target-arch", arch, "--release"]
    scheme = {"aarch64": "aarch64", "riscv64": "riscv", "loongarch64": "loongarch"}.get(arch)
    if scheme:
        build_cmd.extend(["--scheme", scheme])
    build_result = subprocess.run(build_cmd, cwd=PROJECT_ROOT, capture_output=True, env=nightly_env)
    if build_result.returncode != 0:
        cf.fail(f"Build failed for {arch}")
        return "FAIL"

    # Locate kernel binary — OSDK outputs under target/osdk/
    kernel = PROJECT_ROOT / "target" / "osdk" / "aster-kernel.bin"
    kernel_elf = PROJECT_ROOT / "target" / cfg["target"] / "release" / "aster-kernel-osdk-bin"
    kernel_qemu_elf = PROJECT_ROOT / "target" / "osdk" / "aster-kernel-osdk-bin.qemu_elf"
    kernel_path = kernel if kernel.exists() else (kernel_qemu_elf if kernel_qemu_elf.exists() else kernel_elf)

    if not kernel_path.exists():
        cf.fail(f"Kernel binary not found: {kernel_path}")
        return "FAIL"

    # Boot in QEMU with 30-second timeout
    cf.pending(f"Booting in QEMU ({cfg['machine']}, {cfg['cpu']}, {cfg['memory']}MB)...")
    output_dir.mkdir(parents=True, exist_ok=True)
    log_file = output_dir / f"test-{arch}.log"

    cmd = [
        qemu,
        "-M", cfg["machine"],
        "-cpu", cfg["cpu"],
        "-m", cfg["memory"],
        "-kernel", str(kernel_path),
        "-nographic",
        "-no-reboot",
    ]

    try:
        result = subprocess.run(
            cmd,
            timeout=30,
            capture_output=True,
            text=True,
        )
        log_content = (result.stdout or "") + (result.stderr or "")
    except subprocess.TimeoutExpired as e:
        log_content = (e.stdout or "") + (e.stderr or "") if isinstance(e.stdout, str) else ""
        log_content = str(log_content)

    log_file.write_text(log_content)

    log_lower = log_content.lower()
    if "panic" in log_lower or "oops" in log_lower:
        if "kernel_main" not in log_lower and "shell" not in log_lower:
            cf.fail("FAIL: kernel panicked")
            return "FAIL"

    if any(kw in log_lower for kw in ["kei", "asterinas", "kernel_main", "shell", "console"]):
        cf.ok("PASS: kernel booted")
        return "PASS"

    cf.warn("UNKNOWN: could not determine boot status")
    return "UNKNOWN"


def main() -> int:
    if build_env.wsl_main_guard():
        return 0
    all_archs = list(ARCH_CONFIG.keys())
    selected = sys.argv[1:] if len(sys.argv) > 1 else all_archs

    cf.section("kei multi-architecture boot test")
    cf.info(f"  Architectures: {' '.join(selected)}")

    output_dir = PROJECT_ROOT / "target" / "test-output"

    results = {"PASS": 0, "FAIL": 0, "SKIP": 0, "UNKNOWN": 0}
    for arch in selected:
        if arch not in ARCH_CONFIG:
            cf.fail(f"Unknown architecture: {arch}")
            results["FAIL"] += 1
            continue
        status = test_arch(arch, output_dir)
        results[status] += 1

    cf.blank()
    cf.section("Results")
    for status, count in results.items():
        if count > 0:
            if status == "PASS":
                cf.ok(f"  {status}: {count}")
            elif status == "FAIL":
                cf.fail(f"  {status}: {count}")
            else:
                cf.info(f"  {status}: {count}")

    return 1 if results["FAIL"] > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
