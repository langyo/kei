# 보드 지원 패키지 가이드

## 개요

kei의 BSP(보드 지원 패키지)는 특정 SoC 플랫폼용 디바이스 드라이버를 제공하는
`#![no_std]` Rust crate입니다. BSP는 OSDK 라이브러리 crate로 빌드되어
패치된 Asterinas 커널에 링크됩니다.

## 새 BSP 만들기

### 1. 템플릿 복사

```bash
cp -r bsp/rk3566 bsp/mysoc
```

### 2. Cargo.toml 업데이트

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

### 3. 드라이버 모듈 구현

각 모듈은 ostd 디바이스 모델을 따릅니다:

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

### 4. workspace에 등록

`bsp/Cargo.toml`에 추가합니다:

```toml
members = [
    "rk3566",
    "bcm2711",
    "jh7110",
    "mysoc",       # <-- add here
]
```

### 5. 보드 설정 추가

`configs/myboard.toml`을 생성합니다:

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

### 6. 디바이스 트리 추가

`board/myboard/device-tree/mysoc-myboard.dts`를 생성합니다.

## 드라이버 패턴

### 메모리 맵 I/O(MMIO)

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

### 인터럽트 처리

ostd IRQ 서브시스템을 통해 인터럽트 핸들러를 등록합니다:

```rust
// TODO: use ostd::irq::register_handler(cpu_id, vector, handler)
```

### 디바이스 트리

FDT(플랫 디바이스 트리)를 파싱하여 디바이스를 검색합니다:

```rust
// TODO: use ostd::device::fdt module
```

## 테스트

하드웨어에 배포하기 전에 QEMU에서 BSP를 테스트합니다:

```bash
just build   # Build with your BSP
just test    # Boot in QEMU, check console output
```

하드웨어 테스트를 위해서는 kei 커널이 포함된 aris 펌웨어를 플래시합니다.
