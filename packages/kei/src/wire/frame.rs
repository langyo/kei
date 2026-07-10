//! Wire frame encode/decode and the [`Frame`] struct.

use alloc::vec::Vec;
use serde::Deserialize;

use super::{MsgType, Register, StationId};
use crate::manifest::SensorUnit;

/// Frame magic byte — synchronises the stream.
pub const FRAME_MAGIC: u8 = 0xCE;
/// Maximum payload length (before postcard encoding). Keeps frames small
/// enough for MCUs with limited RAM.
pub const MAX_PAYLOAD_LEN: usize = 1024;

/// Header overhead: magic(1) + length(2) + msg_type(1) + crc16(2) = 6 bytes.
pub const FRAME_OVERHEAD: usize = 6;

/// A parsed wire frame. The payload is still raw bytes (postcard-encoded);
/// use the `as_*` methods or `postcard::from_bytes` to deserialize.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub msg_type: MsgType,
    pub payload: Vec<u8>,
}

impl Frame {
    /// Build a Telemetry frame (node → gateway).
    pub fn telemetry(station: StationId, register: Register, value: f32, unit: SensorUnit) -> Self {
        let payload = postcard::to_allocvec(&super::Telemetry {
            station_id: station,
            register,
            value,
            unit,
            timestamp_ms: 0, // node fills in if it has a clock
        })
        .expect("postcard encode telemetry");
        Self {
            msg_type: MsgType::Telemetry,
            payload,
        }
    }

    /// Build an Alarm frame (node → gateway).
    pub fn alarm(
        station: StationId,
        register: Register,
        level: super::AlarmLevel,
        message: &str,
    ) -> Self {
        let payload = postcard::to_allocvec(&super::Alarm {
            station_id: station,
            register,
            level,
            message: alloc::string::String::from(message),
            timestamp_ms: 0,
        })
        .expect("postcard encode alarm");
        Self {
            msg_type: MsgType::Alarm,
            payload,
        }
    }

    /// Build a Status frame (node → gateway).
    pub fn status(station: StationId, state: super::NodeState, detail: &str) -> Self {
        let payload = postcard::to_allocvec(&super::Status {
            station_id: station,
            state,
            detail: alloc::string::String::from(detail),
            timestamp_ms: 0,
        })
        .expect("postcard encode status");
        Self {
            msg_type: MsgType::Status,
            payload,
        }
    }

    /// Build a ReadRegister request (gateway → node).
    pub fn read_register(station: StationId, register: Register) -> Self {
        let payload = postcard::to_allocvec(&super::ReadRegister {
            station_id: station,
            register,
        })
        .expect("postcard encode read_register");
        Self {
            msg_type: MsgType::ReadRegister,
            payload,
        }
    }

    /// Build a Nack frame.
    pub fn nack(station: StationId, error_code: u16, message: &str) -> Self {
        let payload = postcard::to_allocvec(&super::Nack {
            station_id: station,
            error_code,
            message: alloc::string::String::from(message),
        })
        .expect("postcard encode nack");
        Self {
            msg_type: MsgType::Nack,
            payload,
        }
    }

    /// Deserialize the payload as a specific type.
    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T, postcard::Error> {
        postcard::from_bytes(&self.payload)
    }

    /// Deserialize as Telemetry (convenience).
    pub fn as_telemetry(&self) -> Result<super::Telemetry, postcard::Error> {
        self.payload_as()
    }

    /// Encode this frame to wire bytes.
    ///
    /// Layout: `magic | len_lo len_hi | msg_type | payload... | crc_lo crc_hi`
    pub fn encode(&self) -> Vec<u8> {
        encode_frame(self.msg_type, &self.payload)
    }
}

/// Encode a frame to wire bytes. CRC16-Modbus covers `[length .. end of payload]`.
pub fn encode_frame(msg_type: MsgType, payload: &[u8]) -> Vec<u8> {
    let payload_len = payload.len().min(MAX_PAYLOAD_LEN);
    let len_bytes = (payload_len as u16).to_le_bytes();

    // CRC covers: length(2) + msg_type(1) + payload
    let mut crc_region = Vec::with_capacity(3 + payload_len);
    crc_region.extend_from_slice(&len_bytes);
    crc_region.push(msg_type as u8);
    crc_region.extend_from_slice(&payload[..payload_len]);
    let crc_val = crc16_modbus(&crc_region);

    let mut out = Vec::with_capacity(FRAME_OVERHEAD + payload_len);
    out.push(FRAME_MAGIC);
    out.extend_from_slice(&len_bytes);
    out.push(msg_type as u8);
    out.extend_from_slice(&payload[..payload_len]);
    out.extend_from_slice(&crc_val.to_le_bytes());
    out
}

/// Decode a complete frame from wire bytes.
///
/// Returns `Err(DecodeError)` if the input is malformed, truncated, or
/// the CRC check fails. For streaming input (UART byte-by-byte), use
/// [`super::FrameDecoder`] instead.
pub fn decode_frame(buf: &[u8]) -> Result<Frame, DecodeError> {
    if buf.len() < FRAME_OVERHEAD {
        return Err(DecodeError::TooShort);
    }
    if buf[0] != FRAME_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let payload_len = u16::from_le_bytes([buf[1], buf[2]]) as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLong);
    }
    let msg_type = MsgType::from_u8(buf[3]).ok_or(DecodeError::UnknownMsgType)?;
    let total = FRAME_OVERHEAD + payload_len;
    if buf.len() < total {
        return Err(DecodeError::TooShort);
    }
    let payload = &buf[4..4 + payload_len];
    let crc_recv = u16::from_le_bytes([buf[4 + payload_len], buf[5 + payload_len]]);

    // Verify CRC over [length(2) + msg_type(1) + payload]
    let crc_region = &buf[1..4 + payload_len];
    let crc_calc = crc16_modbus(crc_region);
    if crc_calc != crc_recv {
        return Err(DecodeError::CrcMismatch {
            expected: crc_calc,
            got: crc_recv,
        });
    }

    Ok(Frame {
        msg_type,
        payload: payload.to_vec(),
    })
}

/// Error returned by [`decode_frame`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    TooShort,
    BadMagic,
    PayloadTooLong,
    UnknownMsgType,
    CrcMismatch { expected: u16, got: u16 },
}

// ── CRC16-Modbus (poly 0xA001, init 0xFFFF) ─────────────────────────────────

/// Compute CRC16-Modbus over `data`.
///
/// This is the standard Modbus RTU checksum (polynomial 0xA001, reflected,
/// init 0xFFFF, no final XOR). Also used as the wire-frame integrity check.
pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}
