//! Parameter model shared by every instrument and effect.

/// How a normalized 0..=1 value maps onto the plain range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Exponential,
}

/// Static description of one user-facing parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    pub curve: Curve,
    pub default: f64,
    pub help: &'static str,
}

impl ParamSpec {
    /// Maps `normalized` (clamped to 0..=1) to the plain range.
    pub fn to_plain(&self, normalized: f64) -> f64 {
        let n = normalized.clamp(0.0, 1.0);
        if n == 0.0 {
            return self.min;
        }
        if n == 1.0 {
            return self.max;
        }
        match self.curve {
            Curve::Linear => self.min + n * (self.max - self.min),
            Curve::Exponential => self.min * (self.max / self.min).powf(n),
        }
    }

    /// Maps `plain` (clamped to min..=max) to 0..=1.
    pub fn to_normalized(&self, plain: f64) -> f64 {
        let p = plain.clamp(self.min, self.max);
        match self.curve {
            Curve::Linear => (p - self.min) / (self.max - self.min),
            Curve::Exponential => (p / self.min).ln() / (self.max / self.min).ln(),
        }
    }
}

/// Failure to look up or set a parameter.
#[derive(Debug, thiserror::Error)]
pub enum ParamError {
    #[error("unknown parameter id `{0}`")]
    UnknownId(String),
    #[error("value {value} for `{id}` is out of range {min}..={max}")]
    OutOfRange {
        id: String,
        value: f64,
        min: f64,
        max: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: ParamSpec = ParamSpec {
        id: "cutoff",
        label: "Cutoff",
        unit: "Hz",
        min: 314.0,
        max: 2394.0,
        curve: Curve::Exponential,
        default: 1000.0,
        help: "",
    };
    const TUNING: ParamSpec = ParamSpec {
        id: "tuning",
        label: "Tuning",
        unit: "Hz",
        min: 400.0,
        max: 480.0,
        curve: Curve::Linear,
        default: 440.0,
        help: "",
    };

    #[test]
    fn exp_endpoints_exact() {
        assert_eq!(CUTOFF.to_plain(0.0), 314.0);
        assert_eq!(CUTOFF.to_plain(1.0), 2394.0);
    }

    #[test]
    fn exp_midpoint() {
        assert!((CUTOFF.to_plain(0.5) - 867.016).abs() < 0.001);
    }

    #[test]
    fn round_trips() {
        for spec in [CUTOFF, TUNING] {
            for x in [0.0, 0.25, 0.7, 1.0] {
                let back = spec.to_normalized(spec.to_plain(x));
                assert!((back - x).abs() < 1e-12, "{} {x} -> {back}", spec.id);
            }
        }
    }

    #[test]
    fn clamps() {
        assert_eq!(CUTOFF.to_plain(-3.0), 314.0);
        assert_eq!(CUTOFF.to_plain(7.0), 2394.0);
        assert_eq!(CUTOFF.to_normalized(1.0), 0.0);
        assert_eq!(CUTOFF.to_normalized(1e9), 1.0);
        assert_eq!(TUNING.to_plain(0.5), 440.0);
    }

    #[test]
    fn error_messages() {
        let e = ParamError::UnknownId("nope".into());
        assert!(e.to_string().contains("nope"));
        let e = ParamError::OutOfRange {
            id: "cutoff".into(),
            value: 1.0,
            min: 314.0,
            max: 2394.0,
        };
        assert!(e.to_string().contains("cutoff"));
    }
}
