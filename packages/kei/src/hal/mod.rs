//! HAL traits — embassy-implementable abstractions for hardware access.
//!
//! Embassy nodes implement these traits against their specific MCU peripherals.
//! The `kei::wire` module uses them to send/receive frames and read sensors
//! without coupling to any particular async runtime or HAL crate.

pub mod device;
pub mod transport;

pub use device::{DeviceError, SensorDevice};
pub use transport::{
    AddressedTransport, AsyncAddressedTransport, AsyncTransport, AsyncTransportExt, NodeAddr,
    Transport, TransportError,
};
