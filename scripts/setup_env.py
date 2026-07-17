#!/usr/bin/env python3
"""kei — prepare host environment for QEMU kernel testing.

Interactively prompts for sudo password if needed, then installs:
  - qemu-system-aarch64 (QEMU arm64 emulator)
  - device-tree-compiler (dtc, for FDT decompilation)
  - gcc-aarch64-linux-gnu (aarch64 cross-compiler for C deps)

Usage:
    python3 scripts/setup_env.py
"""
from __future__ import annotations

import getpass
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import cli_format as cf


REQUIRED_TOOLS = {
    "qemu-system-aarch64": "qemu-system-arm",
    "dtc": "device-tree-compiler",
    "aarch64-linux-gnu-gcc": "gcc-aarch64-linux-gnu",
    "cargo-osdk": None,  # installed via cargo, not apt
}


def check_tools() -> dict[str, bool]:
    """Check which required tools are already available."""
    status = {}
    for tool, _ in REQUIRED_TOOLS.items():
        status[tool] = shutil.which(tool) is not None
    return status


def install_apt_packages(packages: list[str], sudo_pass: str) -> bool:
    """Install packages via apt-get using sudo."""
    try:
        # Fix interrupted dpkg first
        subprocess.run(
            ["sudo", "-S", "dpkg", "--configure", "-a"],
            input=sudo_pass + "\n",
            capture_output=True,
            text=True,
            timeout=60,
        )
    except Exception:
        pass

    cmd = ["sudo", "-S", "apt-get", "install", "-y", "-qq"] + packages
    result = subprocess.run(
        cmd,
        input=sudo_pass + "\n",
        capture_output=True,
        text=True,
        timeout=300,
    )
    if result.returncode != 0:
        cf.fail(f"apt-get install failed: {result.stderr.strip()[:200]}")
        return False
    return True


def install_cargo_osdk() -> bool:
    """Install cargo-osdk from local source."""
    project_root = Path(__file__).resolve().parent.parent
    osdk_dir = project_root / "osdk"
    if not osdk_dir.exists():
        cf.warn("packages/osdk/ directory not found, skipping cargo-osdk install")
        return False

    nightly_bin = Path.home() / ".rustup" / "toolchains" / "nightly-2026-05-01-x86_64-unknown-linux-gnu" / "bin"
    env = dict(os.environ)
    if nightly_bin.exists():
        env["PATH"] = str(nightly_bin) + ":" + env.get("PATH", "")

    result = subprocess.run(
        ["cargo", "install", "--path", str(osdk_dir), "--force"],
        env=env,
        capture_output=True,
        text=True,
        timeout=300,
    )
    return result.returncode == 0


def main() -> int:
    cf.section("kei — Host Environment Setup")

    status = check_tools()
    cf.blank()
    cf.step("Current tool availability:")
    for tool, ok in status.items():
        marker = "[  OK  ]" if ok else "[MISSING]"
        print(f"  {marker}  {tool}")

    missing_apt = [
        pkg for tool, pkg in REQUIRED_TOOLS.items()
        if pkg and not status.get(tool)
    ]
    need_osdk = not status.get("cargo-osdk", False)

    if not missing_apt and not need_osdk:
        cf.blank()
        cf.ok("All tools already installed!")
        return 0

    cf.blank()
    cf.step("Installing missing tools...")

    if missing_apt:
        sudo_pass = getpass.getpass("  Enter sudo password (for apt-get): ")
        cf.info(f"  Installing: {', '.join(missing_apt)}")
        if not install_apt_packages(missing_apt, sudo_pass):
            return 1

    if need_osdk:
        cf.info("  Installing cargo-osdk from local source...")
        if not install_cargo_osdk():
            cf.warn("  cargo-osdk install failed (may already be installed)")

    cf.blank()
    cf.step("Verification:")
    status = check_tools()
    all_ok = True
    for tool, ok in status.items():
        marker = "[  OK  ]" if ok else "[FAIL  ]"
        print(f"  {marker}  {tool}")
        if not ok:
            all_ok = False

    cf.blank()
    if all_ok:
        cf.ok("Environment ready! Run: python3 scripts/build.py nanopi-r3s")
    else:
        cf.fail("Some tools missing — check errors above")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
