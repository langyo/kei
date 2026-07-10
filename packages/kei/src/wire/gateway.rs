//! Gateway-side wire protocol handler.
//!
//! [`Gateway`] is the high-level API the evernight broker (or kei-kernel)
//! uses to talk to sensor nodes. It wraps a [`Transport`] and provides:
//!
//! - `send_read_register()` — ask a node for a register value
//! - `send_write_register()` — command a node to write a value
//! - `send_discover()` — broadcast a discovery probe
//! - `recv()` — receive the next frame from any node (telemetry / alarm / status / response)
//!
//! Like [`Node`](super::node::Node), this is sync and runtime-agnostic.

use crate::hal::{Transport, TransportError};

use super::{
    frame::encode_frame, Alarm, Discover, DiscoverResponse, Frame, MsgType, Nack, ReadRegister,
    ReadStation, StationId, Status, Telemetry, WriteRegister,
};

/// A message received from a sensor node, already deserialised.
#[derive(Clone, Debug)]
pub enum Incoming {
    /// A telemetry reading (the most common frame).
    Telemetry(Telemetry),
    /// An alarm condition.
    Alarm(Alarm),
    /// A lifecycle status (boot / heartbeat / degraded).
    Status(Status),
    /// A response to a discovery probe.
    DiscoverResponse(DiscoverResponse),
    /// A negative acknowledgement.
    Nack(Nack),
}

/// The gateway side of the wire bus. Manages frame encoding for outgoing
/// requests and decoding for incoming node messages.
pub struct Gateway<T: Transport> {
    transport: T,
    decoder: super::decode::FrameDecoder,
}

impl<T: Transport> Gateway<T> {
    /// Create a new gateway over `transport`.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            decoder: super::decode::FrameDecoder::new(),
        }
    }

    /// Borrow the underlying transport.
    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Ask a node to read a register. The node will reply with a Telemetry frame.
    pub fn send_read_register(
        &mut self,
        station_id: StationId,
        register: u16,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&ReadRegister {
            station_id,
            register,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::ReadRegister, &payload)
    }

    /// Command a node to write a value to a register.
    pub fn send_write_register(
        &mut self,
        station_id: StationId,
        register: u16,
        value: f32,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&WriteRegister {
            station_id,
            register,
            value,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::WriteRegister, &payload)
    }

    /// Ask a node for all current values of a station.
    pub fn send_read_station(&mut self, station_id: StationId) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&ReadStation { station_id }).expect("postcard encode");
        self.send_frame(MsgType::ReadStation, &payload)
    }

    /// Broadcast a discovery probe. Nodes that hear it reply with
    /// DiscoverResponse frames (received via [`recv`](Self::recv)).
    pub fn send_discover(&mut self) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&Discover {
            protocol_version: 1,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::Discover, &payload)
    }

    /// Receive the next incoming message from any node. Blocks until a
    /// complete frame arrives.
    pub fn recv(&mut self) -> Result<Incoming, GatewayError> {
        let frame = self.recv_frame()?;
        match frame.msg_type {
            MsgType::Telemetry => {
                let t: Telemetry =
                    postcard::from_bytes(&frame.payload).map_err(GatewayError::Postcard)?;
                Ok(Incoming::Telemetry(t))
            }
            MsgType::Alarm => {
                let a: Alarm =
                    postcard::from_bytes(&frame.payload).map_err(GatewayError::Postcard)?;
                Ok(Incoming::Alarm(a))
            }
            MsgType::Status => {
                let s: Status =
                    postcard::from_bytes(&frame.payload).map_err(GatewayError::Postcard)?;
                Ok(Incoming::Status(s))
            }
            MsgType::DiscoverResponse => {
                let d: DiscoverResponse =
                    postcard::from_bytes(&frame.payload).map_err(GatewayError::Postcard)?;
                Ok(Incoming::DiscoverResponse(d))
            }
            MsgType::Nack => {
                let n: Nack =
                    postcard::from_bytes(&frame.payload).map_err(GatewayError::Postcard)?;
                Ok(Incoming::Nack(n))
            }
            // Requests (ReadRegister etc.) arriving at the gateway are
            // protocol errors (nodes shouldn't send requests). Ignore them.
            _ => {
                // Re-try for the next frame.
                self.recv()
            }
        }
    }

    // ── Internal ────────────────────────────────────────────────────────

    fn send_frame(&mut self, msg_type: MsgType, payload: &[u8]) -> Result<(), TransportError> {
        let wire = encode_frame(msg_type, payload);
        self.transport.send(&wire)?;
        Ok(())
    }

    fn recv_frame(&mut self) -> Result<Frame, GatewayError> {
        let mut byte = [0u8; 1];
        loop {
            let n = self
                .transport
                .recv(&mut byte)
                .map_err(GatewayError::Transport)?;
            if n == 0 {
                continue;
            }
            match self.decoder.push(byte[0]) {
                Ok(frame) => return Ok(frame),
                Err(super::decode::DecodeStreamError::NeedMore) => continue,
                Err(_) => continue, // resync on error
            }
        }
    }
}

/// Error from gateway operations.
#[derive(Clone, Debug)]
pub enum GatewayError {
    Transport(TransportError),
    Postcard(postcard::Error),
}
