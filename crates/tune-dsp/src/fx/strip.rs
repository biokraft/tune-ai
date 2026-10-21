//! Per-track strip params: sends to the shared buses and kick ducking.

use crate::acid303::math::db2amp;
use crate::param::{Curve, ParamSpec};

/// Send level at or below which the send is exactly off.
pub const SEND_OFF_DB: f64 = -60.0;

/// Index of `send.delay` in [`STRIP_PARAMS`].
pub const STRIP_SEND_DELAY: usize = 0;
/// Index of `send.reverb` in [`STRIP_PARAMS`].
pub const STRIP_SEND_REVERB: usize = 1;
/// Index of `duck.depth` in [`STRIP_PARAMS`].
pub const STRIP_DUCK_DEPTH: usize = 2;
/// Index of `duck.release` in [`STRIP_PARAMS`].
pub const STRIP_DUCK_RELEASE: usize = 3;

/// Strip params every track can carry.
pub const STRIP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "send.delay",
        label: "Delay",
        unit: "dB",
        min: -60.0,
        max: 0.0,
        curve: Curve::Linear,
        default: -60.0,
        help: "How much of this track goes to the shared dotted-eighth delay. -60 is off.",
    },
    ParamSpec {
        id: "send.reverb",
        label: "Reverb",
        unit: "dB",
        min: -60.0,
        max: 0.0,
        curve: Curve::Linear,
        default: -60.0,
        help: "How much goes to the shared reverb. -60 is off.",
    },
    ParamSpec {
        id: "duck.depth",
        label: "Duck",
        unit: "dB",
        min: 0.0,
        max: 24.0,
        curve: Curve::Linear,
        default: 0.0,
        help: "How far this track dips on every kick, so the kick punches through. 0 is off.",
    },
    ParamSpec {
        id: "duck.release",
        label: "Duck release",
        unit: "ms",
        min: 20.0,
        max: 400.0,
        curve: Curve::Exponential,
        default: 80.0,
        help: "How fast the track comes back after the dip. 50-100 ms suits a 303.",
    },
];

/// Linear gain of a send: exactly 0.0 at or below [`SEND_OFF_DB`].
pub fn send_gain(db: f64) -> f64 {
    if db <= SEND_OFF_DB { 0.0 } else { db2amp(db) }
}
