# kei — build & run commands
# Usage: just <recipe>
#
# Quick start (aarch64):
#   just setup-keys   # generate SSH keys (one-time)
#   just run          # build + launch QEMU with SDL window
#
# For other architectures:
#   just run aarch64
#   just run x86_64
#   just run riscv64

set shell := ["bash", "-c"]
set windows-shell := ["bash.exe", "-c"]
set unstable
set lists

# Auto-load .env so ARIS_REPO (and other config) is available in all recipes.
# just >= 1.32 supports this natively; we're on 1.55.
set dotenv-load := true

# Path to the aris repository. Override via .env or shell env var.
# Used by build-aris / build-desktop recipes and the initramfs scripts.
ARIS_REPO := env_var_or_default("ARIS_REPO", "../aris")

# Shared celestia-devtools recipes — NOT in git. This justfile references shared
# variables, so the import is REQUIRED. Bootstrap once: celestia-devtools init
# (or `just fetch` if already staged). Refresh after upgrades.
import? "./.just/git-bash-interop.just"
import "./.just/celestia-devtools.just"

# Stage shared celestia-devtools recipes into .just/ (gitignored).
# Source order: explicit URL arg → local pip bundle (offline) → GitHub raw.
# curl honors HTTP_PROXY/HTTPS_PROXY/ALL_PROXY env vars automatically.
[script('bash')]
fetch URL='':
    #!/usr/bin/env bash
    set -euo pipefail
    out=.just/celestia-devtools.just
    mkdir -p .just
    if [ -n "{{URL}}" ]; then
      echo "[fetch] {{URL}} -> $out"
      curl -fsSL "{{URL}}" -o "$out"
    elif command -v celestia-devtools >/dev/null 2>&1; then
      src=$(celestia-devtools include-path)
      echo "[fetch] local bundle ($src) -> $out"
      cp "$src" "$out"
    else
      echo "[fetch] github raw -> $out"
      curl -fsSL "https://raw.githubusercontent.com/celestia-island/celestia-devtools/dev/src/celestia_devtools/common.just" -o "$out"
    fi
    echo "[fetch] wrote $out"

default: list-arch

# ── Environment ─────────────────────────────────────────────

# Inspect the build environment: host kind, WSL2 distros (on Windows),
# selected distro, and container backend. Pre-flight check before build.
env-check:
    {{python_cmd}} scripts/check_env.py

# ── Vendoring (Apple LLVM model: pin + periodically absorb) ──

setup:
    {{python_cmd}} scripts/setup.py

# Vendor upstream asterinas into the tree.
#   just vendor          # latest
#   just vendor <ref>    # specific git ref
vendor *ARGS='':
    {{python_cmd}} scripts/vendor_upstream.py {{ARGS}}

# Pull (vendor) upstream code.
#   just pull arm64          # latest arm64 code
#   just pull arm64 <ref>    # specific git ref
[script('sh')]
pull target='arm64' *ARGS='':
    set -euo pipefail
    case "{{target}}" in
      arm64)
        {{python_cmd}} scripts/pull_arm64.py {{ARGS}}
        ;;
      *)
        echo "unknown pull target: {{target}}" >&2
        echo "usage: just pull [arm64]" >&2
        exit 1
        ;;
    esac

versions:
    @echo "=== Upstream asterinas ==="
    @cat .vendored-upstream 2>/dev/null || echo "  (not vendored yet — run 'just vendor')"
    @echo ""
    @echo "=== ARM64 source ==="
    @cat .vendored-arm64 2>/dev/null || echo "  (not pulled yet — run 'just pull arm64')"

# ── SSH Keys (aarch64) ──────────────────────────────────────
#
# dropbear uses public-key auth only (no password). Generate the
# client keypair and embed the public key into the initramfs.

