#!/usr/bin/env bash
# Run inside WSL. Build the kei kernel for the requested arch via cargo-osdk.
# Writes the produced qemu_elf to target/kernels/<arch>/.
set -u
ARCH="${1:-aarch64}"
KEI="/mnt/d/源代码/工程项目/celestia/kei"

# Map arch -> OSDK scheme name (see OSDK.toml).
case "$ARCH" in
  aarch64) SCHEME="aarch64" ;;
  riscv64) SCHEME="riscv" ;;
  x86_64)  SCHEME="microvm" ;;
  *) echo "[err] unknown arch: $ARCH"; exit 1 ;;
esac

cd "$KEI" || { echo "[err] cd failed"; exit 1; }

# Ensure cargo (and cargo-osdk) are on PATH (the login shell sets this up,
# but scripts invoked via `bash script.sh` may not inherit it).
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "[err] cargo not found on PATH"; exit 1; }
command -v cargo-osdk >/dev/null 2>&1 || { echo "[err] cargo-osdk not found; install with: cargo install cargo-osdk"; exit 1; }

# riscv64 and x86_64 kernels need VDSO_LIBRARY_DIR pointing at the prebuilt
# vDSO .so files (aarch64/loongarch64 don't use vDSO).
export VDSO_LIBRARY_DIR="$KEI/tests/vdso"
echo "[build] VDSO_LIBRARY_DIR=$VDSO_LIBRARY_DIR"

# Fix any permission oddities on the target tree (the build-std fingerprint
# dirs sometimes end up unwriteable under the DrvFs mount).
chmod -R u+w target/release/.fingerprint 2>/dev/null

echo "[build] cargo osdk build --target-arch $ARCH --scheme $SCHEME --release"
cargo osdk build --target-arch "$ARCH" --scheme "$SCHEME" --release 2>&1 | tail -20
RC=${PIPESTATUS[0]}
echo "[build] cargo osdk exit code: $RC"

if [ $RC -ne 0 ]; then
  echo "[build] FAILED"
  exit $RC
fi

# Locate the produced kernel and copy it to a per-arch slot.
# OSDK writes to target/osdk/aster-kernel/, but if the final packaging (strip)
# step fails (rust-strip not in PATH), the raw ELF is still in the per-arch
# target dir. Fall back to that.
SRC="$KEI/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf"
if [ ! -f "$SRC" ]; then
  # Fall back to the unstripped ELF in the arch-specific target dir.
  case "$ARCH" in
    aarch64) SRC="$KEI/target/aarch64-unknown-none/release/aster-kernel-osdk-bin" ;;
    riscv64) SRC="$KEI/target/riscv64imac-unknown-none-elf/release/aster-kernel-osdk-bin" ;;
    x86_64)  SRC="$KEI/target/x86_64-unknown-none/release/aster-kernel-osdk-bin" ;;
  esac
  if [ ! -f "$SRC" ]; then
    echo "[build] kernel artifact not found (neither OSDK output nor $SRC)"
    exit 1
  fi
  echo "[build] using unstripped ELF fallback: $SRC"
fi
mkdir -p "$KEI/target/kernels/$ARCH"
cp "$SRC" "$KEI/target/kernels/$ARCH/aster-kernel-osdk-bin.qemu_elf"
echo "[build] copied kernel -> target/kernels/$ARCH/aster-kernel-osdk-bin.qemu_elf"
file "$KEI/target/kernels/$ARCH/aster-kernel-osdk-bin.qemu_elf"

# For aarch64, also produce the raw ARM64 Image format (.image). QEMU's
# -kernel only generates and passes the FDT via x0 when the kernel is in
# ARM64 Image format (magic "ARMd" at file offset 56). ELF kernels get x0=0
# and no FDT in RAM, which breaks our FDT scan fallback.
if [ "$ARCH" = "aarch64" ]; then
  if command -v aarch64-linux-gnu-objcopy >/dev/null 2>&1; then
    aarch64-linux-gnu-objcopy -O binary "$SRC" "$KEI/target/kernels/$ARCH/aster-kernel-osdk-bin.image"
    echo "[build] produced ARM64 Image: target/kernels/$ARCH/aster-kernel-osdk-bin.image"
    file "$KEI/target/kernels/$ARCH/aster-kernel-osdk-bin.image"
  else
    echo "[build] WARN: aarch64-linux-gnu-objcopy not found; skipping .image generation"
    echo "[build]       (the ELF will boot but FDT scan may fail)"
  fi
fi
