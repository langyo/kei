#!/usr/bin/env python3
"""kei — SD card image assembler for real hardware boot.

Constructs a single-flash GPT image for NanoPi R3S containing:
  - Armbian U-Boot at sector 64
  - ext4 boot partition at LBA 32768 (kernel + DTB + boot.scr + initramfs)

Requires an Armbian reference image for U-Boot and GPT header.

Usage:
    python3 scripts/make_sdcard.py [board] --armbian-image PATH/TO/armbian.img

The image is self-contained and can be flashed with:
    dd if=target/output/<board>/sdcard.img of=/dev/sdX bs=4M conv=fsync
"""
from __future__ import annotations

import os
import shutil
import struct
import subprocess
import sys
import uuid
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "utils"))
import cli_format as cf

PROJECT_ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = PROJECT_ROOT / "target" / "output"
SECTOR = 512

BOOT_PART_LBA = 32768
BOOT_PART_SIZE_MB = 128
GPT_TYPE_LINUX = "0fc63daf-8483-4772-8e79-3d69d8477de4"


def _uuid_str_to_bytes(s: str) -> bytes:
    parts = s.split("-")
    if len(parts) != 5:
        return b"\x00" * 16
    data = b""
    data += struct.pack("<I", int(parts[0], 16))
    data += struct.pack("<H", int(parts[1], 16))
    data += struct.pack("<H", int(parts[2], 16))
    data += bytes([int(parts[3][0:2], 16), int(parts[3][2:4], 16)])
    data += bytes(int(parts[4][i : i + 2], 16) for i in range(0, 12, 2))
    return data


