#!/usr/bin/env python3
"""Minimal PPM (P6) → PNG writer with zero dependencies.

Usage: python3 ppm_to_png.py input.ppm output.png [scale]
   or: python3 ppm_to_png.py input.ppm   # writes input.png next to it

The PNG is constructed by hand (single IDAT, zlib via stdlib). This lets us
view kei QEMU screendumps on hosts without ImageMagick/Pillow.
"""
import struct
import sys
import zlib


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    idx = 0
    fields = []

    def skip_ws(i):
        while i < len(data) and data[i] in b" \t\n\r":
            i += 1
        return i

    def read_token(i):
        i = skip_ws(i)
        # skip comments
        while i < len(data) and data[i] == 0x23:  # '#'
            while i < len(data) and data[i] != 0x0A:
                i += 1
            i = skip_ws(i)
        j = i
        while j < len(data) and data[j] not in b" \t\n\r":
            j += 1
        return data[i:j], j

    magic, idx = read_token(idx)
    if magic != b"P6":
        raise ValueError(f"not a P6 PPM: magic={magic!r}")
    w_tok, idx = read_token(idx)
    h_tok, idx = read_token(idx)
    max_tok, idx = read_token(idx)
    w = int(w_tok)
    h = int(h_tok)
    mx = int(max_tok)
    # single whitespace after maxval
    idx += 1
    pixels = data[idx : idx + w * h * 3]
    if mx != 255:
        # scale to 8-bit
        pixels = bytes(int(p) * 255 // mx for p in pixels)
    return w, h, pixels


def write_png(path, w, h, rgb):
    def chunk(tag, body):
        c = tag + body
        crc = zlib.crc32(c) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + c + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)  # 8-bit, color type 2 (RGB)
    # add filter byte (0) per scanline
    raw = bytearray()
    stride = w * 3
    for y in range(h):
        raw.append(0)
        raw.extend(rgb[y * stride : (y + 1) * stride])
    idat = zlib.compress(bytes(raw), 9)
    with open(path, "wb") as f:
        f.write(sig)
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", idat))
        f.write(chunk(b"IEND", b""))


def downscale(w, h, rgb, scale):
    if scale <= 1:
        return w, h, rgb
    nw, nh = w // scale, h // scale
    out = bytearray(nw * nh * 3)
    stride = w * 3
    for y in range(nh):
        for x in range(nw):
            sy, sx = y * scale, x * scale
            base = (sy * w + sx) * 3
            obase = (y * nw + x) * 3
            out[obase] = rgb[base]
            out[obase + 1] = rgb[base + 1]
            out[obase + 2] = rgb[base + 2]
    return nw, nh, bytes(out)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    inp = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else (inp.rsplit(".", 1)[0] + ".png")
    scale = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    w, h, rgb = read_ppm(inp)
    w, h, rgb = downscale(w, h, rgb, scale)
    write_png(out, w, h, rgb)
    # stats
    n = len(rgb) // 3
    nonblack = sum(1 for i in range(0, len(rgb), 3) if rgb[i] or rgb[i + 1] or rgb[i + 2])
    print(f"[ppm2png] {inp} -> {out} {w}x{h} nonblack={nonblack}/{n} ({100*nonblack/max(n,1):.1f}%)")


if __name__ == "__main__":
    main()
