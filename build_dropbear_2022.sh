#!/bin/bash
set -e
export AR=aarch64-linux-gnu-ar
export RANLIB=aarch64-linux-gnu-ranlib
cd /tmp/dropbear-2022.83
make clean 2>/dev/null || true
CC=/tmp/aarch64-linux-musl-gcc ./configure \
    --host=aarch64-linux-musl \
    --disable-zlib --disable-syslog \
    --disable-lastlog --disable-utmp --disable-utmpx \
    --disable-wtmp --disable-wtmpx \
    --disable-loginfunc --disable-pututline --disable-pututxline \
    2>&1 | tail -5
echo "=== removing PIE from Makefile ==="
sed -i 's/-Wl,-pie//' Makefile
echo "=== building ==="
make -j4 PROGRAMS="dropbear dropbearkey" 2>&1 | tail -5
echo "=== verify ==="
file dropbear
aarch64-linux-gnu-readelf -l dropbear 2>/dev/null | grep interpreter && echo "DYNAMIC!" || echo "STATIC_OK"
ls -la dropbear dropbearkey
