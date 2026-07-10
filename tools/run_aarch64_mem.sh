#!/bin/bash
# Run kernel, wait, then dump guest physical memory at the descriptor/avail
# region via QMP pmemsave to see EXACTLY what QEMU thinks is there.
set -e
cd /opt/kei

KERNEL=target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf
SOCK=/tmp/qmpm.sock
rm -f "$SOCK" /tmp/memdump.bin

qemu-system-aarch64 \
    -cpu cortex-a72 \
    -machine virt,gic-version=3,virtualization=on \
    -m 2G \
    -smp 1 \
    --no-reboot \
    -nographic \
    -serial file:/tmp/serialm.log \
    -qmp unix:$SOCK,server,nowait \
    -kernel "$KERNEL" \
    -device virtio-gpu-device \
    -device virtio-keyboard-device \
    &
QEMU_PID=$!

echo "[qmp] waiting for boot..."
sleep 8

python3 - "$SOCK" <<'PYEOF'
import socket, sys, time, json
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sys.argv[1])
def recv_json(timeout=2):
    s.settimeout(timeout)
    data = b""
    try:
        while True:
            chunk = s.recv(4096)
            if not chunk: break
            data += chunk
            if b"\r\n" in chunk: break
    except socket.timeout:
        pass
    return data.decode(errors="replace")

recv_json()
s.sendall(json.dumps({"execute": "qmp_capabilities"}).encode() + b"\r\n")
recv_json()

# Dump guest PA 0x4013e000 (descriptor + avail ring), 8192 bytes
cmd = {"execute": "pmemsave",
       "arguments": {"val": 0x4013e000, "size": 8192, "filename": "/tmp/memdump.bin"}}
s.sendall(json.dumps(cmd).encode() + b"\r\n")
time.sleep(0.5)
print("pmemsave:", recv_json())
s.close()
PYEOF

kill $QEMU_PID 2>/dev/null || true
wait $QEMU_PID 2>/dev/null || true

echo "[qmp] === memory dump at 0x4013e000 (desc + avail) ==="
if [ -f /tmp/memdump.bin ]; then
    echo "Hexdump of first 128 bytes (descriptor table):"
    xxd /tmp/memdump.bin | head -8
    echo ""
    echo "Avail ring at offset 0x400 (1024):"
    xxd -s 0x400 -l 32 /tmp/memdump.bin
    echo ""
    echo "Used ring at offset 0x1000 (4096):"
    xxd -s 0x1000 -l 32 /tmp/memdump.bin
fi
