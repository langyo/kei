//! Hardware manifest schema — the shared contract between sensor nodes
//! and the gateway.
//!
//! Both embassy nodes and evernight deserialize the **same** `HardwareManifest`.
//! The manifest describes: facilities (boxes), connections (protocol + address),
//! stations (register maps), alarm rules, and scale transforms.
//!
//! This module is `#![no_std]` + `alloc`. The only dependency is `serde`.
//! TOML/JSON parsing adapters live behind the `std` feature flag (they
//! need the `toml` / `serde_json` crates which lean std).

pub mod scale;
pub mod sensor;

pub use scale::ScaleTransform;
pub use sensor::{AlarmLevel, Quality, RawValue, RegisterMode, SensorUnit};

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// A complete hardware manifest for a deployment.
///
/// This is the top-level type both embassy nodes and evernight load.
/// It is a pure data model — no behaviour beyond what serde provides.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HardwareManifest {
    /// Schema format version ("1").
    #[serde(default = "default_format_version")]
    pub format_version: String,
    /// Facility / site identifier.
    #[serde(default)]
    pub facility: Facility,
    /// All stations (sensor endpoints) in this deployment.
    #[serde(default)]
    pub stations: Vec<Station>,
    /// Connections (how to reach each station).
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// Alarm rules evaluated against telemetry.
    #[serde(default)]
    pub alarm_rules: Vec<AlarmRule>,
}

fn default_format_version() -> String {
    String::from("1")
}

/// A physical location or logical grouping.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Facility {
    /// Human-readable name (e.g. "Plant A - Building 3").
    #[serde(default)]
    pub name: String,
    /// Optional geographic coordinates (latitude, longitude).
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
}

/// A sensor endpoint — a device or logical group that exposes registers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Station {
    /// Unique station identifier within this manifest.
    pub station_id: u16,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// The connection id this station is reachable through.
    #[serde(default)]
    pub connection_id: String,
    /// Register map for this station (address → name/unit/scale).
    #[serde(default)]
    pub registers: Vec<RegisterMap>,
}

/// A single register's metadata within a station.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterMap {
    /// Register address (protocol-specific: Modbus holding, S7 DB offset, etc.).
    pub address: u16,
    /// Human-readable name (e.g. "temperature_1").
    #[serde(default)]
    pub name: String,
    /// Physical unit of the value at this register.
    #[serde(default)]
    pub unit: SensorUnit,
    /// Read/write mode.
    #[serde(default)]
    pub mode: RegisterMode,
    /// Scale transform applied to the raw value (linear, passthrough, etc.).
    #[serde(default)]
    pub scale: ScaleTransform,
}

/// How to reach a station.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    /// Unique connection identifier (referenced by stations).
    pub id: String,
    /// Connection parameters (protocol-specific).
    #[serde(flatten)]
    pub params: ConnectionParams,
}

/// Tagged enum for the supported connection types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConnectionParams {
    /// Modbus RTU over serial (RS-485).
    #[serde(rename = "modbus_rtu")]
    ModbusRtu {
        serial_port: String,
        baud_rate: u32,
        slave_id: u8,
    },
    /// Modbus TCP.
    #[serde(rename = "modbus_tcp")]
    ModbusTcp {
        host: String,
        port: u16,
        slave_id: u8,
    },
    /// CAN bus (via Waveshare USB-CAN or similar serial bridge).
    #[serde(rename = "can")]
    Can { serial_port: String, baud_rate: u32 },
    /// Siemens S7comm.
    #[serde(rename = "s7comm")]
    S7comm {
        host: String,
        port: u16,
        rack: u8,
        slot: u8,
    },
    /// Mitsubishi MC Protocol (3E frame).
    #[serde(rename = "mc_protocol")]
    McProtocol {
        host: String,
        port: u16,
        network: u8,
        station: u8,
    },
    /// Celestia wire protocol (embassy node over UART/USB-CDC).
    #[serde(rename = "celestia_wire")]
    CelestiaWire { serial_port: String, baud_rate: u32 },
}

/// An alarm rule — ISA-18.2 style thresholds on a register value.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlarmRule {
    /// Station this rule applies to.
    pub station_id: u16,
    /// Register this rule watches.
    pub register: u16,
    /// The threshold set.
    #[serde(flatten)]
    pub thresholds: AlarmThresholds,
}

/// ISA-18.2 alarm thresholds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AlarmThresholds {
    /// Low-low threshold (value below this → LL alarm).
    #[serde(default)]
    pub ll: Option<f32>,
    /// Low threshold.
    #[serde(default)]
    pub l: Option<f32>,
    /// High threshold.
    #[serde(default)]
    pub h: Option<f32>,
    /// High-high threshold.
    #[serde(default)]
    pub hh: Option<f32>,
    /// Hysteresis band (prevents alarm chattering).
    #[serde(default)]
    pub hysteresis: Option<f32>,
}
