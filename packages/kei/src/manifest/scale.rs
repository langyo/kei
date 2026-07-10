//! Scale transforms — convert raw register values to engineering units.
//!
//! The manifest declares a scale transform per register. The gateway and
//! sensor nodes both apply it, ensuring they agree on the engineering value.
//! Closures are not serialisable (and not `no_std`-friendly with `Arc<dyn Fn>`),
//! so we use an explicit enum.

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

/// A scale transform applied to a raw register value.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum ScaleTransform {
    /// No transformation — the raw value IS the engineering value.
    #[default]
    Passthrough,
    /// Linear: `engineering = (raw * factor) + offset`.
    Linear {
        factor: f32,
        offset: f32,
        /// The unit the scaled value is in (overrides the register's default).
        #[serde(default)]
        unit: Option<crate::manifest::SensorUnit>,
    },
    /// Lookup table with linear interpolation between points.
    ///
    /// Use for non-linear sensors (NTC thermistors, RTD, pressure
    /// transducers). `x` must be sorted ascending. Values outside the
    /// table range are clamped to the nearest endpoint.
    Table {
        /// Raw values (must be sorted ascending).
        x: Vec<f32>,
        /// Engineering values (same length as `x`).
        y: Vec<f32>,
        #[serde(default)]
        unit: Option<crate::manifest::SensorUnit>,
    },
    /// Polynomial: `engineering = c0 + c1*raw + c2*raw² + ...`.
    ///
    /// Coefficients are stored low-degree-first: `coeffs[0]` is the
    /// constant term, `coeffs[1]` is the linear coefficient, etc.
    Polynomial {
        coeffs: Vec<f32>,
        #[serde(default)]
        unit: Option<crate::manifest::SensorUnit>,
    },
}

impl ScaleTransform {
    /// Apply this transform to a raw f64 value, returning the engineering value.
    pub fn apply(&self, raw: f64) -> f64 {
        match self {
            Self::Passthrough => raw,
            Self::Linear { factor, offset, .. } => raw * (*factor as f64) + (*offset as f64),
            Self::Table { x, y, .. } => interpolate(x, y, raw as f32) as f64,
            Self::Polynomial { coeffs, .. } => {
                // Horner's method: c0 + raw*(c1 + raw*(c2 + ...))
                let r = raw;
                let mut result = 0.0_f64;
                for &c in coeffs.iter().rev() {
                    result = result * r + (c as f64);
                }
                result
            }
        }
    }

    /// Apply to a raw value and return the unit (if the transform overrides it).
    pub fn apply_with_unit(
        &self,
        raw: f64,
        default_unit: crate::manifest::SensorUnit,
    ) -> (f64, crate::manifest::SensorUnit) {
        let scaled = self.apply(raw);
        let unit = match self {
            Self::Linear { unit: Some(u), .. }
            | Self::Table { unit: Some(u), .. }
            | Self::Polynomial { unit: Some(u), .. } => *u,
            _ => default_unit,
        };
        (scaled, unit)
    }
}

/// Linear interpolation on sorted lookup tables.
///
/// Clamps to the nearest endpoint if `raw` is outside `[x[0], x[-1]]`.
/// Returns `y[0]` if the table has only one entry.
fn interpolate(x: &[f32], y: &[f32], raw: f32) -> f32 {
    if x.is_empty() || y.is_empty() {
        return raw; // degenerate table — passthrough
    }
    if x.len() == 1 || raw <= x[0] {
        return y[0];
    }
    if raw >= x[x.len() - 1] {
        return y[y.len() - 1];
    }
    // Binary search for the interval [x[i], x[i+1]] containing raw.
    let mut lo = 0usize;
    let mut hi = x.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if x[mid] <= raw {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let x0 = x[lo];
    let x1 = x[hi];
    let y0 = y[lo];
    let y1 = y[hi];
    if (x1 - x0).abs() < f32::EPSILON {
        return y0;
    }
    y0 + (raw - x0) * (y1 - y0) / (x1 - x0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough() {
        assert_eq!(ScaleTransform::Passthrough.apply(42.0), 42.0);
    }

    #[test]
    fn linear() {
        let t = ScaleTransform::Linear {
            factor: 2.0,
            offset: 10.0,
            unit: None,
        };
        assert!((t.apply(5.0) - 20.0).abs() < 0.001); // 5*2+10 = 20
    }

    #[test]
    fn table_interpolation() {
        let t = ScaleTransform::Table {
            x: alloc::vec![0.0, 100.0, 200.0],
            y: alloc::vec![-50.0, 50.0, 150.0],
            unit: None,
        };
        // Exact points
        assert!((t.apply(0.0) - (-50.0)).abs() < 0.01);
        assert!((t.apply(100.0) - 50.0).abs() < 0.01);
        // Midpoint interpolation
        assert!((t.apply(50.0) - 0.0).abs() < 0.01);
        assert!((t.apply(150.0) - 100.0).abs() < 0.01);
        // Clamp outside range
        assert!((t.apply(-10.0) - (-50.0)).abs() < 0.01);
        assert!((t.apply(300.0) - 150.0).abs() < 0.01);
    }

    #[test]
    fn polynomial() {
        // quadratic: 1 + 2x + 3x² ; at x=2 → 1 + 4 + 12 = 17
        let t = ScaleTransform::Polynomial {
            coeffs: alloc::vec![1.0, 2.0, 3.0],
            unit: None,
        };
        assert!((t.apply(2.0) - 17.0).abs() < 0.001);
    }

    #[test]
    fn polynomial_constant() {
        let t = ScaleTransform::Polynomial {
            coeffs: alloc::vec![42.0],
            unit: None,
        };
        assert!((t.apply(999.0) - 42.0).abs() < 0.001);
    }
}
