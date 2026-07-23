#!/usr/bin/env python3
"""kei — build kernel for target board.

Usage:
    python3 scripts/build.py [board] [profile]
    python3 scripts/build.py nanopi-r3s
    python3 scripts/build.py nanopi-r3s release
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

sys.path.insert(0, str(Path(__file__).parent / "utils"))
import build_env
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent

ARCH_TO_TARGET = {
    "x86_64": "x86_64-unknown-none",
    "aarch64": "aarch64-unknown-none",
    "riscv64": "riscv64imac-unknown-none-elf",
    "loongarch64": "loongarch64-unknown-none-softfloat",
}

ARCH_TO_SCHEME = {
    "x86_64": None,
    "aarch64": "aarch64",
    "riscv64": "riscv",
    "loongarch64": "loongarch",
}


def find_nightly_cargo() -> list[str]:
    """Build the cargo command with the correct nightly toolchain.

    cargo-osdk internally spawns a cargo subprocess that needs nightly-only
    -Z flags. The stable cargo (default) rejects them. We must prepend the
    nightly toolchain's bin directory to PATH so OSDK's internal cargo picks
    up the nightly compiler.
    """
    toolchain = "nightly-2026-04-03"
    host_triple = "x86_64-unknown-linux-gnu"
    rustup_home = Path(os.environ.get("RUSTUP_HOME", os.path.expanduser("~/.rustup")))
    nightly_bin = rustup_home / "toolchains" / f"{toolchain}-{host_triple}" / "bin"
    if nightly_bin.exists():
        env = dict(os.environ)
        env["PATH"] = str(nightly_bin) + ":" + env.get("PATH", "")
        return env
    return dict(os.environ)


def find_llvm_objcopy() -> str | None:
    """Locate llvm-objcopy from the nightly toolchain."""
    toolchain = "nightly-2026-04-03"
    host_triple = "x86_64-unknown-linux-gnu"
    rustup_home = Path(os.environ.get("RUSTUP_HOME", os.path.expanduser("~/.rustup")))
    candidates = [
        rustup_home / "toolchains" / f"{toolchain}-{host_triple}" / "lib" / "rustlib" / host_triple / "bin" / "llvm-objcopy",
        shutil.which("llvm-objcopy"),
        shutil.which("aarch64-linux-gnu-objcopy"),
        shutil.which("objcopy"),
    ]
    for c in candidates:
        if c and Path(c).exists():
            return str(c)
    return None


def load_board_config(board: str) -> dict:
    config_path = PROJECT_ROOT / "configs" / f"{board}.toml"
    if not config_path.exists():
        cf.warn(f"Config not found: {config_path}, using defaults")
        return {"board": {"name": board, "arch": "aarch64"},
                "kernel": {"bsp_crate": "bsp-rk3566"}}
    with config_path.open("rb") as f:
        return tomllib.load(f)


def main() -> int:
    if build_env.wsl_main_guard():
        return 0
    import argparse

    parser = argparse.ArgumentParser(description="Build kei kernel")
    parser.add_argument("board", nargs="?", default="nanopi-r3s")
    parser.add_argument("profile", nargs="?", default="release")
    args = parser.parse_args()

    board = args.board
    profile = args.profile
    config = load_board_config(board)

    board_cfg = config.get("board", {})
    arch = board_cfg.get("arch", "aarch64")
    rust_target = ARCH_TO_TARGET.get(arch)
    if not rust_target:
        cf.fail(f"Unknown arch: {arch}")
        return 1

    output_dir = PROJECT_ROOT / "target" / "output" / board
    output_dir.mkdir(parents=True, exist_ok=True)

    cf.section(f"kei build: {board} ({profile})")

    # Verify kei tree is populated
    if not (PROJECT_ROOT / "packages/ostd").exists():
        cf.fail("kei tree not populated (ostd/ missing)")
        cf.info("  Run: just vendor && just pull arm64")
        return 1

    cf.blank()
    cf.step("[1/5] Board config loaded")
    cf.info(f"  Target: {rust_target}")
    cf.info(f"  Arch:   {arch}")

    # Generate board_config.rs from TOML (GPIO, LEDs, serial, framebuffer)
    gen_script = PROJECT_ROOT / "scripts" / "gen_board_config.py"
    if gen_script.exists():
        subprocess.run([sys.executable, str(gen_script)], cwd=PROJECT_ROOT, capture_output=True)

    # Ensure initramfs exists (cargo osdk build requires it)
    cf.blank()
    cf.step("[2/5] Preparing initramfs")
    initramfs_script = PROJECT_ROOT / "scripts" / "initramfs.py"
    subprocess.run(
        [sys.executable, str(initramfs_script), "--arch", arch],
        check=False,
    )

    cf.blank()
    cf.step("[3/5] Building kernel via cargo osdk")
    nightly_env = find_nightly_cargo()
    scheme = ARCH_TO_SCHEME.get(arch)
    cmd = ["cargo", "osdk", "build", "--target-arch", arch, "--profile", profile]
    if scheme:
        cmd.extend(["--scheme", scheme])
    result = subprocess.run(cmd, cwd=PROJECT_ROOT, env=nightly_env)
    if result.returncode != 0:
        cf.fail("Kernel build failed")
        cf.info("  TIP: verify ostd/src/arch/aarch64/ exists")
        cf.info("  TIP: ensure nightly-2026-04-03 toolchain is installed")
        return 1

    cf.blank()
    cf.step("[4/5] Copying build artifacts")
    # OSDK produces:
    #   target/osdk/aster-kernel-osdk-bin.qemu_elf — ELF for QEMU direct boot
    #   target/{rust_target}/{profile}/aster-kernel-osdk-bin — raw ELF from cargo
    # For aarch64, we also need an ARM64 Image (.bin) so QEMU passes the FDT
    # address in x0 (ELF format does not get FDT in x0).
    qemu_elf = PROJECT_ROOT / "target" / "osdk" / "aster-kernel-osdk-bin.qemu_elf"
    raw_elf = PROJECT_ROOT / "target" / rust_target / profile / "aster-kernel-osdk-bin"
    existing_bin = PROJECT_ROOT / "target" / "osdk" / "aster-kernel.bin"

    copied = False
    if arch == "aarch64" and qemu_elf.exists():
        # Generate ARM64 Image (.bin) from ELF using llvm-objcopy.
        # QEMU's -kernel with ARM64 Image format correctly passes FDT in x0.
        objcopy = find_llvm_objcopy()
        if objcopy:
            bin_path = output_dir / "kei-kernel.bin"
            subprocess.run(
                [objcopy, "-O", "binary", str(qemu_elf), str(bin_path)],
                check=False,
            )
            if bin_path.exists() and bin_path.stat().st_size > 100_000:
                cf.ok(f"  Kernel (ARM64 Image): {bin_path}")
                copied = True
            else:
                cf.warn("  objcopy produced invalid binary")

    if not copied:
        # Fall back to existing .bin or copy ELF directly
        for kp in [existing_bin, qemu_elf, raw_elf]:
            if kp.exists():
                shutil.copy2(kp, output_dir / "kei-kernel.bin")
                cf.ok(f"  Kernel: {output_dir / 'kei-kernel.bin'}")
                copied = True
                break

    if not copied:
        cf.warn("  Kernel binary not found at expected paths")
        cf.info(f"  Looked in: {[str(qemu_elf), str(raw_elf)]}")

    cf.blank()
    cf.step("[5/5] Compiling device tree")
    dtc = shutil.which("dtc")
    dtb_name = config.get("kernel", {}).get("dtb", "")
    if dtc and dtb_name:
        dtb_src = PROJECT_ROOT / "configs" / "board" / board / "device-tree"
        dts_files = list(dtb_src.glob("*.dts")) if dtb_src.exists() else []
        if dts_files:
            subprocess.run(
                ["dtc", "-I", "dts", "-O", "dtb",
                 "-o", str(output_dir / "board.dtb"),
                 str(dts_files[0])],
                check=False,
            )
            if (output_dir / "board.dtb").exists():
                cf.ok(f"  DTB: {output_dir / 'board.dtb'}")
            else:
                cf.warn("  DTB compilation failed")
        else:
            cf.info("  No .dts files found")
    else:
        cf.info("  (dtc not available or no DTB configured — skipping)")

    # Copy armbianEnv.txt for SD card image builder
    armbian_env = PROJECT_ROOT / "configs" / "board" / board / "armbianEnv.txt"
    if armbian_env.exists():
        shutil.copy2(armbian_env, output_dir / "armbianEnv.txt")
        cf.ok(f"  armbianEnv.txt copied")

    cf.blank()
    cf.ok(f"Build complete: {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