# Generate an ed25519 SSH keypair for VM access (one-time setup).
# The private key is saved to tests/initramfs/build/client_ssh_key.
[script('bash')]
setup-keys:
    set -e
    KEYDIR="tests/initramfs/build"
    mkdir -p "$KEYDIR"
    if [ -f "$KEYDIR/client_ssh_key" ]; then
        echo "SSH key already exists at $KEYDIR/client_ssh_key"
    else
        ssh-keygen -t ed25519 -N "" -C "kei@aarch64" \
            -f "$KEYDIR/client_ssh_key"
        echo "Generated SSH keypair:"
        echo "  Private: $KEYDIR/client_ssh_key"
        echo "  Public:  $KEYDIR/client_ssh_key.pub"
    fi
    # Also copy to /tmp for the rootfs build scripts
    cp "$KEYDIR/client_ssh_key.pub" /tmp/client_ssh_key.pub 2>/dev/null || true

# Show SSH connection instructions for the running VM.
ssh-info:
    @echo ""
    @echo "╔══════════════════════════════════════════════════════════════╗"
    @echo "║                    SSH Connection Info                       ║"
    @echo "╠══════════════════════════════════════════════════════════════╣"
    @echo "║  Host:     127.0.0.1                                         ║"
    @echo "║  Port:     2222                                              ║"
    @echo "║  User:     root                                              ║"
    @echo "║  Auth:     public-key (ed25519)                              ║"
    @echo "║  Key:      tests/initramfs/build/client_ssh_key               ║"
    @echo "╠══════════════════════════════════════════════════════════════╣"
    @echo "║  Connect:                                                    ║"
    @echo "║    ssh -i tests/initramfs/build/client_ssh_key \\             ║"
    @echo "║        -o StrictHostKeyChecking=no -p 2222 root@127.0.0.1    ║"
    @echo "╚══════════════════════════════════════════════════════════════╝"
    @echo ""

# ── Build ──────────────────────────────────────────────────
#
# Build verbs follow a two-level convention:  just build <object> [args]
#   just build                    # default board (NanoPi R3S)
#   just build board <BOARD>      # specific board
#   just build kernel <ARCH>      # kei kernel only (aarch64|x86_64|riscv64)
#   just build browser <ARCH>     # aris-render browser engine (musl cross)
#   just build desktop <ARCH>     # full stack: kernel + browser + initramfs

# Build dispatcher: just build <object> [args...]
build WHAT="default" ARG1="":
    @case "{{WHAT}}" in \
        default)  just _build-default ;; \
        board)    just _build-board "{{ARG1}}" ;; \
        kernel)   just build-arch "{{ARG1}}" ;; \
        browser)  just _build-browser "{{ARG1}}" ;; \
        desktop)  just _build-desktop "{{ARG1}}" ;; \
        *) echo "Usage: just build [board|kernel|browser|desktop] [arg]"; \
           echo "  just build              # default board (NanoPi R3S)"; \
           echo "  just build board <name> # specific board"; \
           echo "  just build kernel <arch>  # aarch64|x86_64|riscv64"; \
           echo "  just build browser <arch> # aris-render musl binary"; \
           echo "  just build desktop <arch> # full stack"; exit 1 ;; \
    esac

_build-default:
    just cache-guard
    {{python_cmd}} scripts/build.py nanopi-r3s

_build-board BOARD:
    just cache-guard
    {{python_cmd}} scripts/build.py {{BOARD}}

# ── Dev ─────────────────────────────────────────────────────

# Quick dev launch: build + run QEMU for the host architecture.
# On Windows defaults to aarch64 (SDL window + virtio-gpu display).
# Usage: just dev              # auto-detect (aarch64 on Windows)
#        just dev aarch64      # ARM64 with SDL window
#        just dev x86_64       # x86_64 serial console
dev ARCH="":
    just run {{ARCH}}

# Run kei with aris-rendered UI filling the entire screen.
# Usage: just render             # aarch64 QEMU + aris-rendered desktop
[script('bash')]
render ARCH="aarch64":
    RENDER_UI=1 just _run-aarch64 0

# ── aris cross-compilation (browser engine) ─────────────────
#
# Internal recipes invoked by `just build browser` / `just build desktop`.
# aris must be checked out at $ARIS_REPO (see .env / .env.example).
# The build runs inside WSL because Windows has no musl cross-toolchain;
# aris's .cargo/config.toml uses rust-lld self-contained linking.

