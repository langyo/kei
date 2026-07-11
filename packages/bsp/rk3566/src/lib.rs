//! Rockchip RK3566 Board Support Package for Asterinas/kei.
//!
//! Provides device drivers and platform initialization for the
//! RK3566 SoC used on boards like NanoPi R3S and OrangePi 3B.
//!
//! ## Features
//!
//! - GPIO (pinctrl via Rockchip GRF)
//! - Dual Gigabit Ethernet (stmmac / RK GMAC)
//! - UART (DW 8250-compatible)
//! - SPI Master (DesignWare)
//! - I2C Master (RK3x)
//! - Hardware Watchdog (DW WDT)
//! - SD/eMMC (DW MMC)
//! - Power Management / Clock Tree (RK3566 CRU)

#![no_std]
#![feature(asm_const)]

extern crate alloc;

pub mod ethernet;
pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;
pub mod watchdog;

/// Initialize all RK3566 platform devices.
///
/// Called early in kernel boot after MMU and interrupt controller setup.
/// Registers device drivers with the ostd device model.
pub fn init() {
    // TODO: probe device tree, init clock tree, register drivers
}
