#!/bin/bash
set -e
export AR=aarch64-linux-gnu-ar
export RANLIB=aarch64-linux-gnu-ranlib
cd /tmp/dropbear-2024.86
make clean 2>/dev/null || true
CC=/tmp/aarch64-linux-musl-gcc ./configure \
    --host=aarch64-linux-musl \
    --disable-zlib --disable-syslog \
    --disable-lastlog --disable-utmp --disable-utmpx \
    --disable-wtmp --disable-wtmpx \
    --disable-loginfunc --disable-pututline --disable-pututxline \
    2>&1 | tail -5
echo "=== building ==="
make -j4 PROGRAMS="dropbear dropbearkey" 2>&1 | tail -5
echo "=== verify ==="
file dropbear
ls -la dropbear dropbearkey
