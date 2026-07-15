//! Broadcom BCM2711 BSP — Raspberry Pi 4 / CM4.
//!
//! **Status (2026-07-14): skeleton — no drivers implemented yet.**
//!
//! Planned support. Drivers TBD:
//! - BCM2711 GPIO
//! - BCM GENET Ethernet
//! - PL011 UART
//! - BCM2835 SPI/I2C
//!
//! See `packages/bsp/README.md` for the BSP completion matrix.
//!
//! The `init` function is intentionally a no-op placeholder. Linking this crate
//! into a kernel build will produce a deprecation warning at compile time so the
//! choice is never silent.

#![no_std]

/// Placeholder init. **Does nothing** — this BSP has no drivers.
///
/// Calling this from a real board is a configuration error: the kernel will boot
/// but no bcm2711 hardware will be initialised. Either implement the first driver
/// (start with GPIO) or do not select `bsp-bcm2711` in your board config.
///
/// A deprecation warning is emitted on use so the link is never silent. The
/// warning is the canonical "this BSP is not ready" signal — see
/// `packages/bsp/README.md` for the completion matrix.
#[deprecated(
    since = "0.1.0",
    note = "bsp-bcm2711 is a skeleton (no drivers). Implement GPIO/etc. before \
            linking. See packages/bsp/README.md."
)]
pub fn init() {}
