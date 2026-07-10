#!/usr/bin/env python3
"""kei — initial setup: configure git remotes for the vendoring workflow.

Usage:
    python3 scripts/setup.py
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent

UPSTREAM_URL = "https://github.com/asterinas/asterinas.git"
ARM64_URL = "https://github.com/wanywhn/asterinas.git"


def ensure_remote(name: str, url: str) -> None:
    result = subprocess.run(
        ["git", "remote", "get-url", name],
        capture_output=True, cwd=PROJECT_ROOT,
    )
    if result.returncode == 0:
        cf.info(f"  Exists: {name}")
    else:
        subprocess.run(["git", "remote", "add", name, url], check=True, cwd=PROJECT_ROOT)
        cf.ok(f"  Added: {name} → {url.split('/')[-1]}")


def check_dir(path: Path, label: str, hint: str) -> None:
    if path.exists():
        cf.ok(f"  {label} present")
    else:
        cf.warn(f"  {label} missing — {hint}")


def main() -> int:
    cf.section("kei setup")

    cf.blank()
    cf.step("[1/2] Configuring git remotes")
    ensure_remote("upstream", UPSTREAM_URL)
    ensure_remote("arm64", ARM64_URL)

    cf.blank()
    cf.step("[2/2] Status check")
    check_dir(
        PROJECT_ROOT / "ostd",
        "ostd/",
        "run 'just vendor' to absorb upstream",
    )
    check_dir(
        PROJECT_ROOT / "ostd" / "src" / "arch" / "aarch64",
        "ostd/src/arch/aarch64/",
        "run 'just pull-arm64'",
    )

    cf.blank()
    cf.ok("Setup complete")
    cf.info("  Populate:  just vendor && just pull-arm64")
    cf.info("  Build:     just build")
    cf.info("  Test:      just test-all")
    return 0


if __name__ == "__main__":
    sys.exit(main())
