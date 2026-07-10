#!/usr/bin/env python3
"""kei — enter build environment shell.

Usage:
    python3 scripts/dev_shell.py [command...]
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    env = os.environ.copy()
    tools_bin = str(PROJECT_ROOT / "vendor" / "asterinas" / "osdk" / "target" / "release")
    env["PATH"] = f"{tools_bin}:{env.get('PATH', '')}"
    env["KEI_PATCHES"] = str(PROJECT_ROOT / "patches" / "arm64")
    env["KEI_BSP"] = str(PROJECT_ROOT / "bsp")
    env["KEI_BOARD"] = str(PROJECT_ROOT / "board")

    cf.section("kei dev shell")
    for key in ("KEI_PATCHES", "KEI_BSP", "KEI_BOARD"):
        cf.info(f"  {key}={env[key]}")
    cf.blank()

    cmd = sys.argv[1:] if len(sys.argv) > 1 else [env.get("SHELL", "bash")]
    result = subprocess.run(cmd, env=env)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
