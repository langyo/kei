# ボードサポートパッケージガイド

## 概要

kei の BSP（ボードサポートパッケージ）は、特定の SoC プラットフォーム向けのデバイスドライバを
提供する `#![no_std]` Rust crate です。BSP は OSDK ライブラリ crate としてビルドされ、
パッチを当てた Asterinas カーネルにリンクされます。

## 新しい BSP の作成

### 1. テンプレートをコピー

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Cargo.toml を更新

```toml
[package]
name = "bsp-mysoc"
version.workspace = true
edition.workspace = true
description = "My SoC Board Support Package for Asterinas/kei"

[dependencies]
# When built in the kernel workspace, depend on ostd
# ostd = { path = "../../vendor/asterinas/ostd" }
```

### 3. ドライバモジュールを実装

各モジュールは ostd デバイスモデルに従います：

```
src/
├── lib.rs        # init() entry point
├── gpio.rs       # GPIO controller driver
├── ethernet.rs   # Ethernet MAC driver
├── uart.rs       # UART console driver
├── watchdog.rs   # Hardware watchdog
├── spi.rs        # SPI master
└── i2c.rs        # I2C master
```

### 4. workspace に登録

`bsp/Cargo.toml` に追加します：

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. ボード設定を追加

`configs/myboard.toml` を作成します：

```toml
[board]
name = "myboard"
soc = "mysoc"
arch = "aarch64"

[kernel]
asterinas_version = "0.18.0"
bsp_crate = "bsp-mysoc"
dtb = "mysoc-myboard"
features = ["aarch64"]
```

### 6. デバイスツリーを追加

`board/myboard/device-tree/mysoc-myboard.dts` を作成します。

## ドライバパターン

### メモリマップト I/O（MMIO）

```rust
use core::ptr::{read_volatile, write_volatile};

const UART_BASE: usize = 0xFE660000;

fn uart_write(reg_offset: usize, value: u32) {
    unsafe {
        write_volatile((UART_BASE + reg_offset) as *mut u32, value);
    }
}

fn uart_read(reg_offset: usize) -> u32 {
    unsafe { read_volatile((UART_BASE + reg_offset) as *const u32) }
}
```

### 割り込み処理

ostd IRQ サブシステム経由で割り込みハンドラを登録します：

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### デバイスツリー

FDT（フラットデバイスツリー）をパースしてデバイスを検出します：

```rust
// TODO: use ostd::device::fdt module
```

## テスト

ハードウェアにデプロイする前に、QEMU で BSP をテストします：

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

ハードウェアテストでは、kei カーネルを含む aris ファームウェアを書き込みます。
