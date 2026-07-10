//! Sensor core types — physical units, alarm levels, register modes, raw values.
//!
//! These are the fundamental value types shared between sensor nodes and the
//! gateway. All are plain `serde` types with no OS dependency.

use serde::{Deserialize, Serialize};

/// Physical unit associated with a sensor value.
///
/// Non-exhaustive — new units may be added without a major version bump.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SensorUnit {
    MPa,
    Bar,
    Celsius,
    Ppm,
    Percent,
    PercentLEL,
    Kg,
    Grams,
    Volts,
    Amps,
    Watts,
    Kw,
    Nm3PerHour,
    LitersPerMin,
    MLitersPerMin,
    MicroSiemensPerCm,
    Hours,
    Minutes,
    /// Unitless / dimensionless value.
    #[default]
    Dimensionless,
}

impl SensorUnit {
    /// Returns the canonical short string for this unit (e.g. "°C", "V").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MPa => "MPa",
            Self::Bar => "bar",
            Self::Celsius => "°C",
            Self::Ppm => "ppm",
            Self::Percent => "%",
            Self::PercentLEL => "%LEL",
            Self::Kg => "kg",
            Self::Grams => "g",
            Self::Volts => "V",
            Self::Amps => "A",
            Self::Watts => "W",
            Self::Kw => "kW",
            Self::Nm3PerHour => "Nm³/h",
            Self::LitersPerMin => "L/min",
            Self::MLitersPerMin => "mL/min",
            Self::MicroSiemensPerCm => "µS/cm",
            Self::Hours => "hours",
            Self::Minutes => "min",
            Self::Dimensionless => "",
        }
    }
}

/// Alarm severity level (ISA-18.2 inspired).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum AlarmLevel {
    /// Informational (no action needed).
    Info,
    /// Low priority (e.g. approaching a threshold).
    Low,
    /// High priority (action required).
    High,
    /// Critical (immediate action required).
    Critical,
}

/// Register read/write mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum RegisterMode {
    /// Read-only (input register / discrete input).
    #[default]
    ReadOnly,
    /// Read-write (holding register / coil).
    ReadWrite,
    /// Write-only (output coil).
    WriteOnly,
}

/// Quality flag for a sensor reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Quality {
    /// Reading is fresh and valid.
    Good,
    /// Reading has not been updated within the expected interval.
    Stale,
    /// Read or conversion error occurred.
    Error,
}

/// Raw register value — covers both u16 holding/input registers and coil booleans.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RawValue {
    /// Unsigned 16-bit integer (holding/input register).
    U16(u16),
    /// Signed 16-bit integer (interpreted from raw u16).
    I16(i16),
    /// 32-bit float (reconstructed from two consecutive u16 registers).
    F32(f32),
    /// Boolean (coil / discrete input).
    Bool(bool),
}

impl RawValue {
    /// Convert to an f64 for scale transforms and alarm evaluation.
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::U16(v) => *v as f64,
            Self::I16(v) => *v as f64,
            Self::F32(v) => *v as f64,
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}
