# 板級支援包指南

## 概述

kei 中的 BSP（板級支援包）是一個 `#![no_std]` Rust crate，
為特定 SoC 平台提供裝置驅動。BSP 作為 OSDK 函式庫 crate 建構，
並連結到打過補丁的 Asterinas 核心中。

## 建立新 BSP

### 1. 複製範本

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. 更新 Cargo.toml

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

### 3. 實作驅動模組

每個模組遵循 ostd 裝置模型：

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

### 4. 在 workspace 中註冊

新增到 `bsp/Cargo.toml`：

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. 新增板級設定

建立 `configs/myboard.toml`：

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

### 6. 新增裝置樹

建立 `board/myboard/device-tree/mysoc-myboard.dts`。

## 驅動模式

### 記憶體映射 I/O（MMIO）

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

### 中斷處理

透過 ostd IRQ 子系統註冊中斷處理常式：

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### 裝置樹

解析 FDT（扁平裝置樹）以發現裝置：

```rust
// TODO: use ostd::device::fdt module
```

## 測試

在部署到硬體之前，先在 QEMU 中測試 BSP：

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

若要進行硬體測試，請燒錄包含 kei 核心的 aris 韌體。
