use std::fs::File;
use std::ops::Range;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::AnalysisError;

/// Planar f32 audio: one or two channels of equal length.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Split interleaved samples into planar channels.
    pub fn from_interleaved(
        sample_rate: u32,
        channels: u16,
        samples: &[f32],
    ) -> Result<Audio, AnalysisError> {
        if sample_rate == 0 {
            return Err(AnalysisError::Invalid("sample rate is 0".into()));
        }
        if channels == 0 {
            return Err(AnalysisError::Invalid("audio has no channels".into()));
        }
        if channels > 2 {
            return Err(AnalysisError::Channels(channels));
        }
        let n = channels as usize;
        if samples.is_empty() {
            return Err(AnalysisError::Invalid("audio is empty".into()));
        }
        if !samples.len().is_multiple_of(n) {
            return Err(AnalysisError::Invalid(format!(
                "{} samples do not divide into {n} channels",
                samples.len()
            )));
        }
        let planar = (0..n)
            .map(|c| samples.iter().skip(c).step_by(n).copied().collect())
            .collect();
        Ok(Audio {
            sample_rate,
            channels: planar,
        })
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    pub fn is_stereo(&self) -> bool {
        self.channels.len() == 2
    }

    /// The frames in `frames`, clamped to the end of the audio.
    pub fn slice(&self, frames: Range<usize>) -> Audio {
        let end = frames.end.min(self.frames());
        let start = frames.start.min(end);
        Audio {
            sample_rate: self.sample_rate,
            channels: self
                .channels
                .iter()
                .map(|c| c[start..end].to_vec())
                .collect(),
        }
    }
}

/// Decode a file: `.wav` through hound, everything else through symphonia.
pub fn decode_file(path: &Path) -> Result<Audio, AnalysisError> {
    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if is_wav {
        decode_wav(path)
    } else {
        decode_symphonia(path)
    }
}

fn open_err(path: &Path, e: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Open {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

fn decode_err(path: &Path, e: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Decode {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

fn decode_wav(path: &Path) -> Result<Audio, AnalysisError> {
    let file = File::open(path).map_err(|e| open_err(path, e))?;
    let mut reader =
        hound::WavReader::new(std::io::BufReader::new(file)).map_err(|e| decode_err(path, e))?;
    let spec = reader.spec();
    if spec.channels > 2 {
        return Err(AnalysisError::Channels(spec.channels));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| decode_err(path, e))?,
        hound::SampleFormat::Int => {
            if !matches!(spec.bits_per_sample, 8 | 16 | 24 | 32) {
                return Err(decode_err(
                    path,
                    format!("unsupported bit depth {}", spec.bits_per_sample),
                ));
            }
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f64;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| (f64::from(v) * scale) as f32))
                .collect::<Result<_, _>>()
                .map_err(|e| decode_err(path, e))?
        }
    };
    if samples.is_empty() {
        return Err(decode_err(path, "no audio frames"));
    }
    Audio::from_interleaved(spec.sample_rate, spec.channels, &samples)
}

fn decode_symphonia(path: &Path) -> Result<Audio, AnalysisError> {
    let file = File::open(path).map_err(|e| open_err(path, e))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &format_opts, &MetadataOptions::default())
        .map_err(|e| decode_err(path, e))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| decode_err(path, "no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| decode_err(path, e))?;

    let mut rate = 0u32;
    let mut chans = 0u16;
    let mut samples: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(decode_err(path, e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(decode_err(path, e)),
        };
        let spec = *decoded.spec();
        if rate == 0 {
            rate = spec.rate;
            chans = spec.channels.count() as u16;
            if chans > 2 {
                return Err(AnalysisError::Channels(chans));
            }
        }
        if buf
            .as_ref()
            .is_none_or(|b| b.capacity() < decoded.capacity() * chans as usize)
        {
            buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let b = buf.as_mut().expect("buffer set above");
        b.copy_interleaved_ref(decoded);
        samples.extend_from_slice(b.samples());
    }
    if samples.is_empty() {
        return Err(decode_err(path, "no audio frames"));
    }
    Audio::from_interleaved(rate, chans, &samples)
}
