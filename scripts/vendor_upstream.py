#!/usr/bin/env python3
"""kei — vendor upstream asterinas via squash/directory-level replacement.

This implements the "Apple LLVM" model: kei is an independent fork that
periodically absorbs upstream changes as a single replacement operation.

Usage:
    python3 scripts/vendor_upstream.py              # vendor latest upstream main
    python3 scripts/vendor_upstream.py <commit>     # vendor specific commit/tag
"""
from __future__ import annotations

import datetime
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent
UPSTREAM_URL = "https://github.com/asterinas/asterinas.git"

# Paths that are OURS — preserved across vendoring
OUR_PATHS = [
    "ostd/src/arch/aarch64",
    "kernel/src/arch/aarch64",
    "bsp",
    "board",
    "configs",
    "scripts",
    "docs",
    ".github",
    ".gitignore",
    ".editorconfig",
    "justfile",
    "PLAN.md",
    "README.md",
    "rust-toolchain.toml",
    "clippy.toml",
    "deny.toml",
    "Cargo.toml",  # Merged: upstream base + kei bsp/ members
]

# Directories refreshed from upstream on each vendor
UPSTREAM_DIRS = ["ostd", "kernel", "osdk", "test", "tools"]

# Root-level files tracked from upstream (Cargo.toml gets merged, not replaced)
UPSTREAM_FILES = ["Cargo.lock", "Makefile", "OSDK.toml",
                  "Components.toml", "VERSION"]


def git(*args: str, **kw) -> subprocess.CompletedProcess:
    from proxy import get_proxy_env
    env = {**os.environ.copy(), **get_proxy_env()}
    return subprocess.run(
        ["git", *args],
        cwd=PROJECT_ROOT,
        capture_output=kw.get("capture", False),
        text=True,
        env=env,
    )


def git_check(*args: str) -> bool:
    return git(*args, capture=True).returncode == 0


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Vendor upstream asterinas")
    parser.add_argument("ref", nargs="?", default="main",
                        help="Upstream commit/tag/branch (default: main)")
    args = parser.parse_args()

    upstream_ref = args.ref

    cf.section(f"kei vendor: upstream asterinas @ {upstream_ref}")

    # Ensure remote
    if not git_check("remote", "get-url", "upstream"):
        git("remote", "add", "upstream", UPSTREAM_URL)

    cf.blank()
    cf.step("[1/5] Fetching upstream")
    result = git("fetch", "upstream", upstream_ref)
    if result.returncode != 0:
        cf.fail(f"Failed to fetch upstream/{upstream_ref}")
        return 1
    upstream_sha = git("rev-parse", "--short", f"upstream/{upstream_ref}", capture=True).stdout.strip()
    cf.ok(f"upstream/{upstream_ref} = {upstream_sha}")

    cf.blank()
    cf.step("[2/5] Snapshotting kei-specific code")
    stash_dir = Path(tempfile.mkdtemp(prefix="kei-vendor-"))
    try:
        for rel in OUR_PATHS:
            src = PROJECT_ROOT / rel
            if src.exists():
                dst = stash_dir / rel
                dst.parent.mkdir(parents=True, exist_ok=True)
                if src.is_dir():
                    shutil.copytree(src, dst, symlinks=True)
                else:
                    shutil.copy2(src, dst)
        cf.ok(f"Snapshot saved ({len(OUR_PATHS)} paths)")

        cf.blank()
        cf.step("[3/5] Replacing upstream source tree")
        for d in UPSTREAM_DIRS:
            target = PROJECT_ROOT / d
            if target.exists():
                shutil.rmtree(target)
            result = git("checkout", f"upstream/{upstream_ref}", "--", d)
            if result.returncode == 0:
                cf.ok(f"  {d}/ refreshed")
            else:
                cf.warn(f"  {d}/ not found in upstream (renamed/removed?)")

        for f in UPSTREAM_FILES:
            if git_check("cat-file", "-e", f"upstream/{upstream_ref}:{f}"):
                git("checkout", f"upstream/{upstream_ref}", "--", f)

        cf.blank()
        cf.step("[4/5] Restoring kei-specific code")
        for rel in OUR_PATHS:
            stash_path = stash_dir / rel
            if stash_path.exists():
                target = PROJECT_ROOT / rel
                if target.exists():
                    if target.is_dir():
                        shutil.rmtree(target)
                    else:
                        target.unlink()
                target.parent.mkdir(parents=True, exist_ok=True)
                if stash_path.is_dir():
                    shutil.copytree(stash_path, target, symlinks=True)
                else:
                    shutil.copy2(stash_path, target)
        cf.ok("kei code restored")

        cf.blank()
        cf.step("[5/5] Recording vendored version")
        full_sha = git("rev-parse", f"upstream/{upstream_ref}", capture=True).stdout.strip()
        date_result = git("log", "-1", "--format=%ci", f"upstream/{upstream_ref}", capture=True)
        upstream_date = date_result.stdout.strip() or "unknown"
        version_file = PROJECT_ROOT / ".vendored-upstream"
        version_file.write_text(
            f"upstream_url={UPSTREAM_URL}\n"
            f"upstream_ref={upstream_ref}\n"
            f"upstream_sha={full_sha}\n"
            f"upstream_date={upstream_date}\n"
            f"vendored_date={datetime.datetime.now(datetime.timezone.utc).isoformat()}\n"
        )
        cf.ok("Version recorded")

    finally:
        shutil.rmtree(stash_dir, ignore_errors=True)

    cf.blank()
    cf.ok(f"Vendoring complete: {upstream_sha}")
    cf.info("  Next: cargo check  (fix API breaks)")
    cf.info("  Next: just test-all")
    cf.info("  Commit: git add -A && git commit -m 'vendor: absorb asterinas {upstream_sha}'")
    return 0


if __name__ == "__main__":
    sys.exit(main())
