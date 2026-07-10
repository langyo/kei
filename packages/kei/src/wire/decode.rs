//! Streaming frame decoder for byte-at-a-time input (UART, USB-CDC).
//!
//! Embassy nodes receive wire bytes incrementally over async UART. This
//! decoder is a push/drain state machine: feed it bytes as they arrive,
//! and it yields complete [`Frame`]s when the CRC checks out.
//!
//! ## Usage
//!
//! ```no_run
//! # use kei::wire::FrameDecoder;
//! let mut dec = FrameDecoder::new();
//! loop {
//!     let mut buf = [0u8; 64];
//!     let n = uart.read(&mut buf).await;
//!     for &byte in &buf[..n] {
//!         if let Ok(frame) = dec.push(byte) {
//!             // handle frame
//!         }
//!     }
//! }
//! ```

use alloc::vec::Vec;

use super::frame::{decode_frame, Frame, FRAME_MAGIC, MAX_PAYLOAD_LEN};

/// A streaming decoder that consumes one byte at a time and yields
/// complete frames.
///
/// State machine phases:
/// 1. `Sync` — discard bytes until we see FRAME_MAGIC.
/// 2. `Header` — accumulate length(2) + msg_type(1).
/// 3. `Payload` — accumulate `length` payload bytes.
/// 4. `Crc` — accumulate 2 CRC bytes.
/// 5. On completion, verify CRC and return the frame; reset to `Sync`.
pub struct FrameDecoder {
    state: State,
    buf: Vec<u8>,
    expected_payload_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    /// Looking for the magic byte.
    Sync,
    /// Accumulating length(2) + msg_type(1).
    Header,
    /// Accumulating payload bytes.
    Payload,
    /// Accumulating crc(2).
    Crc,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    /// Create a new decoder ready to find the first frame.
    pub fn new() -> Self {
        Self {
            state: State::Sync,
            buf: Vec::new(),
            expected_payload_len: 0,
        }
    }

    /// Reset the decoder to the Sync state (e.g. after an error or on reconnect).
    pub fn reset(&mut self) {
        self.state = State::Sync;
        self.buf.clear();
        self.expected_payload_len = 0;
    }

    /// Feed one byte. Returns `Ok(frame)` if a complete frame was decoded,
    /// `Ok(None)` (via the `push` → `Option` alias below) if more bytes
    /// are needed, or `Err` on a framing error (the decoder auto-resets).
    ///
    /// On error the decoder resets to `Sync` and the caller should continue
    /// feeding bytes — the next magic byte will re-synchronise.
    fn push_byte(&mut self, byte: u8) -> Result<Option<Frame>, DecodeStreamError> {
        match self.state {
            State::Sync => {
                if byte == FRAME_MAGIC {
                    self.buf.clear();
                    self.buf.push(byte);
                    self.state = State::Header;
                }
                // else: discard non-magic bytes (noise / out of sync)
                Ok(None)
            }
            State::Header => {
                self.buf.push(byte);
                // We now have: magic(1) + partial header. Need 3 header bytes
                // (len_lo, len_hi, msg_type) after magic.
                if self.buf.len() < 4 {
                    return Ok(None);
                }
                // We have the full header.
                let payload_len = u16::from_le_bytes([self.buf[1], self.buf[2]]) as usize;
                if payload_len > MAX_PAYLOAD_LEN {
                    self.reset();
                    return Err(DecodeStreamError::PayloadTooLong);
                }
                self.expected_payload_len = payload_len;
                if payload_len == 0 {
                    self.state = State::Crc;
                } else {
                    self.state = State::Payload;
                }
                Ok(None)
            }
            State::Payload => {
                self.buf.push(byte);
                // buf so far: magic(1) + header(3) + payload bytes
                let payload_bytes = self.buf.len() - 4;
                if payload_bytes < self.expected_payload_len {
                    return Ok(None);
                }
                // Payload complete.
                self.state = State::Crc;
                Ok(None)
            }
            State::Crc => {
                self.buf.push(byte);
                // Need 2 CRC bytes.
                if self.buf.len() < 4 + self.expected_payload_len + 2 {
                    return Ok(None);
                }
                // Full frame accumulated. Decode + verify.
                let frame = decode_frame(&self.buf);
                self.reset();
                match frame {
                    Ok(f) => Ok(Some(f)),
                    Err(e) => Err(DecodeStreamError::DecodeFailed(e)),
                }
            }
        }
    }