# Compile the aris-render browser engine (kei_desktop) for the target arch.
# Invoked via: just build browser <ARCH>
[script('bash')]
_build-browser ARCH="aarch64":
    set -e
    ARCH="{{ARCH}}"
    ARIS="{{ARIS_REPO}}"
    case "$ARCH" in
        aarch64) TRIPLE="aarch64-unknown-linux-musl" ;;
        riscv64) TRIPLE="riscv64gc-unknown-linux-musl" ;;
        x86_64)  TRIPLE="x86_64-unknown-linux-musl" ;;
        *) echo "Unsupported arch: $ARCH (aarch64|riscv64|x86_64)"; exit 1 ;;
    esac

    # Resolve ARIS to an absolute path, then convert to a WSL /mnt/... path
    # so cargo inside Ubuntu-24.04 can find the source tree.
    ARIS_ABS=$(cd "$ARIS" 2>/dev/null && pwd || echo "$ARIS")
    echo "[build browser] ARIS_REPO=$ARIS_ABS  triple=$TRIPLE"

    # Windows path → WSL path (D:\foo\bar → /mnt/d/foo/bar)
    WSL_ARIS=$(echo "$ARIS_ABS" | sed 's|\\|/|g' | sed -E 's|^([A-Za-z]):|/mnt/\L\1|')

    wsl -d Ubuntu-24.04 -- bash -lc \
        'cd "$1" && source ~/.cargo/env 2>/dev/null && RUSTUP_TOOLCHAIN=nightly-2026-05-01 cargo build --release --target "$2" -p aris-render --no-default-features --features "render fbdev" --bin kei_desktop' \
        bash "$WSL_ARIS" "$TRIPLE" 2>&1 | tail -15

    echo "[build browser] done: $ARIS_ABS/target/$TRIPLE/release/kei_desktop"

# Full desktop stack: kernel + browser + initramfs.
# Invoked via: just build desktop <ARCH>
[script('bash')]
_build-desktop ARCH="aarch64":
    set -e
    ARCH="{{ARCH}}"
    echo "═══════ build desktop: $ARCH ═══════"
    echo "[1/3] Building kei kernel..."
    just build-arch "$ARCH"
    echo "[2/3] Building aris-render browser..."
    just _build-browser "$ARCH"
    echo "[3/3] Packaging initramfs..."
    ARIS_REPO="{{ARIS_REPO}}" {{python_cmd}} scripts/build_desktop_initramfs.py "$ARCH"
    echo "═══════ done: just render $ARCH ═══════"

# Build only (no QEMU launch).
dev-build ARCH="":
    just build-arch {{ARCH}}

# Build the kernel for a specific architecture.
# Usage: just build-arch aarch64  (or x86_64, riscv64, loongarch64)
[script('bash')]
build-arch ARCH:
    set -e
    ARCH="{{ARCH}}"
    case "$ARCH" in
        aarch64)
            just _build-aarch64
            ;;
        x86_64)
            cargo osdk build --target x86_64-unknown-none
            ;;
        riscv64)
            cargo osdk build --scheme riscv --target-arch riscv64
            ;;
        loongarch64)
            cargo osdk build --scheme loongarch --target-arch loongarch64
            ;;
        *)
            echo "Unsupported arch: $ARCH"
            echo "Supported: aarch64, x86_64, riscv64, loongarch64"
            exit 1
            ;;
    esac

# Build aarch64 kernel + ARM64 Image + initramfs (internal).
[script('bash')]
_build-aarch64:
    set -e
    echo "[build] Building aarch64 kernel..."
    wsl -d Ubuntu-24.04 -- bash -lc 'source ~/.cargo/env 2>/dev/null; cd "/mnt/d/源代码/工程项目/celestia/kei" && cargo osdk build --scheme aarch64 --target-arch aarch64' 2>&1 | tail -5
    # Copy ELF if OSDK packaging failed (WSL/9p issue)
    if [ ! -f target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf ]; then
        cp target/osdk/aster-kernel-osdk-bin.qemu_elf target/osdk/aster-kernel/ 2>/dev/null || true
    fi
    # Build ARM64 Image from ELF
    echo "[build] Creating ARM64 Image..."
    wsl -d Ubuntu-24.04 -- bash -c 'python3 "/mnt/d/源代码/工程项目/celestia/kei/scripts/tools/make_arm64_image.py" "/mnt/d/源代码/工程项目/celestia/kei/target/osdk/aster-kernel/aster-kernel-osdk-bin.qemu_elf" "/mnt/d/源代码/工程项目/celestia/kei/target/osdk/aster-kernel/aster-kernel-osdk-bin.image" 2>&1 | tail -1'
    echo "[build] Done. Kernel image: target/osdk/aster-kernel/aster-kernel-osdk-bin.image"

