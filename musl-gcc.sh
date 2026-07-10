#!/bin/bash
# musl-gcc wrapper for aarch64 cross-compilation
exec aarch64-linux-gnu-gcc \
    -nostdinc -isystem /tmp/musl-aarch64/include \
    -nostartfiles -static -no-pie -fno-pie \
    "$@" \
    /tmp/musl-aarch64/lib/crt1.o /tmp/musl-aarch64/lib/crti.o \
    -L/tmp/musl-aarch64/lib -lc /tmp/musl-aarch64/lib/crtn.o \
    -lgcc -lgcc_eh
