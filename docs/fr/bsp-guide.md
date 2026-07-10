# Guide des Board Support Packages

## Vue d'ensemble

Un BSP (Board Support Package) dans kei est un crate Rust `#![no_std]` qui
fournit des pilotes de périphériques pour une plateforme SoC spécifique. Les BSP
sont construits comme des crates de bibliothèque OSDK et liés au noyau Asterinas
patché.

## Créer un nouveau BSP

### 1. Copier le modèle

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Mettre à jour Cargo.toml

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

### 3. Implémenter les modules de pilote

Chaque module suit le modèle de périphérique ostd :

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

### 4. Enregistrer dans le workspace

Ajouter à `bsp/Cargo.toml` :

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. Ajouter la configuration de carte

Créer `configs/myboard.toml` :

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

### 6. Ajouter l'arbre des périphériques

Créer `board/myboard/device-tree/mysoc-myboard.dts`.

## Modèles de pilote

### E/S mappée en mémoire (MMIO)

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

### Gestion des interruptions

Enregistrer les gestionnaires d'interruption via le sous-système IRQ d'ostd :

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### Arbre des périphériques

Analyser le FDT (Flattened Device Tree) pour découvrir les périphériques :

```rust
// TODO: use ostd::device::fdt module
```

## Tests

Tester les BSP dans QEMU avant le déploiement sur matériel :

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

Pour les tests sur matériel, flasher le firmware aris qui inclut le noyau kei.