def create_boot_ext4(board: str) -> Path | None:
    """Create ext4 filesystem image with kernel, DTB, boot.scr, initramfs."""
    output_dir = OUTPUT_DIR / board
    boot_dir = output_dir / "boot-staging"
    if boot_dir.exists():
        shutil.rmtree(boot_dir)
    boot_dir.mkdir(parents=True)

    # Put boot files in /boot/ subdirectory (matches Armbian U-Boot layout).
    # Armbian U-Boot's distro_bootcmd searches /boot/ for boot.scr and
    # armbianEnv.txt first. Files at root level are silently ignored.
    boot_sub = boot_dir / "boot"
    boot_sub.mkdir()

    artifacts = ["kei-kernel.bin", "board.dtb", "boot.scr", "armbianEnv.txt"]
    for name in artifacts:
        src = output_dir / name
        if src.exists():
            shutil.copy2(src, boot_sub / name)
        else:
            cf.warn(f"  {name} not found — skipping")

    initramfs = PROJECT_ROOT / "tests" / "initramfs" / "build" / "initramfs.cpio.gz"
    if initramfs.exists():
        shutil.copy2(initramfs, boot_sub / "initramfs.cpio.gz")
    else:
        # Try aarch64-specific name
        initramfs_alt = PROJECT_ROOT / "tests" / "initramfs" / "build" / "initramfs_aarch64.cpio.gz"
        if initramfs_alt.exists():
            shutil.copy2(initramfs_alt, boot_sub / "initramfs.cpio.gz")
        else:
            cf.warn("  initramfs.cpio.gz not found — run 'just build' first")

    ext4 = output_dir / "boot.ext4"
    if ext4.exists():
        ext4.unlink()
    subprocess.run(
        ["truncate", "-s", f"{BOOT_PART_SIZE_MB}M", str(ext4)], check=True,
    )
    result = subprocess.run(
        ["/usr/sbin/mkfs.ext4", "-q", "-F", "-L", "KEIBOOT",
         "-d", str(boot_dir), str(ext4)],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        cf.fail(f"mkfs.ext4 failed: {result.stderr}")
        return None
    return ext4


def write_gpt(image_path: Path, start_lba: int, end_lba: int) -> None:
    total_lba = image_path.stat().st_size // SECTOR

    with open(image_path, "r+b") as f:
        # Protective MBR
        mbr = bytearray(SECTOR)
        mbr[0x1C2] = 0xEE
        mbr[0x1C6:0x1CA] = struct.pack("<I", 1)
        max_lba = min(total_lba - 1, 0xFFFFFFFF)
        mbr[0x1CA:0x1CE] = struct.pack("<I", max_lba)
        mbr[0x1FE] = 0x55
        mbr[0x1FF] = 0xAA
        f.seek(0)
        f.write(bytes(mbr))

        # GPT Header
        header = bytearray(SECTOR)
        header[0:8] = b"EFI PART"
        struct.pack_into("<I", header, 8, 0x00010000)
        struct.pack_into("<I", header, 12, 92)
        struct.pack_into("<Q", header, 24, 1)
        struct.pack_into("<Q", header, 32, total_lba - 1)
        struct.pack_into("<Q", header, 40, 2048)
        struct.pack_into("<Q", header, 48, total_lba - 34)
        disk_guid = bytearray(16)
        for i in range(16):
            disk_guid[i] = uuid.uuid4().bytes[i]
        header[56:72] = bytes(disk_guid)
        struct.pack_into("<Q", header, 72, 2)
        struct.pack_into("<I", header, 80, 128)
        struct.pack_into("<I", header, 84, 128)
        f.seek(SECTOR)
        f.write(bytes(header))

        # Partition entries
        f.seek(2 * SECTOR)
        entry = bytearray(128)
        type_bytes = _uuid_str_to_bytes(GPT_TYPE_LINUX)
        entry[0:16] = type_bytes
        unique = bytearray(16)
        unique[0] = 1
        entry[16:32] = bytes(unique)
        struct.pack_into("<Q", entry, 32, start_lba)
        struct.pack_into("<Q", entry, 40, end_lba)
        name_bytes = "boot".encode("utf-16-le")[:72]
        entry[56 : 56 + len(name_bytes)] = name_bytes
        f.write(bytes(entry))
        for _ in range(127):
            f.write(b"\x00" * 128)

        # Backup
        backup_entries_lba = total_lba - 33
        f.seek(2 * SECTOR)
        entries = f.read(128 * 128)
        f.seek(backup_entries_lba * SECTOR)
        f.write(entries)

        part_crc = zlib.crc32(entries) & 0xFFFFFFFF
        struct.pack_into("<I", header, 88, part_crc)
        struct.pack_into("<I", header, 16, 0)
        hdr_crc = zlib.crc32(bytes(header[:92])) & 0xFFFFFFFF
        struct.pack_into("<I", header, 16, hdr_crc)
        f.seek(SECTOR)
        f.write(bytes(header))

        backup = bytearray(header)
        struct.pack_into("<Q", backup, 24, total_lba - 1)
        struct.pack_into("<Q", backup, 32, 1)
        struct.pack_into("<Q", backup, 72, backup_entries_lba)
        struct.pack_into("<I", backup, 16, 0)
        backup_crc = zlib.crc32(bytes(backup[:92])) & 0xFFFFFFFF
        struct.pack_into("<I", backup, 16, backup_crc)
        f.seek((total_lba - 1) * SECTOR)
        f.write(bytes(backup))


def make_image(board: str, armbian_img: Path) -> Path | None:
    output_dir = OUTPUT_DIR / board
    output_dir.mkdir(parents=True, exist_ok=True)

    if not armbian_img.exists():
        cf.fail(f"Armbian image not found: {armbian_img}")
        return None

    cf.step("[1/4] Compiling boot script")
    boot_cmd = PROJECT_ROOT / "configs" / "board" / board / "boot.cmd"
    boot_scr = output_dir / "boot.scr"
    mkimage = shutil.which("mkimage")
    if mkimage and boot_cmd.exists():
        subprocess.run(
            [mkimage, "-C", "none", "-A", "arm", "-T", "script",
             "-d", str(boot_cmd), str(boot_scr)],
            capture_output=True,
        )
        if boot_scr.exists():
            cf.ok(f"  boot.scr ({boot_scr.stat().st_size} bytes)")
    else:
        cf.warn("  mkimage not found — boot.scr not compiled")

    # Find initramfs
    initramfs = PROJECT_ROOT / "tests" / "initramfs" / "build" / "initramfs.cpio.gz"
    if not initramfs.exists():
        cf.warn("  initramfs not found — run 'just build' first")
    else:
        cf.ok(f"  initramfs ({initramfs.stat().st_size} bytes)")

    cf.step("[2/4] Creating boot filesystem")
    ext4 = create_boot_ext4(board)
    if not ext4:
        return None
    ext4_size = ext4.stat().st_size
    cf.ok(f"  boot.ext4 ({ext4_size // (1024 * 1024)}MB)")

    cf.step("[3/4] Assembling SD card image")
    sdcard = output_dir / "sdcard.img"
    armbian_head = BOOT_PART_LBA * SECTOR
    total_size = armbian_head + ext4_size + 33 * SECTOR

    if sdcard.exists():
        sdcard.unlink()
    subprocess.run(["truncate", "-s", str(total_size), str(sdcard)], check=True)

    # Copy Armbian sectors 0-32767 (U-Boot + env + GPT header)
    # KEEP the Armbian GPT intact — DO NOT overwrite it. Armbian U-Boot
    # expects the exact GPT it was built with. Creating a new GPT breaks
    # U-Boot's ability to find the boot partition (same lesson as evernight).
    with open(sdcard, "r+b") as img:
        img.seek(0)
        with open(armbian_img, "rb") as src:
            chunk = src.read(armbian_head)
            img.write(chunk)

        # Write boot partition at LBA 32768 (same as Armbian's partition start)
        img.seek(BOOT_PART_LBA * SECTOR)
        with open(ext4, "rb") as src:
            while True:
                chunk = src.read(1024 * 1024)
                if not chunk:
                    break
                img.write(chunk)

    # The Armbian GPT already describes the partition at LBA 32768.
    # We preserve it unchanged. The GPT says the partition is larger than
    # our actual content — this is fine; U-Boot only reads the first few MB.
    cf.step("[4/4] Preserving Armbian GPT (no rewrite)")
    cf.ok("  GPT kept from Armbian reference image")

    ext4.unlink(missing_ok=True)
    shutil.rmtree(output_dir / "boot-staging", ignore_errors=True)

    size_mb = sdcard.stat().st_size // (1024 * 1024)
    cf.blank()
    cf.ok(f"SD card image: {sdcard} ({size_mb}MB)")
    cf.info(f"  Flash: dd if={sdcard.name} of=/dev/sdX bs=4M conv=fsync")
    return sdcard


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Build kei SD card image")
    parser.add_argument("board", nargs="?", default="nanopi-r3s")
    parser.add_argument("--armbian-image", required=True,
                        help="Path to Armbian reference image (for U-Boot)")
    args = parser.parse_args()

    armbian = Path(args.armbian_image)
    if not armbian.exists():
        cf.fail(f"Armbian image not found: {armbian}")
        sys.exit(1)

    result = make_image(args.board, armbian)
    sys.exit(0 if result else 1)
