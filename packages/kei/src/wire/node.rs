//! Sensor-node-side wire protocol handler.
//!
//! [`Node`] is the high-level API embassy sensor nodes use. It wraps a
//! [`Transport`](crate::hal::Transport) and provides:
//!
//! - `poll()` — receive the next gateway request (ReadRegister / WriteRegister / Discover)
//! - `telemetry()` — send a telemetry value to the gateway
//! - `alarm()` — report an alarm condition
//! - `status()` — report lifecycle state (boot / heartbeat)
//!
//! The node owns a [`FrameDecoder`] internally and handles CRC verification,
//! re-synchronisation, and frame encoding. The caller only sees high-level
//! message types.
//!
//! ## Async model
//!
//! `Node` is **sync** — it does `recv_byte()` on the transport in a loop
//! internally. Embassy callers wrap this in `embassy_futures::block_on` or
//! call `poll()` from an embassy task. This keeps kei runtime-agnostic
//! (no dependency on `embassy-async` or `tokio`).
//!
//! For embassy async usage, see `examples/embassy_node.rs`.

use alloc::vec::Vec;

use crate::hal::{DeviceError, SensorDevice, Transport, TransportError};
use crate::manifest::SensorUnit;

use super::decode::FrameDecoder;
use super::frame::Frame;
use super::{
    Alarm, AlarmLevel, Discover, DiscoverResponse, MsgType, Nack, NodeState, ReadRegister,
    ReadStation, StationId, Status, Telemetry, WriteRegister,
};

/// A request received from the gateway, already deserialised.
#[derive(Clone, Debug)]
pub enum Request {
    /// Gateway wants a single register's value. Reply via
    /// [`Node::telemetry`].
    ReadRegister(ReadRegister),
    /// Gateway wants to write a value.
    WriteRegister(WriteRegister),
    /// Gateway wants all current values of a station.
    ReadStation(ReadStation),
    /// Discovery probe — reply via [`Node::discover_response`].
    Discover(Discover),
}

/// A sensor node on the wire bus. Owns the transport and a frame decoder.
///
/// Create one per station, then call [`Node::poll`] in your main loop.
pub struct Node<T: Transport> {
    transport: T,
    decoder: FrameDecoder,
    station_id: StationId,
    /// Accumulated incoming bytes that haven't formed a complete frame yet.
    rx_buf: Vec<u8>,
}

impl<T: Transport> Node<T> {
    /// Create a new node for `station_id` over `transport`.
    pub fn new(transport: T, station_id: StationId) -> Self {
        Self {
            transport,
            decoder: FrameDecoder::new(),
            station_id,
            rx_buf: Vec::new(),
        }
    }

    /// Borrow the underlying transport (e.g. to configure baud rate).
    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Send a telemetry value to the gateway.
    pub fn send_telemetry(
        &mut self,
        register: u16,
        value: f32,
        unit: SensorUnit,
        timestamp_ms: u64,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&Telemetry {
            station_id: self.station_id,
            register,
            value,
            unit,
            timestamp_ms,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::Telemetry, &payload)
    }

    /// Report an alarm condition.
    pub fn send_alarm(
        &mut self,
        register: u16,
        level: AlarmLevel,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&Alarm {
            station_id: self.station_id,
            register,
            level,
            message: alloc::string::String::from(message),
            timestamp_ms,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::Alarm, &payload)
    }

    /// Report lifecycle state (call on boot, periodically for heartbeat).
    pub fn send_status(
        &mut self,
        state: NodeState,
        detail: &str,
        timestamp_ms: u64,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&Status {
            station_id: self.station_id,
            state,
            detail: alloc::string::String::from(detail),
            timestamp_ms,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::Status, &payload)
    }

