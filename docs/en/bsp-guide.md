# Board Support Package Guide

## Overview

A BSP (Board Support Package) in kei is a `#![no_std]` Rust crate that
provides device drivers for a specific SoC platform. BSPs are built as
OSDK library crates and linked into the patched Asterinas kernel.

## Creating a New BSP

### 1. Copy the template

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Update Cargo.toml

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

### 3. Implement driver modules

Each module follows the ostd device model:

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

### 4. Register in workspace

Add to `bsp/Cargo.toml`:

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. Add board config

Create `configs/myboard.toml`:

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

### 6. Add device tree

Create `board/myboard/device-tree/mysoc-myboard.dts`.

## Driver Patterns

### Memory-Mapped I/O (MMIO)

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

### Interrupt Handling

Register interrupt handlers via the ostd IRQ subsystem:

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### Device Tree

Parse FDT (Flattened Device Tree) to discover devices:

```rust
// TODO: use ostd::device::fdt module
```

## Testing

Test BSPs in QEMU before deploying to hardware:

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

For hardware testing, flash aris firmware that includes the kei kernel.
