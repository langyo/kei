//! Gateway → node request messages.

use serde::{Deserialize, Serialize};

use super::{Register, StationId};

/// Gateway asks a node to read a single register and report it back
/// (via a Telemetry frame). Payload of [`MsgType::ReadRegister`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ReadRegister {
    pub station_id: StationId,
    pub register: Register,
}

/// Gateway asks a node to write a value to a register.
/// Payload of [`MsgType::WriteRegister`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WriteRegister {
    pub station_id: StationId,
    pub register: Register,
    /// The raw value to write, as an IEEE-754 f32. The node's manifest
    /// defines the register's native type (u16/int32/float) and the node
    /// converts accordingly.
    pub value: f32,
}

/// Gateway asks for all current values of a station.
/// Payload of [`MsgType::ReadStation`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ReadStation {
    pub station_id: StationId,
}

/// Gateway broadcasts a discovery probe. Nodes that hear it reply with
/// a DiscoverResponse. Payload of [`MsgType::Discover`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Discover {
    /// Protocol version the gateway speaks. A node only responds if it
    /// supports the same major version.
    pub protocol_version: u8,
}