# Format Rust + Markdown docs
fmt:
    cargo fmt --all
    just fmt-markdown

fmt-check:
    cargo fmt --all -- --check
    just fmt-markdown --check

check-bsp:
    cd bsp && cargo check

# Build the aarch64 initramfs with dropbear SSH server.
initramfs:
    just setup-keys
    {{python_cmd}} scripts/initramfs.py --arch aarch64

initramfs-force:
    just setup-keys
    {{python_cmd}} scripts/initramfs.py --arch aarch64 --force

# ── Run / Debug ─────────────────────────────────────────────
#
# Launch QEMU for interactive use.
#   - aarch64: SDL window (virtio-gpu) + SSH (dropbear, port 2222)
#   - x86_64 / riscv64 / loongarch64: serial console (cargo osdk run)
#
# Usage:
#   just run              # host arch (auto-detected)
#   just run aarch64      # ARM64 with SDL window + SSH
#   just run x86_64       # x86_64 serial console
#   just run riscv64      # RISC-V serial console
#   just run headless     # aarch64 without GUI (SSH only)

# Launch QEMU. Defaults to host architecture; pass ARCH to override.
[script('bash')]
run ARCH="":
    set -e
    ARG="{{ARCH}}"
    if [ -z "$ARG" ]; then
        # Auto-detect host architecture
        HOST_ARCH=$(uname -m)
        case "$HOST_ARCH" in
            x86_64|amd64)  ARG="x86_64" ;;
            aarch64|arm64) ARG="aarch64" ;;
            riscv64)       ARG="riscv64" ;;
            loongarch64)   ARG="loongarch64" ;;
            *)             ARG="x86_64" ;;
        esac
        echo "[run] Auto-detected host arch: $HOST_ARCH → $ARG"
    fi

    if [ "$ARG" = "headless" ]; then
        ARG="aarch64"
        HEADLESS=1
    else
        HEADLESS=0
    fi

    echo ""
    echo "═══════════════════════════════════════════════════════"
    echo "  kei VM — Architecture: $ARG"
    echo "═══════════════════════════════════════════════════════"
    echo ""

    case "$ARG" in
        aarch64)
            just _run-aarch64 "$HEADLESS"
            ;;
        x86_64)
            just _run-x86_64
            ;;
        riscv64)
            just _run-riscv64
            ;;
        loongarch64)
            just _run-loongarch64
            ;;
        *)
            echo "Unsupported arch: $ARG"
            echo "Supported: aarch64, x86_64, riscv64, loongarch64"
            echo "  just run aarch64    — ARM64 with SDL window + SSH"
            echo "  just run x86_64     — x86_64 with serial console"
            echo "  just run riscv64    — RISC-V with serial console"
            echo "  just run headless   — aarch64 without GUI"
            exit 1
            ;;
    esac

