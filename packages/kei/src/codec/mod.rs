//! Protocol type identifiers and opaque frame containers.
//!
//! This module does **not** contain protocol encode/decode logic. All
//! wire-level framing (Modbus RTU/TCP, MC Protocol, EtherNet/IP-CIP, CAN,
//! S7comm, etc.) is the sole responsibility of **evernight**, the gateway
//! broker. kei only provides:
//!
//! - [`ProtocolKind`] — identifies which industrial protocol a station uses.
//! - [`ProtocolFrame`] — an opaque tagged byte buffer for passing raw
//!   protocol data through the wire layer without kei needing to understand it.
//!
//! This keeps kei small and lets evernight evolve its codecs independently.

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Identifies an industrial protocol family.
///
/// Used in manifest `Connection` entries and wire `DiscoverResponse` to
/// declare what protocol a station speaks. kei does not implement any of
/// these — evernight owns the full codec stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProtocolKind {
    /// Modbus RTU (serial RS-485).
    ModbusRtu,
    /// Modbus TCP.
    ModbusTcp,
    /// Siemens S7comm.
    S7comm,
    /// Mitsubishi MC Protocol (3E frame).
    McProtocol,
    /// Rockwell EtherNet/IP + CIP.
    EthernetIp,
    /// EtherCAT.
    Ethercat,
    /// CAN bus.
    Can,
    /// OPC UA.
    OpcUa,
    /// MQTT.
    Mqtt,
    /// Celestia wire protocol (embassy native).
    CelestiaWire,
    /// Unknown / custom protocol (extensible).
    Other(u8),
}

/// An opaque protocol frame — raw bytes tagged with the protocol kind.
///
/// This is a pass-through container. kei does not inspect or modify the
/// bytes; evernight (or the embassy node, for `CelestiaWire`) encodes and
/// decodes them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolFrame {
    /// Which protocol these bytes belong to.
    pub kind: ProtocolKind,
    /// The raw protocol bytes (opaque to kei).
    pub data: Vec<u8>,
}
