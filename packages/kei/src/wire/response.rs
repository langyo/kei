//! Node → gateway response / unsolicited messages.

use alloc::string::String;
use serde::{Deserialize, Serialize};

use super::{Register, StationId};
use crate::manifest::{AlarmLevel, SensorUnit};

/// A node reports a telemetry value. Payload of [`MsgType::Telemetry`].
///
/// This is the most common frame on the wire — sensor nodes send these
/// periodically (poll) or on-change (event-driven).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Telemetry {
    pub station_id: StationId,
    pub register: Register,
    pub value: f32,
    pub unit: SensorUnit,
    /// Epoch milliseconds (Unix). 0 if the node has no RTC — the gateway
    /// stamps its receive time in that case.
    pub timestamp_ms: u64,
}

/// A node reports an alarm condition. Payload of [`MsgType::Alarm`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alarm {
    pub station_id: StationId,
    pub register: Register,
    pub level: AlarmLevel,
    /// Human-readable description (e.g. "temperature exceeded HH threshold").
    pub message: String,
    pub timestamp_ms: u64,
}

/// A node reports its lifecycle state. Payload of [`MsgType::Status`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    /// Node just booted / joined the bus.
    Boot = 0,
    /// Heartbeat (the node is alive and polling).
    Heartbeat = 1,
    /// Node is shutting down or going to sleep.
    Shutdown = 2,
    /// Node hit an internal error but is still running.
    Degraded = 3,
}

/// Payload of [`MsgType::Status`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub station_id: StationId,
    pub state: NodeState,
    /// Optional detail (e.g. firmware version on Boot, error on Degraded).
    pub detail: String,
    pub timestamp_ms: u64,
}

/// A node responds to a [`super::Discover`] probe.
/// Payload of [`MsgType::DiscoverResponse`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoverResponse {
    pub station_id: StationId,
    /// The node's protocol version.
    pub protocol_version: u8,
    /// Station name from the node's manifest (if any).
    pub name: String,
    /// How many registers this station exposes.
    pub register_count: u16,
}

/// Negative acknowledgement. Payload of [`MsgType::Nack`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Nack {
    pub station_id: StationId,
    /// Gateway-defined error code (0 = generic).
    pub error_code: u16,
    pub message: String,
}
