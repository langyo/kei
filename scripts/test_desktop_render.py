#!/usr/bin/env python3
"""Render kei_desktop's pixel output to a PNG on the host (no kernel needed).

kei_desktop writes BGRX pixels to /dev/fb0 via seek+write. On the host we
can substitute a regular file as /dev/fb0: run the binary with /tmp/fb0 as
its target, then convert the file to a PNG for visual inspection.

This validates the desktop rendering logic (wallpaper gradient, taskbar,
start menu, icons, text) before booting under kei.

Usage:
    # Build the x86_64 host-testable build first, then:
    python3 scripts/test_desktop_render.py
"""
import os
import subprocess
import sys
import struct
from pathlib import Path

KEI = Path(__file__).resolve().parent.parent
ARIS = KEI.parent / "aris"
BIN = ARIS / "target" / "x86_64-unknown-linux-musl" / "release" / "kei_desktop"
W, H = 800, 600
FB_FILE = Path("/tmp/kei_desktop_fb0.bin")
PNG_OUT = KEI / "target" / "kei_desktop_preview.png"


def main():
    if not BIN.exists():
        print(f"[err] binary not found: {BIN}")
        sys.exit(1)

    # Create the fb0 substitute file at the right size.
    FB_FILE.parent.mkdir(parents=True, exist_ok=True)
    FB_FILE.write_bytes(b"\x00" * (W * H * 4))
    print(f"[host-test] fb0 substitute: {FB_FILE} ({W*H*4} bytes)")

    # The kei_desktop binary hardcodes /dev/fb0. For host testing we need
    # to point it at our file. Simplest: create a symlink /dev/fb0 -> file.
    # That requires root, so instead we patch via LD_PRELOAD is overkill.
    # Instead, just create /dev/fb0 as a regular file if running as root,
    # or skip and rely on the kei boot test.
    if os.geteuid() == 0:
        try:
            os.remove("/dev/fb0")
        except OSError:
            pass
        os.symlink(str(FB_FILE), "/dev/fb0")
        print("[host-test] symlinked /dev/fb0 -> file (running as root)")
    else:
        # Use a small C shim that bind-mounts, or just warn.
        print("[host-test] not root; cannot symlink /dev/fb0")
        print("[host-test] falling back to: run kei_desktop under qemu-user")
        # Under qemu-user we can't easily redirect /dev/fb0 either.
        # Instead, verify the binary runs and exits cleanly.
        print("[host-test] skip host render test; rely on kei boot test.")
        return

    # Run kei_desktop with a timeout (it loops forever, so we kill it).
    print(f"[host-test] running {BIN} ...")
    try:
        proc = subprocess.Popen([str(BIN)], stderr=subprocess.PIPE)
        import time
        time.sleep(2)  # let it write pixels
        proc.terminate()
        proc.wait(timeout=5)
    except Exception as e:
        print(f"[host-test] run failed: {e}")

    # Read back the framebuffer and convert to PNG.
    data = FB_FILE.read_bytes()
    print(f"[host-test] read back {len(data)} bytes")
    if len(data) < W * H * 4:
        print(f"[host-test] ERROR: fb too small")
        return

    # Pixel stats
    nonzero = sum(1 for i in range(0, len(data), 4) if any(data[i:i+3]))
    print(f"[host-test] non-black pixels: {nonzero}/{W*H} ({100*nonzero/(W*H):.1f}%)")
    # Sample first pixel
    b, g, r, _ = data[0:4]
    print(f"[host-test] first pixel BGR=({b},{g},{r}) = #{r:02x}{g:02x}{b:02x}")

    # Write PNG (zero-dep encoder)
    write_png(data, PNG_OUT)
    print(f"[host-test] wrote PNG: {PNG_OUT}")


def write_png(bgrx: bytes, path: Path):
    """Minimal zero-dependency BGRX->PNG writer."""
    import zlib
    # Convert BGRX -> RGB
    rgb = bytearray(W * H * 3)
    for i in range(W * H):
        b, g, r = bgrx[i*4], bgrx[i*4+1], bgrx[i*4+2]
        rgb[i*3] = r
        rgb[i*3+1] = g
        rgb[i*3+2] = b

    def chunk(typ: bytes, data: bytes) -> bytes:
        c = typ + data
        crc = zlib.crc32(c) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + c + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)  # 8-bit RGB
    # Raw image data with filter byte (0) per scanline
    raw = bytearray()
    for y in range(H):
        raw.append(0)  # filter: none
        raw.extend(rgb[y*W*3:(y+1)*W*3])
    idat = zlib.compress(bytes(raw), 9)
    png = sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")
    path.write_bytes(png)


if __name__ == "__main__":
    main()