    /// Reply to a discovery probe.
    pub fn send_discover_response(
        &mut self,
        name: &str,
        register_count: u16,
    ) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&DiscoverResponse {
            station_id: self.station_id,
            protocol_version: 1,
            name: alloc::string::String::from(name),
            register_count,
        })
        .expect("postcard encode");
        self.send_frame(MsgType::DiscoverResponse, &payload)
    }

    /// Poll for the next gateway request. Blocks until a complete frame
    /// arrives (or transport error).
    ///
    /// If a write request targets this node's `SensorDevice`, this method
    /// dispatches it automatically and returns the next *read* request (or
    /// discovery). Callers typically only handle `ReadRegister` / `Discover`.
    ///
    /// Pass `&mut impl SensorDevice` to enable automatic write dispatch;
    /// pass `&mut None` (via the `poll_readonly` method) if you don't have
    /// a device yet.
    pub fn poll(&mut self, device: &mut dyn SensorDevice) -> Result<Request, NodeError> {
        loop {
            let frame = self.recv_frame()?;
            match frame.msg_type {
                MsgType::ReadRegister => {
                    let req: ReadRegister =
                        postcard::from_bytes(&frame.payload).map_err(NodeError::Postcard)?;
                    if req.station_id != self.station_id {
                        continue; // not for us
                    }
                    return Ok(Request::ReadRegister(req));
                }
                MsgType::WriteRegister => {
                    let req: WriteRegister =
                        postcard::from_bytes(&frame.payload).map_err(NodeError::Postcard)?;
                    if req.station_id != self.station_id {
                        continue;
                    }
                    // Auto-dispatch write.
                    match device.write_register(req.register, req.value) {
                        Ok(()) => continue, // ack by continuing to next request
                        Err(e) => {
                            self.send_nack(req.register, 1, "write failed")?;
                            return Err(NodeError::Device(e));
                        }
                    }
                }
                MsgType::ReadStation => {
                    let req: ReadStation =
                        postcard::from_bytes(&frame.payload).map_err(NodeError::Postcard)?;
                    if req.station_id != self.station_id {
                        continue;
                    }
                    return Ok(Request::ReadStation(req));
                }
                MsgType::Discover => {
                    let _: Discover =
                        postcard::from_bytes(&frame.payload).map_err(NodeError::Postcard)?;
                    return Ok(Request::Discover(Discover {
                        protocol_version: 1,
                    }));
                }
                _ => {
                    // Unexpected message type from gateway; ignore.
                    continue;
                }
            }
        }
    }

    /// Poll variant that doesn't auto-dispatch writes (for read-only devices
    /// or before the device is initialised).
    pub fn poll_readonly(&mut self) -> Result<Request, NodeError> {
        // Use a dummy device that rejects all writes.
        struct NoDevice;
        impl SensorDevice for NoDevice {
            fn read_register(&mut self, _: u16) -> Result<crate::manifest::RawValue, DeviceError> {
                Err(DeviceError::HardwareError)
            }
            fn write_register(&mut self, _: u16, _: f32) -> Result<(), DeviceError> {
                Err(DeviceError::PermissionDenied)
            }
            fn register_count(&self) -> u16 {
                0
            }
        }
        self.poll(&mut NoDevice)
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn send_frame(&mut self, msg_type: MsgType, payload: &[u8]) -> Result<(), TransportError> {
        let wire = super::frame::encode_frame(msg_type, payload);
        self.transport.send(&wire)?;
        Ok(())
    }

    fn send_nack(&mut self, _register: u16, code: u16, msg: &str) -> Result<(), TransportError> {
        let payload = postcard::to_allocvec(&Nack {
            station_id: self.station_id,
            error_code: code,
            message: alloc::string::String::from(msg),
        })
        .expect("postcard encode");
        self.send_frame(MsgType::Nack, &payload)
    }

    fn recv_frame(&mut self) -> Result<Frame, NodeError> {
        let mut byte = [0u8; 1];
        loop {
            // Try the decoder with buffered bytes first.
            if !self.rx_buf.is_empty() {
                if let Some(frame) = self.decoder.push_slice(&self.rx_buf)? {
                    self.rx_buf.clear();
                    return Ok(frame);
                }
            }
            self.rx_buf.clear();

            // Read one byte from transport.
            let n = self
                .transport
                .recv(&mut byte)
                .map_err(NodeError::Transport)?;
            if n == 0 {
                continue;
            }
            // Feed to decoder.
            if let Some(frame) = self.decoder.push_slice(&byte[..n])? {
                return Ok(frame);
            }
            // Accumulate (decoder may need more).
            // Note: push_slice already consumed the byte into internal state;
            // rx_buf is only for the multi-byte slice case above.
        }
    }
}

/// Error from node operations.
#[derive(Clone, Debug)]
pub enum NodeError {
    Transport(TransportError),
    Postcard(postcard::Error),
    Device(DeviceError),
    /// Frame decoding error (bad CRC, framing, etc).
    Decode(super::decode::DecodeStreamError),
}

impl From<TransportError> for NodeError {
    fn from(e: TransportError) -> Self {
        Self::Transport(e)
    }
}

impl From<postcard::Error> for NodeError {
    fn from(e: postcard::Error) -> Self {
        Self::Postcard(e)
    }
}

impl From<DeviceError> for NodeError {
    fn from(e: DeviceError) -> Self {
        Self::Device(e)
    }
}

impl From<super::decode::DecodeStreamError> for NodeError {
    fn from(e: super::decode::DecodeStreamError) -> Self {
        Self::Decode(e)
    }
}
