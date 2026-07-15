//! StarFive JH7110 BSP — VisionFive 2 (RISC-V 64).
//!
//! **Status (2026-07-14): skeleton — no drivers implemented yet.**
//!
//! Planned support. Drivers TBD:
//! - JH7110 GPIO
//! - DesignWare GMAC Ethernet
//! - NS16550 UART
//! - DesignWare SPI/I2C
//!
//! Note: riscv64 is Tier 2 in upstream Asterinas, so
//! this BSP may be buildable before ARM64 is merged.
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
/// but no jh7110 hardware will be initialised. Either implement the first driver
/// (start with GPIO) or do not select `bsp-jh7110` in your board config.
///
/// A deprecation warning is emitted on use so the link is never silent. The
/// warning is the canonical "this BSP is not ready" signal — see
/// `packages/bsp/README.md` for the completion matrix.
#[deprecated(
    since = "0.1.0",
    note = "bsp-jh7110 is a skeleton (no drivers). Implement GPIO/etc. before \
            linking. See packages/bsp/README.md."
)]
pub fn init() {}
