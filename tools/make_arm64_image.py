#!/usr/bin/env python3
"""Convert an aarch64 ELF kernel to a QEMU-bootable ARM64 Image.

The ARM64 Image format has a 64-byte header at offset 0 that QEMU
recognizes. When loading such an image, QEMU sets x0 = FDT pointer
(unlike ELF kernels where x0 = 0). This script:

  1. Reads the ELF and extracts all LOADable segments by physical address.
  2. Pads the binary to cover BSS (zero-fill between file end and mem end).
  3. Writes the ARM64 Image header with the correct image_size field.

Usage: python3 make_arm64_image.py <input.elf> <output.image>
"""

import struct
import sys


def read_elf_segments(path):
    """Return list of (paddr, offset, filesz, memsz) for LOAD segments."""
    with open(path, "rb") as f:
        data = f.read()

    # ELF64 header
    assert data[:4] == b"\x7fELF", "Not an ELF file"
    e_phoff = struct.unpack_from("<Q", data, 32)[0]
    e_phentsize = struct.unpack_from("<H", data, 54)[0]
    e_phnum = struct.unpack_from("<H", data, 56)[0]

    segments = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from("<I", data, off)[0]
        if p_type != 1:  # PT_LOAD
            continue
        p_offset = struct.unpack_from("<Q", data, off + 8)[0]
        p_paddr = struct.unpack_from("<Q", data, off + 24)[0]
        p_filesz = struct.unpack_from("<Q", data, off + 32)[0]
        p_memsz = struct.unpack_from("<Q", data, off + 40)[0]
        segments.append((p_paddr, p_offset, p_filesz, p_memsz))
    return segments


def make_image(elf_path, out_path):
    segments = read_elf_segments(elf_path)
    if not segments:
        print("ERROR: No LOAD segments found", file=sys.stderr)
        sys.exit(1)

    # Find the base physical address (lowest paddr) and total span.
    base_paddr = min(s[0] for s in segments)
    max_end = max(s[0] + s[3] for s in segments)  # include BSS (memsz)
    total_size = max_end - base_paddr

    print(f"Base PA: {base_paddr:#x}")
    print(f"Total image size (incl BSS): {total_size:#x} ({total_size} bytes)")

    # Read ELF data
    with open(elf_path, "rb") as f:
        elf_data = f.read()

    # Build the flat binary, zero-filled for BSS.
    image = bytearray(total_size)
    for paddr, offset, filesz, memsz in segments:
        rel = paddr - base_paddr
        # Copy file data
        image[rel : rel + filesz] = elf_data[offset : offset + filesz]
        # BSS (memsz > filesz) is already zeroed by bytearray init.

    # Read the original ARM64 Image header from the ELF (first 64 bytes of
    # the .boot segment, which is at base_paddr in the flat binary).
    header = bytes(image[:64])

    # The header's code0 (offset 0, 4 bytes) is the "b _start" instruction.
    # text_offset (offset 8, 8 bytes) should be 0x80000.
    # image_size (offset 16, 8 bytes) — set to the full binary size.
    # flags (offset 24, 8 bytes) — keep existing.
    # magic (offset 56, 4 bytes) — "ARMd" (0x644d5241).

    text_offset = struct.unpack_from("<Q", header, 8)[0]
    flags = struct.unpack_from("<Q", header, 24)[0]
    magic = header[56:60]

    print(f"Original text_offset: {text_offset:#x}")
    print(f"Original flags: {flags:#x}")
    print(f"Magic: {magic}")

    # Patch the header: set image_size to the total binary size.
    # The ARM64 boot protocol expects image_size to be the size of the
    # entire image (header + code + data + bss), starting from offset 0
    # relative to the image base. Since our flat binary starts at
    # base_paddr (0x40080000) which is RAM_base + text_offset, the
    # image_size should be total_size.
    struct.pack_into("<Q", image, 16, total_size)

    # Ensure the magic is correct.
    image[56:60] = b"ARMd"

    with open(out_path, "wb") as f:
        f.write(image)

    print(f"Wrote {len(image)} bytes to {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input.elf> <output.image>")
        sys.exit(1)
    make_image(sys.argv[1], sys.argv[2])
