# Руководство по Board Support Package

## Обзор

BSP (Board Support Package) в kei — это Rust-крейт `#![no_std]`, который
предоставляет драйверы устройств для конкретной платформы SoC. BSP собираются
как библиотечные крейты OSDK и линкуются с пропатченным ядром Asterinas.

## Создание нового BSP

### 1. Скопировать шаблон

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Обновить Cargo.toml

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

### 3. Реализовать модули драйверов

Каждый модуль следует модели устройств ostd :

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

### 4. Зарегистрировать в workspace

Добавить в `bsp/Cargo.toml` :

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. Добавить конфигурацию платы

Создать `configs/myboard.toml` :

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

### 6. Добавить дерево устройств

Создать `board/myboard/device-tree/mysoc-myboard.dts`.

## Шаблоны драйверов

### MMIO (ввод-вывод, отображённый в память)

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

### Обработка прерываний

Регистрировать обработчики прерываний через подсистему IRQ ostd :

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### Дерево устройств

Разбирать FDT (Flattened Device Tree) для обнаружения устройств :

```rust
// TODO: use ostd::device::fdt module
```

## Тестирование

Тестировать BSP в QEMU перед развёртыванием на железе :

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

Для тестирования на железе прошить прошивку aris, включающую ядро kei.
