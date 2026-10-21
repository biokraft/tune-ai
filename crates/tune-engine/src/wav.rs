//! Atomic WAV output: samples go to `<path>.tmp`, which is renamed over
//! `path` only when [`WavWriter::finish`] succeeds.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Sample format of a written file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavFormat {
    /// 24-bit integer PCM; samples are clamped to full scale.
    Pcm24,
    /// 32-bit float; values beyond +-1 are kept.
    Float32,
}

#[derive(Debug, thiserror::Error)]
pub enum WavError {
    #[error("cannot write `{}`: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write `{}`: {source}", path.display())]
    Wav { path: PathBuf, source: hound::Error },
}

/// Streams interleaved samples to a WAV file, atomically.
///
/// Dropping a writer that was not finished removes its `.tmp` file.
pub struct WavWriter {
    path: PathBuf,
    tmp: PathBuf,
    format: WavFormat,
    inner: Option<hound::WavWriter<BufWriter<File>>>,
}

impl WavWriter {
    /// Creates the parent directory and `<path>.tmp`, and writes the header.
    pub fn create(
        path: &Path,
        channels: u16,
        sample_rate: u32,
        format: WavFormat,
    ) -> Result<Self, WavError> {
        let mut tmp_name = path.as_os_str().to_owned();
        tmp_name.push(".tmp");
        let tmp = PathBuf::from(tmp_name);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|source| WavError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let spec = match format {
            WavFormat::Pcm24 => hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 24,
                sample_format: hound::SampleFormat::Int,
            },
            WavFormat::Float32 => hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        };
        let inner = hound::WavWriter::create(&tmp, spec).map_err(|source| {
            let _ = fs::remove_file(&tmp);
            WavError::Wav {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            tmp,
            format,
            inner: Some(inner),
        })
    }

    /// Appends interleaved samples.
    pub fn write(&mut self, samples: &[f32]) -> Result<(), WavError> {
        let w = self.inner.as_mut().expect("writer is open until finish");
        let result = match self.format {
            WavFormat::Pcm24 => samples.iter().try_for_each(|&x| {
                let q = (f64::from(x) * 8_388_607.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                w.write_sample(q)
            }),
            WavFormat::Float32 => samples.iter().try_for_each(|&x| w.write_sample(x)),
        };
        result.map_err(|source| WavError::Wav {
            path: self.path.clone(),
            source,
        })
    }

    /// Finalizes the header and renames the temp file over the target.
    pub fn finish(mut self) -> Result<(), WavError> {
        let inner = self.inner.take().expect("writer is open until finish");
        inner.finalize().map_err(|source| WavError::Wav {
            path: self.path.clone(),
            source,
        })?;
        fs::rename(&self.tmp, &self.path).map_err(|source| WavError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        // Still open: finish was not reached (error or early return).
        if self.inner.take().is_some() {
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

/// Writes `interleaved` in one go. On error nothing is left behind.
pub fn write_wav(
    path: &Path,
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
    format: WavFormat,
) -> Result<(), WavError> {
    let mut w = WavWriter::create(path, channels, sample_rate, format)?;
    w.write(interleaved)?;
    w.finish()
}