# Internal: launch aarch64 QEMU.
[script('bash')]
_run-aarch64 HEADLESS:
    set -e
    HEADLESS="{{HEADLESS}}"

    # Ensure SSH keys exist
    just setup-keys

    # Ensure kernel is built
    if [ ! -f target/osdk/aster-kernel/aster-kernel-osdk-bin.image ]; then
        echo "[run] Kernel image not found, building..."
        just _build-aarch64
    fi

    # Kill any existing QEMU
    taskkill //F //IM qemu-system-aarch64.exe 2>/dev/null || true
    pkill -9 -f qemu-system-aarch64 2>/dev/null || true
    sleep 1

    # Determine display mode
    if [ "$HEADLESS" = "1" ]; then
        DISPLAY_OPT="-display none"
        echo "[run] Headless mode (no GUI window)"
    else
        DISPLAY_OPT="-display sdl"
        echo "[run] SDL window mode (GUI terminal)"
    fi

    echo ""

    # Print SSH info BEFORE launching QEMU
    just ssh-info

    echo "  Serial log: target/qemu_serial.log"
    echo "  Kernel:     target/osdk/aster-kernel/aster-kernel-osdk-bin.image"
    echo ""

    # Convert paths for Windows QEMU
    WINIMAGE=$(cygpath -w "target/osdk/aster-kernel/aster-kernel-osdk-bin.image" 2>/dev/null || echo "target/osdk/aster-kernel/aster-kernel-osdk-bin.image")
    # Use the aris-rendered UI initramfs if RENDER_UI=1, else the SSH/shell initramfs.
    if [ "$RENDER_UI" = "1" ]; then
        INITRAMFS_PATH="tests/initramfs/build/initramfs_render_new.cpio.gz"
        echo "[run] Using aris-rendered UI initramfs"
    else
        INITRAMFS_PATH="tests/initramfs/build/initramfs_aarch64.cpio.gz"
    fi
    WININITRD=$(cygpath -w "$INITRAMFS_PATH" 2>/dev/null || echo "$INITRAMFS_PATH")
    WINLOG=$(cygpath -w "target/qemu_serial.log" 2>/dev/null || echo "target/qemu_serial.log")

    # Launch QEMU in the foreground. The SDL window appears, and the terminal
    # stays attached. Press Ctrl+C or close the window to stop.
    # -monitor tcp: provides a HMP monitor on port 55555 for screendump etc.
    # MSYS_NO_PATHCONV=1 prevents Git Bash from mangling /init.
    echo "[run] Launching QEMU (Ctrl+C or close window to stop)..."
    echo "[run] Monitor: tcp://127.0.0.1:55555 (use 'just screenshot' to capture)"
    echo ""
    export MSYS_NO_PATHCONV=1; exec "/c/Program Files/qemu/qemu-system-aarch64.exe" \
        -cpu cortex-a72 -machine virt,gic-version=3,virtualization=on \
        -m 2G -smp 1 --no-reboot \
        $DISPLAY_OPT \
        -device virtio-gpu-device \
        -device virtio-keyboard-device \
        -serial file:"$WINLOG" \
        -monitor tcp:127.0.0.1:55555,server,nowait \
        -netdev user,id=net0,hostfwd=tcp::2222-:22 \
        -device virtio-net-device,netdev=net0 \
        -kernel "$WINIMAGE" \
        -initrd "$WININITRD" \
        -append "init=/init SHELL=/bin/sh LOGNAME=root HOME=/ USER=root PATH=/bin:/sbin"

# Internal: launch x86_64 QEMU via cargo osdk run.
[script('bash')]
_run-x86_64:
    set -e
    echo "[run] x86_64 uses 'cargo osdk run' with serial console"
    echo "[run] No SSH server on x86_64 (uses serial shell)"
    echo ""
    cargo osdk run --target x86_64-unknown-none

# Internal: launch RISC-V QEMU via cargo osdk run.
[script('bash')]
_run-riscv64:
    set -e
    echo "[run] RISC-V uses 'cargo osdk run' with serial console"
    echo "[run] No SSH server on RISC-V (uses serial shell)"
    echo ""
    cargo osdk run --scheme riscv --target-arch riscv64

# Internal: launch LoongArch QEMU via cargo osdk run.
[script('bash')]
_run-loongarch64:
    set -e
    echo "[run] LoongArch uses 'cargo osdk run' with serial console"
    echo "[run] No SSH server on LoongArch (uses serial shell)"
    echo ""
    cargo osdk run --scheme loongarch --target-arch loongarch64

