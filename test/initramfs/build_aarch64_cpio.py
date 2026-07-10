#!/usr/bin/env python3
"""Build a newc-format cpio initramfs from a rootfs directory.

Mirrors what `find . | cpio -o -H newc | gzip` produces.
Usage: build_aarch64_cpio.py <rootfs_dir> <output.cpio.gz>
"""
import os
import sys
import gzip
import stat


def make_header(ino, mode, uid, gid, nlink, mtime, devmajor, devminor,
                rdevmajor, rdevminor, namesize, filesize, name, body):
    """Build one newc cpio record. name includes trailing NUL."""
    name_bytes = name.encode() + b"\x00"
    namesize = len(name_bytes)
    fields = [
        b"070701",
        f"{ino:08X}".encode(),
        f"{mode:08X}".encode(),
        f"{uid:08X}".encode(),
        f"{gid:08X}".encode(),
        f"{nlink:08X}".encode(),
        f"{mtime:08X}".encode(),
        f"{filesize:08X}".encode(),
        f"{devmajor:08X}".encode(),
        f"{devminor:08X}".encode(),
        f"{rdevmajor:08X}".encode(),
        f"{rdevminor:08X}".encode(),
        f"{namesize:08X}".encode(),
        b"00000000",  # check (0)
    ]
    hdr = b"".join(fields)
    out = bytearray()
    out += hdr
    out += name_bytes
    # pad header+name to 4-byte boundary (header is 110 bytes)
    while len(out) % 4 != 0:
        out += b"\x00"
    if body:
        out += body
        while len(out) % 4 != 0:
            out += b"\x00"
    return bytes(out)


def build(rootfs, outpath):
    records = []
    ino = 1

    # Walk the tree deterministically.
    all_paths = []
    for dirpath, dirnames, filenames in os.walk(rootfs):
        dirnames.sort()
        filenames.sort()
        rel_dir = os.path.relpath(dirpath, rootfs)
        if rel_dir == ".":
            all_paths.append(".")
        else:
            all_paths.append(rel_dir)
        # For symlinks/hardlinks among dirnames too (rare)
        for fn in filenames:
            all_paths.append(os.path.normpath(os.path.join(rel_dir, fn)) if rel_dir != "." else fn)

    seen = set()
    # Always include "." first
    ordered = ["."]
    for p in all_paths[1:]:
        if p and p != "." and p not in seen:
            ordered.append(p)
            seen.add(p)

    # Extra device nodes that we cannot create as a regular user on the host.
    # These are injected as char-device cpio entries so the guest sees them
    # even before devtmpfs mounts (and as a fallback if devtmpfs is absent).
    # (name, major, minor, mode)
    EXTRA_DEVICES = [
        ("dev/console", 5, 1, 0o600),
        ("dev/null", 1, 3, 0o666),
        ("dev/tty", 5, 0, 0o666),
        ("dev/zero", 1, 5, 0o666),
        ("dev/urandom", 1, 9, 0o666),
    ]
    extra_names = {name for name, _, _, _ in EXTRA_DEVICES}

    for relpath in ordered:
        if not relpath or relpath in extra_names:
            # Skip empty names (defensive) and device nodes added separately.
            continue
        fullpath = os.path.join(rootfs, relpath) if relpath != "." else rootfs
        try:
            st = os.lstat(fullpath)
        except OSError as e:
            print(f"skip {relpath}: {e}", file=sys.stderr)
            continue
        mode = st.st_mode
        body = b""
        rdevmajor = 0
        rdevminor = 0
        if stat.S_ISDIR(mode):
            nlink = 2  # conservative
        elif stat.S_ISLNK(mode):
            target = os.readlink(fullpath)
            body = target.encode()
            nlink = 1
        elif stat.S_ISREG(mode):
            with open(fullpath, "rb") as f:
                body = f.read()
            nlink = 1
        elif stat.S_ISCHR(mode) or stat.S_ISBLK(mode):
            rdevmajor = os.major(st.st_rdev)
            rdevminor = os.minor(st.st_rdev)
            nlink = 1
        else:
            nlink = 1
        rec = make_header(
            ino=ino, mode=mode, uid=0, gid=0, nlink=nlink,
            mtime=0,  # zero mtime for reproducibility
            devmajor=0, devminor=0,
            rdevmajor=rdevmajor, rdevminor=rdevminor,
            namesize=0, filesize=len(body),
            name=relpath, body=body,
        )
        records.append(rec)
        ino += 1

    # Inject device nodes we couldn't create on the host.
    for name, major, minor, mode in EXTRA_DEVICES:
        rec = make_header(
            ino=ino, mode=mode | stat.S_IFCHR, uid=0, gid=0, nlink=1,
            mtime=0, devmajor=0, devminor=0,
            rdevmajor=major, rdevminor=minor,
            namesize=0, filesize=0,
            name=name, body=b"",
        )
        records.append(rec)
        ino += 1

    # Trailer
    trailer_name = b"TRAILER!!!\x00"
    # newc header: magic + 13 hex fields (ino,mode,uid,gid,nlink,mtime,filesize,
    # devmajor,devminor,rdevmajor,rdevminor,namesize,check)
    fields = [b"070701"] + [f"{0:08X}".encode()] * 11 + [f"{len(trailer_name):08X}".encode()] + [b"00000000"]
    trailer = b"".join(fields) + trailer_name
    while len(trailer) % 4 != 0:
        trailer += b"\x00"
    records.append(trailer)

    data = b"".join(records)
    # Pad final archive to 512 bytes (block alignment, helps some loaders)
    while len(data) % 512 != 0:
        data += b"\x00"

    with gzip.open(outpath, "wb") as f:
        f.write(data)
    print(f"Wrote {outpath}: {len(data)} bytes (uncompressed)")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: build_aarch64_cpio.py <rootfs_dir> <output.cpio.gz>")
        sys.exit(1)
    build(sys.argv[1], sys.argv[2])
