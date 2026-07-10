# kei 建置與部署

## 概述

kei 生成 `kei-kernel.bin` — [aris](https://github.com/celestia-island/aris)
所使用的 ARM64 支援的 Asterinas 核心。本指南涵蓋核心的建置、QEMU 測試以及
部署到實體硬體。

## 建置管線

```mermaid
flowchart LR
    SRC["Source\nostd/ kernel/ bsp/"] -->|"cargo build\n(aarch64)"| BIN["kei-kernel.bin"]
    BIN --> QEMU["QEMU Test\n(virt/cortex-a55)"]
    QEMU -->|passes| PACK["Package\n(DTB + initramfs)"]
    PACK --> ARIS["aris firmware\n(image.img)"]
    ARIS --> FLASH["Flash SD card"]
    FLASH --> BOARD["NanoPi R3S"]
```

## 先決條件

- **主機**: Linux x86_64 或 ARM64
- **Rust**: 1.85+，含 `aarch64-unknown-none-softfloat` 目標
- **QEMU**: ≥ 8.0，用於 cortex-a55 的 virt 機器
- **just**: `cargo install just`

## 快速建置

```bash
# One-time setup
just setup        # Configure git remotes and Rust targets

# Sync upstream sources
just vendor       # Absorb latest upstream asterinas (squash)
just pull-arm64   # Pull ARM64 code from wanywhn fork (one-time)
just versions     # Show upstream baseline versions

# Build for the NanoPi R3S
just build        # Builds kei-kernel.bin for aarch64/armv8

# Run QEMU boot tests
just test-all     # Boot-tests all supported architectures
```

## 交叉編譯

從 x86_64 交叉編譯到 aarch64：

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

核心二進位檔案是原始 ARM64 Image（Linux 啟動協定），而非 ELF。它透過
`booti` 命令直接從 U-Boot 啟動。

## QEMU 測試

在部署到硬體之前，請在 QEMU 中測試核心：

```mermaid
flowchart TB
    subgraph Host["主機"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        QEMU["QEMU\n(virt, cortex-a55)"]
    end
    KERN --> QEMU
    DTB --> QEMU
    QEMU -->|"serial output\n(logged)"| LOG["控制台日誌"]
    QEMU -->|"exit 0 = pass"| RESULT["測試結果"]
```

### 測試矩陣

| QEMU 機器 | CPU | RAM | 狀態 | 命令 |
|-------------|-----|-----|--------|---------|
| virt | cortex-a55 | 2GB | ✅ 主要 | `just test` |
| virt | cortex-a72 | 2GB | 🔲 計畫中 | — |
| virt | max | 4GB | 🔲 計畫中 | — |
| sbsa-ref | max | 4GB | 🔲 計畫中 | — |

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

## 實體部署

### NanoPi R3S

將 kei 部署到實體 NanoPi R3S：

```mermaid
flowchart TB
    subgraph Build["建置主機"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        INIT["initramfs.cpio.gz"]
    end
    subgraph Deploy["部署"]
        IMG["image.img"]
        SD["SD 卡"]
        BOARD["NanoPi R3S"]
    end
    KERN --> IMG
    DTB --> IMG
    INIT --> IMG
    IMG -->|"dd / just flash-sd"| SD
    SD --> BOARD
```

### 燒錄到 SD 卡

```bash
# Build the complete firmware image (includes kei-kernel.bin)
# Run from aris repository — aris packages kei as a submodule/dependency
just build-board nanopi-r3s

# Flash to SD card
sudo dd if=output/nanopi-r3s/image.img of=/dev/sdX bs=4M status=progress
sync
```

### 啟動驗證

插入 SD 卡並上電後，透過 USB-TTL 序列埠（1500000 鮑率，8N1）連接：

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
aris-core v0.1.0 starting...
evernight daemon starting...
```

### 啟動順序

```mermaid
flowchart TB
    ROM["Mask ROM"] --> SPL["U-Boot SPL"]
    SPL --> TPL["U-Boot Proper"]
    TPL -->|"load kernel + DTB\nfrom mmc"| KEI["kei-kernel.bin"]
    KEI -->|"Transfer to EL1"| INIT["initramfs\naris-core (PID 1)"]
    INIT --> EVN["evernight daemon"]
    EVN -->|"WebSocket TLS"| ENT["entelecheia"]
```

## 與 aris 整合

kei 提供核心二進位檔案；aris 將其打包為可啟動的映像檔：

```
aris repository                     kei repository
─────────────────                   ─────────────────
packages/core/        supervisor    kernel/          kernel source
packages/builder/     image builder ostd/            core infra
overlay/              rootfs files  bsp/             board support
scripts/              build + flash board/           board configs
│                                    │
│  just build-board                  │  just build
│    ├── cross-compile aris-core     │    └── cargo build (aarch64)
│    ├── fetch kei-kernel.bin        │
│    ├── assemble image.img          │
│    └── just flash-sd /dev/sdX      │
```

驗證整合：

```bash
# In aris repo: build with kei kernel
just build-board nanopi-r3s

# Boot in QEMU with the full image
just test-qemu

# Verify kei kernel version in boot log
grep "kei-kernel" output/boot.log
```

## 故障排除

| 症狀 | 可能原因 | 操作 |
|---------|-------------|--------|
| 無序列埠輸出 | 鮑率錯誤 | 使用 1500000，而非 115200 |
| GICv3 初始化失敗 | QEMU 機器類型 | 使用 `virt,gic-version=3` |
| SMP 失敗 | DTB 中缺少 PSCI | 檢查裝置樹中的 `/cpus` 節點 |
| Kernel panic | LLM 生成的程式碼工件 | 審計 `ostd/src/arch/aarch64/` |
| U-Boot 找不到核心 | 分割區偏移錯誤 | 檢查 `boot.scr` 中的偏移量 |
| evernight 無法連線 | 網路未設定 | 檢查 `/data/network.toml` |
