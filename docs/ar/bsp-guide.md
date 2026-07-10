# دليل حزمة دعم اللوحة

## نظرة عامة

حزمة دعم اللوحة (BSP) في kei هي crate من نوع `#![no_std]` بلغة Rust
توفّر مُشغّلات الأجهزة لمنصة SoC محدّدة. تُبنى حزم BSP كـ crates مكتبية
من نوع OSDK وتُربط بنواة Asterinas المُعدَّلة.

## إنشاء BSP جديد

### 1. نسخ القالب

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. تحديث Cargo.toml

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

### 3. تنفيذ وحدات المُشغّلات

تتبع كل وحدة نموذج أجهزة ostd :

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

### 4. التسجيل في workspace

أضِف إلى `bsp/Cargo.toml` :

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. إضافة إعداد اللوحة

أنشئ `configs/myboard.toml` :

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

### 6. إضافة شجرة الأجهزة

أنشئ `board/myboard/device-tree/mysoc-myboard.dts`.

## أنماط المُشغّلات

### الإدخال/الإخراج المُعنون بالذاكرة (MMIO)

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

### معالجة المقاطعات

تسجيل معالجات المقاطعات عبر نظام IRQ الفرعي في ostd :

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### شجرة الأجهزة

تحليل FDT (Flattened Device Tree) لاكتشاف الأجهزة :

```rust
// TODO: use ostd::device::fdt module
```

## الاختبار

اختبر حزم BSP في QEMU قبل النشر على العتاد :

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

لاختبار العتاد، ومض البرنامج الثابت aris الذي يتضمّن نواة kei.
