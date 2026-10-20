//! Deterministic offline rendering of an acid 303 pattern to samples and WAV.

use std::fs;
use std::path::{Path, PathBuf};

use tune_dsp::acid303::{ACID303_PARAMS, Acid303, AcidPattern, AcidPlayer};
use tune_dsp::{DIST_PARAMS, Distortion, DistortionMode, ParamError, ParamSpec};

/// A linear parameter sweep in normalized (0..=1) space over the whole render.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    pub id: String,
    pub from_norm: f64,
    pub to_norm: f64,
}

/// Everything needed to render a pattern; the same spec gives the same samples.
#[derive(Clone, Debug)]
pub struct RenderSpec {
    pub pattern: AcidPattern,
    pub bpm: f64,
    pub bars: u32,
    pub sample_rate: u32,
    /// Plain values, ids starting `acid303.` or `dist.`.
    pub params: Vec<(String, f64)>,
    pub sweeps: Vec<Sweep>,
    /// `None` bypasses the distortion.
    pub distortion: Option<DistortionMode>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Param(#[from] ParamError),
    #[error("unknown parameter id `{0}`")]
    UnknownParam(String),
    #[error("sweep `{id}`: from/to must be within 0..=1, got {from}:{to}")]
    SweepRange { id: String, from: f64, to: f64 },
    #[error("invalid {what}: {value}")]
    InvalidSpec { what: &'static str, value: String },
    #[error("cannot write `{}`: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write `{}`: {source}", path.display())]
    Wav { path: PathBuf, source: hound::Error },
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Voice,
    Dist,
}

fn route(id: &str) -> Result<(Target, &'static ParamSpec), RenderError> {
    let (target, table) = if id.starts_with("acid303.") {
        (Target::Voice, ACID303_PARAMS)
    } else if id.starts_with("dist.") {
        (Target::Dist, DIST_PARAMS)
    } else {
        return Err(RenderError::UnknownParam(id.to_string()));
    };
    table
        .iter()
        .find(|s| s.id == id)
        .map(|s| (target, s))
        .ok_or_else(|| RenderError::UnknownParam(id.to_string()))
}

fn apply(
    voice: &mut Acid303,
    dist: &mut Distortion,
    target: Target,
    id: &str,
    plain: f64,
) -> Result<(), ParamError> {
    match target {
        Target::Voice => voice.set_param(id, plain),
        Target::Dist => dist.set_param(id, plain),
    }
}

/// Sample rates the voice is known to handle (the wavetable needs a sane Nyquist).
const MIN_SAMPLE_RATE: std::ops::RangeInclusive<u32> = 8_000..=384_000;
/// Longest render accepted, in seconds of audio.
const MAX_RENDER_SECONDS: f64 = 30.0 * 60.0;

/// Renders `spec` to mono samples (`round(bars * 4 * 60 / bpm * sample_rate)` of them).
pub fn render_acid(spec: &RenderSpec) -> Result<Vec<f32>, RenderError> {
    if !(spec.bpm.is_finite() && spec.bpm > 0.0) {
        return Err(RenderError::InvalidSpec {
            what: "bpm",
            value: spec.bpm.to_string(),
        });
    }
    if spec.bars == 0 {
        return Err(RenderError::InvalidSpec {
            what: "bars",
            value: "0".into(),
        });
    }
    if !MIN_SAMPLE_RATE.contains(&spec.sample_rate) {
        return Err(RenderError::InvalidSpec {
            what: "sample rate",
            value: format!(
                "{} (valid range is {}..={})",
                spec.sample_rate,
                MIN_SAMPLE_RATE.start(),
                MIN_SAMPLE_RATE.end()
            ),
        });
    }
    let sr = f64::from(spec.sample_rate);
    let len_f = (f64::from(spec.bars) * 4.0 * 60.0 / spec.bpm * sr).round();
    if len_f > MAX_RENDER_SECONDS * sr {
        return Err(RenderError::InvalidSpec {
            what: "render length",
            value: format!(
                "{:.0}s of audio is too long (limit {MAX_RENDER_SECONDS:.0}s)",
                len_f / sr
            ),
        });
    }
    let mut voice = Acid303::with_param_defaults(sr);
    let mut dist = Distortion::new(sr);
    for (id, value) in &spec.params {
        let (target, _) = route(id)?;
        apply(&mut voice, &mut dist, target, id, *value)?;
    }
    let mut sweeps = Vec::with_capacity(spec.sweeps.len());
    for s in &spec.sweeps {
        let (target, pspec) = route(&s.id)?;
        let ok = |x: f64| (0.0..=1.0).contains(&x);
        if !ok(s.from_norm) || !ok(s.to_norm) {
            return Err(RenderError::SweepRange {
                id: s.id.clone(),
                from: s.from_norm,
                to: s.to_norm,
            });
        }
        sweeps.push((target, pspec, s));
    }
    if let Some(mode) = spec.distortion {
        dist.set_mode(mode);
    }

    let len = len_f as usize;
    let mut player = AcidPlayer::new(spec.pattern.clone(), spec.bpm, sr);
    let mut out = Vec::with_capacity(len);
    for n in 0..len {
        let t = if len > 1 {
            n as f64 / (len - 1) as f64
        } else {
            0.0
        };
        for (target, pspec, s) in &sweeps {
            let norm = s.from_norm + (s.to_norm - s.from_norm) * t;
            apply(&mut voice, &mut dist, *target, &s.id, pspec.to_plain(norm))?;
        }
        player.tick(&mut voice);
        let mut x = voice.get_sample();
        if spec.distortion.is_some() {
            x = dist.process(x);
        }
        if !x.is_finite() {
            return Err(RenderError::InvalidSpec {
                what: "render",
                value: format!("non-finite output at sample {n}"),
            });
        }
        out.push(x as f32);
    }
    Ok(out)
}

/// Writes dual-mono 24-bit PCM. Goes through `<path>.tmp`; on error nothing is left behind.
pub fn write_wav(path: &Path, mono: &[f32], sample_rate: u32) -> Result<(), RenderError> {
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let result = write_inner(&tmp, mono, sample_rate).and_then(|()| {
        fs::rename(&tmp, path).map_err(|source| RenderError::Io {
            path: path.to_path_buf(),
            source,
        })
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| match e {
        RenderError::Wav { source, .. } => RenderError::Wav {
            path: path.to_path_buf(),
            source,
        },
        other => other,
    })
}

fn write_inner(tmp: &Path, mono: &[f32], sample_rate: u32) -> Result<(), RenderError> {
    let wav_spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let wav = |source| RenderError::Wav {
        path: tmp.to_path_buf(),
        source,
    };
    let mut w = hound::WavWriter::create(tmp, wav_spec).map_err(wav)?;
    for &x in mono {
        let q = (f64::from(x) * 8_388_607.0)
            .round()
            .clamp(-8_388_608.0, 8_388_607.0) as i32;
        w.write_sample(q).map_err(wav)?;
        w.write_sample(q).map_err(wav)?;
    }
    w.finalize().map_err(wav)
}
