#![no_std]
//! # kei — no_std embedded bridge library
//!
//! The shared contract layer between embassy-based sensor nodes and the
//! evernight gateway broker.
//!
//! ## Feature flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `wire` | Celestia wire protocol (embassy ↔ gateway framing) |
//! | `manifest` | Hardware manifest schema (register maps, alarms, stations) |
//! | `codec` | Protocol type identifiers (`ProtocolKind`, `ProtocolFrame`) |
//! | `hal` | HAL traits (Transport, SensorDevice) — embassy-implementable |
//! | `std` | Enable std-only helpers (TOML/JSON manifest adapters) |

extern crate alloc;

#[cfg(feature = "manifest")]
pub mod manifest;

#[cfg(feature = "codec")]
pub mod codec;

#[cfg(feature = "wire")]
pub mod wire;

#[cfg(feature = "hal")]
pub mod hal;

/// Crate-wide version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
