//! Sensor device trait — what a sensor node exposes to the wire protocol.
//!
//! Embassy nodes implement this against their hardware (ADC, I2C sensor,
//! GPIO-attached encoder, etc.). The wire protocol handler calls these
//! methods when it receives a ReadRegister / WriteRegister request.

use crate::manifest::{RawValue, RegisterMode, SensorUnit};
use crate::wire::Register;

/// A sensor device that the wire protocol can read from / write to.
///
/// The node registers one `SensorDevice` per station. When the gateway
/// sends a ReadRegister frame, the node calls `read_register`; when it
/// sends WriteRegister, the node calls `write_register`.
pub trait SensorDevice {
    /// Read a register's current value.
    fn read_register(&mut self, register: Register) -> Result<RawValue, DeviceError>;

    /// Write a value to a register.
    fn write_register(&mut self, register: Register, value: f32) -> Result<(), DeviceError>;

    /// How many registers this device exposes.
    fn register_count(&self) -> u16;

    /// The unit for a given register (for telemetry reporting).
    fn unit_for(&self, register: Register) -> SensorUnit {
        let _ = register;
        SensorUnit::Dimensionless
    }

    /// The mode for a given register.
    fn mode_for(&self, register: Register) -> RegisterMode {
        let _ = register;
        RegisterMode::ReadOnly
    }
}

/// Error from a sensor device operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceError {
    /// The register address is not valid for this device.
    InvalidRegister,
    /// Hardware read/write failure.
    HardwareError,
    /// The register is write-only / read-only.
    PermissionDenied,
}