# ── WSL2 QEMU (headless, screenshot-driven) ────────────────
#
# Run kei in WSL2's qemu-system-aarch64 in headless mode and capture
# the display via QEMU monitor screendump. This is the primary path
# for automated CI and screenshot analysis on Windows hosts, since
# WSL2 QEMU avoids the SDL/GUI overhead and the CJK-path-in-WSL blocker
# is sidestepped via the ~/celestia/kei ASCII symlink.
#
# Recipes use the `wslq-` prefix to avoid colliding with the shared
# `wsl-run` recipe in celestia-devtools.just.
#
# Usage:
#   just wslq-run               # run aarch64 headless (100s)
#   just wslq-run 60            # run for 60 seconds
#   just wslq-ui                # run with aris-render kei_ui initramfs
#   just wslq-screenshot        # convert last screendump to PNG

# Run kei aarch64 in WSL2 QEMU headless. Optional SECS (default 100).
# Uses INITRAMFS env var to select the initramfs (default: render_new).
[script('bash')]
wslq-run SECS="100":
    set -e
    export INITRAMFS="${INITRAMFS:-tests/initramfs/build/initramfs_render_new.cpio.gz}"
    wsl -d Ubuntu-24.04 -e bash -lc 'bash ~/celestia/kei/scripts/wsl_qemu_aarch64.sh {{SECS}}'

# Run kei with the kei_ui (aris-render browser UI) initramfs.
[script('bash')]
wslq-ui SECS="110":
    set -e
    export INITRAMFS="tests/initramfs/build/initramfs_kei_ui.cpio.gz"
    wsl -d Ubuntu-24.04 -e bash -lc 'bash ~/celestia/kei/scripts/wsl_qemu_aarch64.sh {{SECS}}'

# Build a render initramfs (kei_ui or kei_fbtest).
[script('bash')]
wslq-initramfs BIN="kei_ui":
    {{python_cmd}} scripts/build_render_initramfs.py {{BIN}}

# Convert the last WSL2 screendump to PNG and show pixel stats.
[script('bash')]
wslq-screenshot:
    set -e
    wsl -d Ubuntu-24.04 -e bash -lc 'cd ~/celestia/kei && python3 scripts/ppm_to_png.py target/wsl_screendump.ppm target/wsl_screendump.png'
    @ls -la target/wsl_screendump.png 2>/dev/null || echo "[wslq-screenshot] no screendump yet"

# Ensure the ~/celestia/kei ASCII symlink exists (bypasses CJK-path blocker).
[script('bash')]
wslq-setup:
    wsl -d Ubuntu-24.04 -e bash -lc 'mkdir -p ~/celestia && ln -sfn "/mnt/d/源代码/工程项目/celestia/kei" ~/celestia/kei && echo "symlink OK: ~/celestia/kei"'

# ── Screenshot ──────────────────────────────────────────────
#
# Capture the QEMU display to a PNG file via the QEMU monitor's
# 'screendump' command. Requires QEMU to be running with
# -monitor tcp:127.0.0.1:55555 (added automatically by 'just run').

# Capture a screenshot of the running QEMU display.
# Usage: just screenshot [filename]
[script('bash')]
screenshot FILE="target/screenshot.ppm":
    set -e
    OUT="{{FILE}}"
    # Ensure .ppm extension for QEMU compatibility
    case "$OUT" in
        *.ppm) ;;
        *) OUT="$OUT.ppm" ;;
    esac
    WOUT=$(cygpath -w "$OUT" 2>/dev/null || echo "$OUT")

    echo "[screenshot] Capturing QEMU display to $OUT ..."

    # Send 'screendump' to QEMU monitor via TCP
    # The monitor expects commands terminated by newline.
    printf 'screendump %s\n' "$WOUT" | \
        "/c/Program Files/qemu/qemu-system-aarch64.exe" -qmp stdout 2>/dev/null || true

    # Alternative: use a simple TCP connection (bash /dev/tcp)
    # This works on Git Bash (MSYS2) with /dev/tcp support
    if [ ! -f "$OUT" ]; then
        echo "[screenshot] /dev/tcp method..."
        exec 3<>/dev/tcp/127.0.0.1/55555 || {
            echo "[screenshot] ERROR: Cannot connect to QEMU monitor on port 55555"
            echo "[screenshot] Make sure 'just run' is running."
            exit 1
        }
        # Read banner
        read -t 2 -u 3 line || true
        # Send screendump
        echo "screendump $WOUT" >&3
        # Read response
        read -t 5 -u 3 line || true
        exec 3>&-
    fi

    if [ -f "$OUT" ]; then
        SIZE=$(wc -c < "$OUT" 2>/dev/null || echo 0)
        echo "[screenshot] Saved $OUT ($SIZE bytes)"

        # Try converting PPM to PNG if ImageMagick is available
        if command -v convert &>/dev/null; then
            PNG="${OUT%.ppm}.png"
            convert "$OUT" "$PNG" 2>/dev/null && {
                echo "[screenshot] Converted to $PNG"
                rm -f "$OUT"
            }
        elif command -v python3 &>/dev/null || command -v python &>/dev/null; then
            PNG="${OUT%.ppm}.png"
            PYTHON=$(command -v python3 || command -v python)
            "$PYTHON" scripts/ppm_info.py "$OUT" 2>/dev/null && echo "[screenshot] PPM validated"
        fi
    else
        echo "[screenshot] ERROR: Screenshot file not created."
        echo "[screenshot] The QEMU monitor screendump may not support the path."
        echo "[screenshot] Try: just screenshot target/screenshot"
    fi

