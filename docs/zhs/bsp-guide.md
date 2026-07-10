# 板级支持包指南

## 概述

kei 中的 BSP（板级支持包）是一个 `#![no_std]` Rust crate，
为特定 SoC 平台提供设备驱动。BSP 作为 OSDK 库 crate 构建，
并链接到打过补丁的 Asterinas 内核中。

## 创建新 BSP

### 1. 复制模板

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

### 3. 实现驱动模块

每个模块遵循 ostd 设备模型：

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

### 4. 在 workspace 中注册

添加到 `bsp/Cargo.toml`：

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. 添加板级配置

创建 `configs/myboard.toml`：

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

### 6. 添加设备树

创建 `board/myboard/device-tree/mysoc-myboard.dts`。

## 驱动模式

### 内存映射 I/O（MMIO）

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

### 中断处理

通过 ostd IRQ 子系统注册中断处理程序：

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### 设备树

解析 FDT（扁平设备树）以发现设备：

```rust
// TODO: use ostd::device::fdt module
```

## 测试

在部署到硬件之前，先在 QEMU 中测试 BSP：

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

若要进行硬件测试，请烧录包含 kei 内核的 aris 固件。
