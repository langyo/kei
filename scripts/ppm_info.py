#!/usr/bin/env python3
"""Read a PPM (P6) file and print its dimensions + pixel-data size.

Used by the `screenshot` just recipe to validate/summarise a QEMU monitor
screendump. Exits non-zero if the file isn't a valid P6 PPM.

Kept as a standalone script (rather than an inline `python -c "..."`) because
`just` parses recipe bodies line-by-line: an indented Python heredoc with
column-0 lines confuses its parser, so the logic lives here instead.
"""
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <file.ppm>", file=sys.stderr)
        return 2
    with open(sys.argv[1], "rb") as f:
        # PPM P6 header: P6\n<width> <height>\n255\n
        magic = f.readline()
        if magic.strip() != b"P6":
            return 1
        dims = f.readline().split()
        w, h = int(dims[0]), int(dims[1])
        f.readline()  # maxval
        data = f.read()
    print(f"{w}x{h}, {len(data)} bytes pixel data")
    return 0


if __name__ == "__main__":
    sys.exit(main())
