# kei ビルドとデプロイ

## 概要

kei は `kei-kernel.bin` — ARM64 対応 Asterinas カーネルを生成します。この
ガイドでは、カーネルのビルド、QEMU でのテスト、物理ハードウェアへの展開を
説明します。

## ビルドパイプライン

```mermaid
flowchart LR
    SRC["Source\nostd/ kernel/ bsp/"] -->|"cargo build\n(aarch64)"| BIN["kei-kernel.bin"]
    BIN --> QEMU["QEMU Test\n(virt/cortex-a55)"]
    QEMU -->|passes| PACK["Package\n(DTB + initramfs)"]
    PACK --> FLASH["Flash SD card"]
    FLASH --> BOARD["NanoPi R3S"]
```

## 前提条件

- **ホスト**: Linux x86_64 または ARM64
- **Rust**: 1.85+、`aarch64-unknown-none-softfloat` ターゲット付き
- **QEMU**: ≥ 8.0、cortex-a55 搭載 virt マシン用
- **just**: `cargo install just`

## クイックビルド

```bash
# One-time setup
just setup        # Configure git remotes and Rust targets

# Sync upstream sources
just vendor       # Absorb latest upstream asterinas (squash)
just versions     # Show upstream baseline versions

# Build for the NanoPi R3S
just build        # Builds kei-kernel.bin for aarch64/armv8

# Run QEMU boot tests
just test-all     # Boot-tests all supported architectures
```

## クロスコンパイル

x86_64 から aarch64 へのクロスコンパイルの場合：

```bash
# Add the ARM64 target (one-time)
rustup target add aarch64-unknown-none-softfloat

# Install GCC cross-toolchain (distribution-dependent)
# Ubuntu / Debian:
sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu

# Build
cargo build --release --target aarch64-unknown-none-softfloat \
  -p kei-kernel
```

カーネルバイナリは生の ARM64 Image（Linux ブートプロトコル）であり、ELF
ではありません。U-Boot から `booti` コマンドで直接起動します。

## QEMU テスト

ハードウェアに展開する前に、QEMU でカーネルをテストします：

```mermaid
flowchart TB
    subgraph Host["ホストマシン"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        QEMU["QEMU\n(virt, cortex-a55)"]
    end
    KERN --> QEMU
    DTB --> QEMU
    QEMU -->|"serial output\n(logged)"| LOG["コンソールログ"]
    QEMU -->|"exit 0 = pass"| RESULT["テスト結果"]
```

### テストマトリックス

| QEMU マシン | CPU | RAM | 状態 | コマンド |
|-------------|-----|-----|--------|---------|
| virt | cortex-a55 | 2GB | ✅ 主要 | `just test` |
| virt | cortex-a72 | 2GB | 🔲 予定 | — |
| virt | max | 4GB | 🔲 予定 | — |
| sbsa-ref | max | 4GB | 🔲 予定 | — |

```bash
# Run the primary test target
just test

# Manual QEMU invocation
qemu-system-aarch64 \
  -machine virt,gic-version=3 \
  -cpu cortex-a55 \
  -m 2G \
  -kernel output/kei-kernel.bin \
  -nographic
```

## 物理展開

### NanoPi R3S

kei を物理 NanoPi R3S に展開する：

```mermaid
flowchart TB
    subgraph Build["ビルドホスト"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        INIT["initramfs.cpio.gz"]
    end
    subgraph Deploy["展開"]
        IMG["image.img"]
        SD["SD カード"]
        BOARD["NanoPi R3S"]
    end
    KERN --> IMG
    DTB --> IMG
    INIT --> IMG
    IMG -->|"dd / just flash-sd"| SD
    SD --> BOARD
```

### SD カードへの書き込み

```bash
# Build the complete firmware image (includes kei-kernel.bin)
just build-board nanopi-r3s

# Flash to SD card
sudo dd if=output/nanopi-r3s/image.img of=/dev/sdX bs=4M status=progress
sync
```

### 起動検証

SD カードを挿入して電源を投入した後、USB-TTL シリアル（1500000 ボー、
8N1）で接続します：

```
U-Boot 2024.01 (Jan 01 2024 - 00:00:00 +0000)
...
## Loading kernel from mmc 0:1
   Image Name:   kei-kernel
   Image Type:   AArch64 Linux Kernel Image
   Data Size:    4194304 Bytes = 4 MiB
   Load Address: 00000000
   Entry Point:  00000000
## Flattened Device Tree blob at 44000000
   Booting using the fdt blob at 0x44000000

kei-kernel booting...
[KEI] initialising GICv3...
[KEI] initialising ARM Generic Timer...
[KEI] starting SMP...
[KEI] 4 cores online
...
```

### 起動順序

```mermaid
flowchart TB
    ROM["Mask ROM"] --> SPL["U-Boot SPL"]
    SPL --> TPL["U-Boot Proper"]
    TPL -->|"load kernel + DTB\nfrom mmc"| KEI["kei-kernel.bin"]
    KEI -->|"Transfer to EL1"| INIT["kei init\n(ユーザー空間)"]
```

## トラブルシューティング

| 症状 | 考えられる原因 | 対処 |
|---------|-------------|--------|
| シリアル出力なし | ボーレートが間違っている | 115200 ではなく 1500000 を使用 |
| GICv3 初期化失敗 | QEMU マシン種別 | `virt,gic-version=3` を使用 |
| SMP 失敗 | DTB に PSCI がない | デバイスツリーの `/cpus` ノードを確認 |
| Kernel panic | アーキテクチャ層のコードバグ | `ostd/src/arch/aarch64/` を監査 |
| U-Boot がカーネルを見つけられない | パーティションオフセットが間違い | `boot.scr` のオフセットを確認 |
