#!/bin/bash
source $HOME/.cargo/env 2>/dev/null
cd "/mnt/d/源代码/工程项目/celestia/kei"
OBJCOPY="$HOME/.rustup/toolchains/nightly-2026-05-01-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy"
SRC="target/aarch64-unknown-none/debug/aster-kernel-osdk-bin"
DST="target/osdk/aster-kernel/aster-kernel-osdk-bin.img"
"$OBJCOPY" --strip-all -O binary "$SRC" "$DST"
ls -la "$DST"
xxd -s 0x38 -l 4 "$DST"
