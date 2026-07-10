# Guía de Board Support Package

## Visión general

Un BSP (Board Support Package) en kei es un crate de Rust `#![no_std]` que
proporciona drivers de dispositivos para una plataforma SoC específica. Los BSP
se construyen como crates de biblioteca OSDK y se enlazan con el kernel de
Asterinas parcheado.

## Crear un nuevo BSP

### 1. Copiar la plantilla

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Actualizar Cargo.toml

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

### 3. Implementar los módulos de driver

Cada módulo sigue el modelo de dispositivos de ostd :

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

### 4. Registrar en el workspace

Añadir a `bsp/Cargo.toml` :

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. Añadir la configuración de placa

Crear `configs/myboard.toml` :

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

### 6. Añadir el árbol de dispositivos

Crear `board/myboard/device-tree/mysoc-myboard.dts`.

## Patrones de driver

### E/S mapeada en memoria (MMIO)

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

### Manejo de interrupciones

Registrar los manejadores de interrupción vía el subsistema IRQ de ostd :

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### Árbol de dispositivos

Analizar el FDT (Flattened Device Tree) para descubrir dispositivos :

```rust
// TODO: use ostd::device::fdt module
```

## Tests

Probar los BSP en QEMU antes de desplegar en hardware :

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

Para pruebas en hardware, flashear el firmware aris que incluye el kernel kei.