# Connect to the running aarch64 VM via SSH.
ssh:
    @echo "Connecting to kei VM via SSH..."
    ssh -i tests/initramfs/build/client_ssh_key \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -p 2222 root@127.0.0.1

# Stop the running QEMU instance.
[script('bash')]
kill:
    taskkill //F //IM qemu-system-aarch64.exe 2>/dev/null || true
    taskkill //F //IM qemu-system-x86_64.exe 2>/dev/null || true
    taskkill //F //IM qemu-system-riscv64.exe 2>/dev/null || true
    pkill -9 -f qemu-system 2>/dev/null || true
    echo "QEMU stopped."

# Show the serial log (boot messages).
log:
    @tail -50 target/qemu_serial.log 2>/dev/null || echo "No serial log found. Run 'just run' first."

# Watch the serial log in real-time.
log-follow:
    @tail -f target/qemu_serial.log 2>/dev/null || echo "No serial log found."

# ── Test ───────────────────────────────────────────────────

test-all:
    {{python_cmd}} scripts/test_all_arch.py

test-arch ARCH:
    {{python_cmd}} scripts/test_all_arch.py {{ARCH}}

test BOARD="nanopi-r3s":
    {{python_cmd}} scripts/test.py {{BOARD}}

test-bsp:
    cargo test -p bsp-rk3566 -p bsp-bcm2711 -p bsp-jh7110

# ── Utilities ──────────────────────────────────────────────

list-boards:
    ls configs/*.toml | grep -v default | xargs -I{} basename {} .toml

# List all supported architectures and their run commands.
list-arch:
    @echo ""
    @echo "kei supported architectures:"
    @echo ""
    @echo "  Host arch (auto-detected by 'just run')"
    @echo "               just run              — auto: x86_64 on PC, aarch64 on ARM"
    @echo ""
    @echo "  aarch64      ARM64 (QEMU virt) — SDL window + SSH (port 2222)"
    @echo "               just run aarch64"
    @echo "               just run headless   (no GUI, SSH only)"
    @echo "               just ssh            (connect)"
    @echo ""
    @echo "  x86_64       x86-64 (QEMU pc) — serial console"
    @echo "               just run x86_64"
    @echo ""
    @echo "  riscv64      RISC-V (QEMU virt) — serial console"
    @echo "               just run riscv64"
    @echo ""
    @echo "  loongarch64  LoongArch (QEMU virt) — serial console"
    @echo "               just run loongarch64"
    @echo ""
    @echo "Other commands:"
    @echo "  just setup-keys    Generate SSH keys (one-time)"
    @echo "  just ssh-info      Show SSH connection details"
    @echo "  just kill          Stop QEMU"
    @echo "  just log           Show boot log"
    @echo "  just log-follow    Follow boot log live"
    @echo ""

clean:
    rm -rf build/ output/
    cargo clean
    rm -f target/qemu_serial.log target/qemu.pid target/client_ssh_key 2>/dev/null || true

dev-shell:
    {{python_cmd}} scripts/dev_shell.py
