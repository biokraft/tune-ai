//! Offline rendering: whole songs, bar ranges and per-track stems.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::compile::CompiledSong;
use crate::engine::Engine;
use crate::mixer::Meter;
use crate::wav::{WavError, WavFormat, WavWriter};

/// Frames per engine call.
const CHUNK_FRAMES: usize = 4096;

/// What to render. Bars are 0-based and the range is half-open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderOptions {
    pub start_bar: u32,
    pub end_bar: u32,
    pub stems: bool,
}

/// Interleaved stereo audio from [`render`].
#[derive(Clone, Debug)]
pub struct Rendered {
    pub sample_rate: u32,
    pub mix: Vec<f32>,
    /// `(track id, samples)` in song order; empty unless stems were asked for.
    pub stems: Vec<(String, Vec<f32>)>,
    pub meter: Meter,
}

/// What [`render_to_files`] wrote.
#[derive(Clone, Debug)]
pub struct RenderReport {
    pub files: Vec<PathBuf>,
    pub frames: u64,
    pub seconds: f64,
    pub meter: Meter,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("bar range {start}..{end} is invalid for a song of {total} bars")]
    BadRange { start: u32, end: u32, total: u32 },
    #[error(transparent)]
    Wav(#[from] WavError),
}

fn check_range(song: &CompiledSong, opts: &RenderOptions) -> Result<(), RenderError> {
    if opts.start_bar >= opts.end_bar || opts.end_bar > song.total_bars {
        return Err(RenderError::BadRange {
            start: opts.start_bar,
            end: opts.end_bar,
            total: song.total_bars,
        });
    }
    Ok(())
}

/// Runs the engine over the range and hands each chunk to `sink` as the mix
/// plus one stem buffer per track (empty without stems). Only the first
/// `mix.len()` samples of a stem buffer are valid. Buffers are allocated once.
fn run(
    song: Arc<CompiledSong>,
    opts: &RenderOptions,
    mut sink: impl FnMut(&[f32], &[Vec<f32>]) -> Result<(), RenderError>,
) -> Result<(u64, Meter), RenderError> {
    check_range(&song, opts)?;
    let frames = song.clock.bar_start(opts.end_bar) - song.clock.bar_start(opts.start_bar);
    let n_stems = if opts.stems { song.tracks.len() } else { 0 };
    let mut engine = Engine::new(song);
    engine.seek(opts.start_bar);
    let mut mix = vec![0.0f32; 2 * CHUNK_FRAMES];
    let mut stems = vec![vec![0.0f32; 2 * CHUNK_FRAMES]; n_stems];
    let mut done = 0u64;
    while done < frames {
        let n = (frames - done).min(CHUNK_FRAMES as u64) as usize;
        let m = &mut mix[..2 * n];
        if opts.stems {
            let mut views: Vec<&mut [f32]> = stems.iter_mut().map(|s| &mut s[..2 * n]).collect();
            engine.process_stems(m, &mut views);
        } else {
            engine.process(m);
        }
        sink(m, &stems)?;
        done += n as u64;
    }
    Ok((frames, engine.meter()))
}

/// Renders the range to memory.
pub fn render(song: Arc<CompiledSong>, opts: &RenderOptions) -> Result<Rendered, RenderError> {
    let sample_rate = song.clock.sample_rate;
    let ids: Vec<String> = if opts.stems {
        song.tracks.iter().map(|t| t.id.clone()).collect()
    } else {
        Vec::new()
    };
    let mut out_mix = Vec::new();
    let mut out_stems: Vec<Vec<f32>> = vec![Vec::new(); ids.len()];
    let (_, meter) = run(song, opts, |mix, stems| {
        out_mix.extend_from_slice(mix);
        for (dst, src) in out_stems.iter_mut().zip(stems) {
            dst.extend_from_slice(&src[..mix.len()]);
        }
        Ok(())
    })?;
    Ok(Rendered {
        sample_rate,
        mix: out_mix,
        stems: ids.into_iter().zip(out_stems).collect(),
        meter,
    })
}

/// Renders the range straight to WAV files. Without stems, `out` is the mix
/// file; with stems, `out` is a directory receiving `mix.wav` (24-bit) and
/// `<track id>.wav` (32-bit float). Files appear only when everything is
/// written; on error no `.tmp` is left.
pub fn render_to_files(
    song: Arc<CompiledSong>,
    opts: &RenderOptions,
    out: &Path,
) -> Result<RenderReport, RenderError> {
    check_range(&song, opts)?;
    let sr = song.clock.sample_rate;
    let mut files = Vec::new();
    let mut writers: Vec<WavWriter> = Vec::new();
    if opts.stems {
        let mix_path = out.join("mix.wav");
        writers.push(WavWriter::create(&mix_path, 2, sr, WavFormat::Pcm24)?);
        files.push(mix_path);
        for t in &song.tracks {
            let p = out.join(format!("{}.wav", t.id));
            writers.push(WavWriter::create(&p, 2, sr, WavFormat::Float32)?);
            files.push(p);
        }
    } else {
        writers.push(WavWriter::create(out, 2, sr, WavFormat::Pcm24)?);
        files.push(out.to_path_buf());
    }
    let (frames, meter) = run(song, opts, |mix, stems| {
        writers[0].write(mix)?;
        for (w, s) in writers[1..].iter_mut().zip(stems) {
            w.write(&s[..mix.len()])?;
        }
        Ok(())
    })?;
    for w in writers {
        w.finish()?;
    }
    Ok(RenderReport {
        files,
        frames,
        seconds: frames as f64 / f64::from(sr),
        meter,
    })
}
