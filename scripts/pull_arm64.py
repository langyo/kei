#!/usr/bin/env python3
"""kei — pull ARM64 architecture code from wanywhn/asterinas.

This is a ONE-TIME (or rare re-sync) operation. After pulling, the arm64
code is maintained independently in kei.

Usage:
    python3 scripts/pull_arm64.py                # pull latest arm64-support
    python3 scripts/pull_arm64.py <commit>       # pull specific commit
"""
from __future__ import annotations

import datetime
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent
ARM64_URL = "https://github.com/wanywhn/asterinas.git"


def git(*args: str, capture: bool = False) -> subprocess.CompletedProcess:
    from proxy import get_proxy_env
    import os
    env = {**os.environ.copy(), **get_proxy_env()}
    return subprocess.run(
        ["git", *args],
        cwd=PROJECT_ROOT,
        capture_output=capture,
        text=True,
        env=env,
    )


def extract_path(ref: str, path: str) -> bool:
    result = git("checkout", ref, "--", path, capture=True)
    if result.returncode == 0:
        return True
    # Fallback: git archive
    result = git("archive", ref, path, capture=True)
    if result.returncode == 0:
        import tarfile
        import io
        with tarfile.open(fileobj=io.BytesIO(result.stdout.encode())) as tar:
            tar.extractall(PROJECT_ROOT)
        return True
    return False


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Pull ARM64 code from wanywhn fork")
    parser.add_argument("ref", nargs="?", default="arm64-support",
                        help="Branch/commit (default: arm64-support)")
    args = parser.parse_args()

    arm64_ref = args.ref

    cf.section("kei: pull ARM64 architecture code")

    # Ensure remote
    result = git("remote", "get-url", "arm64", capture=True)
    if result.returncode != 0:
        git("remote", "add", "arm64", ARM64_URL)

    cf.blank()
    cf.step(f"[1/3] Fetching wanywhn/asterinas ({arm64_ref})")
    result = git("fetch", "arm64", arm64_ref)
    if result.returncode != 0:
        cf.fail(f"Failed to fetch arm64/{arm64_ref}")
        return 1
    arm64_sha = git("rev-parse", "--short", f"arm64/{arm64_ref}", capture=True).stdout.strip()
    cf.ok(f"arm64/{arm64_ref} = {arm64_sha}")

    cf.blank()
    cf.step("[2/3] Extracting ARM64 architecture code")
    targets = [
        "ostd/src/arch/aarch64",
        "kernel/src/arch/aarch64",
    ]
    for path in targets:
        full = PROJECT_ROOT / path
        full.parent.mkdir(parents=True, exist_ok=True)
        if extract_path(f"arm64/{arm64_ref}", path):
            cf.ok(f"  {path}/ extracted")
        else:
            cf.warn(f"  {path}/ not found")

    cf.blank()
    cf.step("[3/3] Recording ARM64 source version")
    full_sha = git("rev-parse", f"arm64/{arm64_ref}", capture=True).stdout.strip()
    date_result = git("log", "-1", "--format=%ci", f"arm64/{arm64_ref}", capture=True)
    arm64_date = date_result.stdout.strip() or "unknown"
    version_file = PROJECT_ROOT / ".vendored-arm64"
    version_file.write_text(
        f"arm64_url={ARM64_URL}\n"
        f"arm64_ref={arm64_ref}\n"
        f"arm64_sha={full_sha}\n"
        f"arm64_date={arm64_date}\n"
        f"pulled_date={datetime.datetime.now(datetime.timezone.utc).isoformat()}\n"
        f"note=Point-in-time snapshot. Maintained independently in kei thereafter.\n"
    )
    cf.ok("Version recorded")

    cf.blank()
    cf.ok(f"ARM64 pull complete: {arm64_sha}")
    cf.info("  The arm64 code is now part of kei, maintained independently.")
    cf.info("  Audit: grep -rn 'TODO|FIXME|HACK' ostd/src/arch/aarch64/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
