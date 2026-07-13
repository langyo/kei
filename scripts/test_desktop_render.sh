#!/usr/bin/env bash
# test_desktop_render.sh — render kei_desktop to a PNG on WSL host (no kernel).
#
# Creates a regular file at /tmp/fb0, runs kei_desktop with KEI_FB=/tmp/fb0,
# kills it after 2s, then converts the framebuffer to a PNG for inspection.
# Validates the desktop rendering (wallpaper/taskbar/start menu) before booting.
set -u
KEI="$HOME/celestia/kei"
ARIS="$HOME/celestia/aris"
BIN="$ARIS/target/x86_64-unknown-linux-musl/release/kei_desktop"
W=640; H=480; BPP=4
FB=/tmp/kei_fb0.bin
PNG="$KEI/target/kei_desktop_preview.png"

if [ ! -x "$BIN" ]; then
  echo "[err] binary not found: $BIN"; exit 1
fi

# Recreate the fb substitute file.
rm -f "$FB"
head -c $((W*H*BPP)) /dev/zero > "$FB"
echo "[host-test] fb0 substitute: $FB ($((W*H*BPP)) bytes)"

# Run kei_desktop in background with KEI_FB pointing at our file.
KEI_FB="$FB" "$BIN" &
PID=$!
echo "[host-test] started kei_desktop pid=$PID"
sleep 2
kill -TERM "$PID" 2>/dev/null
wait "$PID" 2>/dev/null

# Pixel stats + PNG conversion via the python helper.
python3 - "$FB" "$PNG" "$W" "$H" <<'PY'
import sys, zlib, struct
fb, png, W, H = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
data = open(fb,'rb').read()
print(f"[host-test] read {len(data)} bytes")
nonblack = sum(1 for i in range(0, len(data), 4) if any(data[i:i+3]))
print(f"[host-test] non-black pixels: {nonblack}/{W*H} ({100*nonblack/(W*H):.1f}%)")
b,g,r = data[0], data[1], data[2]
print(f"[host-test] first pixel BGR=({b},{g},{r}) = #{r:02x}{g:02x}{b:02x}")
# Sample taskbar area (should be dark) y=H-20
tb_off = ((H-20)*W + W//2)*4
b,g,r = data[tb_off], data[tb_off+1], data[tb_off+2]
print(f"[host-test] taskbar center BGR=({b},{g},{r}) = #{r:02x}{g:02x}{b:02x}")
# Sample wallpaper area (center)
wp_off = ((H//2)*W + W//2)*4
b,g,r = data[wp_off], data[wp_off+1], data[wp_off+2]
print(f"[host-test] wallpaper center BGR=({b},{g},{r}) = #{r:02x}{g:02x}{b:02x}")

# Write PNG (BGRX -> RGB)
rgb = bytearray(W*H*3)
for i in range(W*H):
    rgb[i*3]   = data[i*4+2]
    rgb[i*3+1] = data[i*4+1]
    rgb[i*3+2] = data[i*4]
def chunk(t,d):
    c=t+d; return struct.pack(">I",len(d))+c+struct.pack(">I",zlib.crc32(c)&0xFFFFFFFF)
raw = bytearray()
for y in range(H):
    raw.append(0); raw.extend(rgb[y*W*3:(y+1)*W*3])
idat = zlib.compress(bytes(raw),9)
open(png,'wb').write(b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack(">IIBBBBB",W,H,8,2,0,0,0))+chunk(b"IDAT",idat)+chunk(b"IEND",b""))
print(f"[host-test] wrote PNG: {png}")
PY
