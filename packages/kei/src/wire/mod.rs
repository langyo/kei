//! Celestia wire protocol — compact binary framing for embassy ↔ gateway.
//!
//! ## Frame layout (v1)
//!
//! ```text
//! ┌───────┬────────┬──────────┬─────────┬───────┐
//! │ magic │ length │ msg_type │ payload │ crc16 │
//! │ 0xCE  │ u16 LE │ u8       │ [u8]    │ u16LE │
//! └───────┴────────┴──────────┴─────────┴───────┘
//! ```
//!
//! - `magic` = `0xCE` — frame synchronisation.
//! - `length` = payload byte count (little-endian, max 1024).
//! - `msg_type` = one of [`MsgType`].
//! - `payload` = postcard-encoded message struct.
//! - `crc16` = CRC16-Modbus over `[length .. end of payload]`.
//!
//! ## Usage
//!
//! Embassy node side:
//! ```no_run
//! # use kei::wire::*;
//! let frame = Frame::telemetry(1, 0x100, 23.5, kei::manifest::SensorUnit::Celsius);
//! let bytes: Vec<u8> = frame.encode();
//! transport.send(&bytes).await;
//! ```
//!
//! Gateway side:
//! ```no_run
//! # use kei::wire::*;
//! let bytes = transport.recv().await;
//! let frame = Frame::decode(&bytes)?;
//! ```

pub mod decode;
pub mod frame;
pub mod gateway;
pub mod node;
pub mod request;
pub mod response;

pub use decode::FrameDecoder;
pub use frame::{decode_frame, encode_frame, Frame, FRAME_MAGIC, MAX_PAYLOAD_LEN};
pub use gateway::{Gateway, GatewayError, Incoming};
pub use node::{Node, NodeError, Request};
pub use request::*;
pub use response::*;

// Re-export manifest types used in wire message payloads, so that
// `super::AlarmLevel` etc. resolve inside frame.rs / response.rs.
pub use crate::manifest::{AlarmLevel, SensorUnit};

use serde::{Deserialize, Serialize};

/// A globally unique station identifier within a gateway's scope.
pub type StationId = u16;

/// A register address (protocol-agnostic; maps to Modbus holding/coil,
/// S7 DB offset, MC device address, etc. per the manifest's station config).
pub type Register = u16;

/// Message type byte in the wire frame header.
///
/// `0x01-0x0F`: gateway → node (requests).
/// `0x10-0x1F`: node → gateway (responses / unsolicited).
/// `0xFF`: error (bidirectional).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MsgType {
    /// Gateway asks a node to read a register. Payload: [`ReadRegister`].
    ReadRegister = 0x01,
    /// Gateway asks a node to write a register. Payload: [`WriteRegister`].
    WriteRegister = 0x02,
    /// Gateway asks for all of a station's current values. Payload: [`ReadStation`].
    ReadStation = 0x03,
    /// Gateway broadcasts a discovery probe. Payload: [`Discover`].
    Discover = 0x04,

    /// Node reports a telemetry value. Payload: [`Telemetry`].
    Telemetry = 0x10,
    /// Node reports an alarm condition. Payload: [`Alarm`].
    Alarm = 0x11,
    /// Node reports its status (boot, heartbeat, error). Payload: [`Status`].
    Status = 0x12,
    /// Node responds to a discovery probe. Payload: [`DiscoverResponse`].
    DiscoverResponse = 0x13,

    /// Negative acknowledgement. Payload: [`Nack`].
    Nack = 0xFF,
}

impl MsgType {
    /// Convert a raw byte to a MsgType, or None if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0x01 => Self::ReadRegister,
            0x02 => Self::WriteRegister,
            0x03 => Self::ReadStation,
            0x04 => Self::Discover,
            0x10 => Self::Telemetry,
            0x11 => Self::Alarm,
            0x12 => Self::Status,
            0x13 => Self::DiscoverResponse,
            0xFF => Self::Nack,
            _ => return None,
        })
    }

    /// True if this message type flows gateway → node.
    pub fn is_request(&self) -> bool {
        matches!(
            self,
            Self::ReadRegister | Self::WriteRegister | Self::ReadStation | Self::Discover
        )
    }

    /// True if this message type flows node → gateway.
    pub fn is_response(&self) -> bool {
        !self.is_request() && !matches!(self, Self::Nack)
    }
}