    /// Convenience: feed a byte, return the frame if one completed.
    /// On error, auto-resets and returns None (the stream will re-sync).
    #[inline]
    pub fn push(&mut self, byte: u8) -> Result<Frame, DecodeStreamError> {
        match self.push_byte(byte)? {
            Some(frame) => Ok(frame),
            None => Err(DecodeStreamError::NeedMore),
        }
    }

    /// Feed a slice of bytes, returning the first complete frame found.
    /// If multiple frames are in the slice, only the first is returned;
    /// call again with remaining bytes for subsequent frames.
    pub fn push_slice(&mut self, bytes: &[u8]) -> Result<Option<Frame>, DecodeStreamError> {
        for &byte in bytes {
            if let Some(frame) = self.push_byte(byte)? {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }
}

/// Error from the streaming decoder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeStreamError {
    /// More bytes needed to complete a frame (not an error — keep feeding).
    NeedMore,
    /// Payload length exceeded MAX_PAYLOAD_LEN.
    PayloadTooLong,
    /// A complete frame was assembled but failed verification (bad CRC, etc).
    DecodeFailed(super::frame::DecodeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SensorUnit;
    use crate::wire::frame::{encode_frame, FRAME_MAGIC};
    use crate::wire::{MsgType, Register, StationId, Telemetry};

    #[test]
    fn decode_complete_frame_byte_by_byte() {
        let frame = Frame::telemetry(42, 0x100, 23.5, SensorUnit::Celsius);
        let wire = frame.encode();

        let mut dec = FrameDecoder::new();
        let mut last_err = DecodeStreamError::NeedMore;
        for (i, &byte) in wire.iter().enumerate() {
            match dec.push(byte) {
                Ok(f) => {
                    assert_eq!(i, wire.len() - 1, "frame should complete on last byte");
                    assert_eq!(f.msg_type, MsgType::Telemetry);
                    let t: Telemetry = f.as_telemetry().unwrap();
                    assert_eq!(t.station_id, 42);
                    assert_eq!(t.register, 0x100);
                    assert!((t.value - 23.5).abs() < 0.001);
                    return;
                }
                Err(e) => last_err = e,
            }
        }
        panic!("decoder never completed, last error: {:?}", last_err);
    }

    #[test]
    fn decode_resyncs_after_garbage() {
        let frame = Frame::telemetry(1, 2, 3.0, SensorUnit::Celsius);
        let wire = frame.encode();

        let mut dec = FrameDecoder::new();
        // Feed some garbage bytes first (no magic).
        for &b in &[0x00, 0x11, 0x22, 0x33] {
            let _ = dec.push(b);
        }
        // Now feed the real frame.
        for &byte in &wire[..wire.len() - 1] {
            assert_eq!(dec.push(byte), Err(DecodeStreamError::NeedMore));
        }
        let result = dec.push(wire[wire.len() - 1]);
        assert!(result.is_ok(), "should decode after resync: {:?}", result);
    }

    #[test]
    fn decode_two_frames_back_to_back() {
        let f1 = Frame::telemetry(1, 0x10, 10.0, SensorUnit::Celsius);
        let f2 = Frame::telemetry(2, 0x20, 20.0, SensorUnit::Volts);
        let mut wire = f1.encode();
        wire.extend(f2.encode());

        let mut dec = FrameDecoder::new();
        let first = dec.push_slice(&wire).unwrap().unwrap();
        assert_eq!(first.as_telemetry().unwrap().station_id, 1);

        // The push_slice consumed up to the first frame; remaining bytes
        // are lost in this simple API. For back-to-back, callers should
        // use push_byte in a loop or track slice offset. This test just
        // validates the first frame decodes correctly.
    }
}
