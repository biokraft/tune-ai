//! Audio output for the `tune` tools: the default device through `cpal`, or a
//! `null` device that runs the processor on a plain thread paced at real
//! time. Both implement [`tune_engine::AudioOut`].
//!
//! The audio callback only calls [`tune_engine::LiveProcessor::process`] on
//! preallocated buffers: no allocation, locks or printing.

#![forbid(unsafe_code)]

mod cpal_out;
mod null_out;

use std::str::FromStr;

pub use cpal_out::CpalOut;
pub use null_out::NullOut;
use tune_engine::AudioOut;

/// Which output device to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Default,
    Null,
}

impl Device {
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Null => "null",
        }
    }
}

impl FromStr for Device {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "default" => Ok(Self::Default),
            "null" => Ok(Self::Null),
            other => Err(format!(
                "unknown device `{other}` (expected `default` or `null`)"
            )),
        }
    }
}

/// Builds the output for `device`.
pub fn output(device: Device) -> Box<dyn AudioOut> {
    match device {
        Device::Default => Box::new(CpalOut::new()),
        Device::Null => Box::new(NullOut::new()),
    }
}
