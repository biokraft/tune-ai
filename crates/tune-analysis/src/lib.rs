//! Headless audio analysis: the ears of tune-ai.
#![forbid(unsafe_code)]

mod audio;
mod canvas;
mod compare;
mod loudness;
mod report;
mod sections;
mod spectral;
pub mod spectrogram;

pub use audio::{Audio, decode_file};
pub use compare::{Comparison, Deltas, compare};
pub use loudness::{Loudness, measure_loudness};
pub use report::{ACID_LUFS, Analysis, TRUE_PEAK_MAX_DBTP, Targets, analyze, classify_targets};
pub use sections::{BarMap, SectionReport, SectionSpan, analyze_sections};
pub use spectral::{BANDS, Band, Brightness, LowEnd, Spectral, Stereo, measure_spectral};

/// Everything that can go wrong while reading or measuring audio.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("cannot open `{path}`: {message}")]
    Open { path: String, message: String },
    #[error("cannot decode `{path}`: {message}")]
    Decode { path: String, message: String },
    #[error("only mono or stereo audio is supported (file has {0} channels)")]
    Channels(u16),
    /// Empty audio, unequal channel lengths, sample rate 0.
    #[error("{0}")]
    Invalid(String),
    #[error("loudness meter: {0}")]
    Meter(String),
}
