//! The open song: edits in memory, debounced write-back, snapshot undo.
//!
//! The producer's edits land on an in-memory [`SongDoc`] at once and are
//! written back as one atomic file write per burst, 150 ms after the last
//! edit of the burst. Each write is one undo step holding the whole text
//! before and after, so undo restores the file byte for byte, comments
//! included. Every write, undo and redo appends `human` entries to the edit
//! log, one per changed path.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use tune_dsp::DrumPattern;
use tune_dsp::acid303::{AcidPattern, AcidStep};
use tune_song::{PatternSteps, Song};

use crate::command::Command;
use crate::diff::{Change, diff_songs};
use crate::doc::{EditError, SongDoc};
use crate::error::{ErrorView, StudioError};
use crate::log::{EditEntry, EditLog, Source, text_hash};
use crate::path::{SongPath, TrackField};
use crate::set::write_atomic;
use crate::view::SongView;

/// Quiet time after the last edit of a burst before it is written.
pub const WRITE_DEBOUNCE: Duration = Duration::from_millis(150);
/// Most undo steps kept; the oldest goes first.
pub const UNDO_DEPTH: usize = 500;
/// How many of the studio's own recent writes are remembered, so the file
/// watcher can tell them from external edits.
pub const OWN_WRITE_RING: usize = 8;
/// Committed knob and mix values keep this many significant digits.
const SIGNIFICANT_DIGITS: usize = 4;

/// One undo step: the whole file before and after one write burst (or one
/// external change).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UndoEntry {
    pub before: String,
    pub after: String,
    pub source: Source,
    pub changes: Vec<Change>,
}

/// An edit applied in memory but not yet written, with the changes it made
/// at the time. The command is kept so it can be applied again onto a
/// different document (when an external change arrives first).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingEdit {
    pub command: Command,
    pub changes: Vec<Change>,
}

/// The open song file and its edit history.
#[derive(Debug)]
pub struct StudioCore {
    path: PathBuf,
    log: EditLog,
    /// The current document: the disk text plus the pending edits.
    doc: SongDoc,
    /// The text last written or read, and the song it describes.
    disk_text: String,
    disk_song: Song,
    /// Edits since the last flush, oldest first.
    pending: Vec<PendingEdit>,
    last_edit: Option<Instant>,
    /// Hashes of the last [`OWN_WRITE_RING`] texts this core wrote.
    own_writes: VecDeque<String>,
    undo: VecDeque<UndoEntry>,
    redo: Vec<UndoEntry>,
}

impl StudioCore {
    /// Opens the song file at `path`.
    pub fn open(path: &Path) -> Result<Self, StudioError> {
        let text = fs::read_to_string(path)
            .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", path.display())))?;
        let doc = SongDoc::parse(&text)
            .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
        Ok(Self {
            path: path.to_owned(),
            log: EditLog::for_song(path),
            disk_song: doc.song().clone(),
            doc,
            disk_text: text,
            pending: Vec::new(),
            last_edit: None,
            own_writes: VecDeque::with_capacity(OWN_WRITE_RING),
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Applies an edit command in memory and returns what it changed. The
    /// file is written later, by [`flush`](Self::flush). An edit that
    /// changes nothing is not recorded. Commands that are not edits
    /// (transport, song choice, undo, a live knob drag) give
    /// [`StudioError::NotAnEdit`].
    pub fn apply_edit(&mut self, cmd: &Command, now: Instant) -> Result<Vec<Change>, StudioError> {
        let mut doc = self.doc.clone();
        apply_command(&mut doc, cmd)?;
        if doc.text() == self.doc.text() {
            return Ok(Vec::new());
        }
        let changes = diff_songs(self.doc.song(), doc.song());
        self.doc = doc;
        self.pending.push(PendingEdit {
            command: cmd.clone(),
            changes: changes.clone(),
        });
        self.last_edit = Some(now);
        self.redo.clear();
        Ok(changes)
    }

    /// Whether edits are pending and the burst has been quiet for
    /// [`WRITE_DEBOUNCE`].
    pub fn flush_due(&self, now: Instant) -> bool {
        !self.pending.is_empty()
            && self
                .last_edit
                .is_some_and(|t| now.saturating_duration_since(t) >= WRITE_DEBOUNCE)
    }

    /// Writes the pending edits as one atomic write and one undo step, and
    /// logs one `human` entry per changed path (first old value, last new
    /// value). Returns the burst's changes; nothing pending gives none.
    pub fn flush(&mut self) -> Result<Vec<Change>, StudioError> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let text = self.doc.text();
        if text == self.disk_text {
            self.clear_pending();
            return Ok(Vec::new());
        }
        let hash = self.write(&text)?;
        self.clear_pending();
        let changes = diff_songs(&self.disk_song, self.doc.song());
        let before = std::mem::replace(&mut self.disk_text, text.clone());
        self.disk_song = self.doc.song().clone();
        self.push_undo(UndoEntry {
            before,
            after: text,
            source: Source::Human,
            changes: changes.clone(),
        });
        self.redo.clear();
        self.log_human(&changes, &hash)?;
        Ok(changes)
    }

    /// Flushes pending edits, then restores the text before the last undo
    /// step and logs the reversed changes as `human`. Nothing to undo gives
    /// no changes.
    pub fn undo(&mut self) -> Result<Vec<Change>, StudioError> {
        self.flush()?;
        let Some(entry) = self.undo.pop_back() else {
            return Ok(Vec::new());
        };
        match self.restore(&entry.before) {
            Ok((changes, hash)) => {
                self.redo.push(entry);
                self.log_human(&changes, &hash)?;
                Ok(changes)
            }
            Err(e) => {
                self.undo.push_back(entry);
                Err(e)
            }
        }
    }

    /// Re-applies the last undone step (see [`undo`](Self::undo)).
    pub fn redo(&mut self) -> Result<Vec<Change>, StudioError> {
        self.flush()?;
        let Some(entry) = self.redo.pop() else {
            return Ok(Vec::new());
        };
        match self.restore(&entry.after) {
            Ok((changes, hash)) => {
                self.push_undo(entry);
                self.log_human(&changes, &hash)?;
                Ok(changes)
            }
            Err(e) => {
                self.redo.push(entry);
                Err(e)
            }
        }
    }

    /// Whether `bar` (0-based) lies in a section with a lane for this
    /// track's `param`.
    pub fn is_automated(&self, track: &str, param: &str, bar: u32) -> bool {
        let song = self.doc.song();
        song.sections
            .iter()
            .zip(song.section_start_bars())
            .find(|(s, start)| (*start..*start + s.bars).contains(&bar))
            .is_some_and(|(s, _)| {
                s.automation
                    .iter()
                    .any(|l| l.track == track && l.param == param)
            })
    }

    /// The current song, pending edits included.
    pub fn song(&self) -> &Song {
        self.doc.song()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current song as the UI shows it.
    pub fn view(&self) -> SongView {
        SongView::from(self.doc.song())
    }

    /// Whether [`undo`](Self::undo) would change something: there are
    /// pending edits or undo steps.
    pub fn can_undo(&self) -> bool {
        !self.pending.is_empty() || !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The edits not yet written, oldest first.
    pub fn pending(&self) -> &[PendingEdit] {
        &self.pending
    }

    /// Whether `hash` (see [`text_hash`]) is one of the studio's own recent
    /// writes.
    pub fn is_own_write(&self, hash: &str) -> bool {
        self.own_writes.iter().any(|h| h == hash)
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.last_edit = None;
    }

    /// Writes `text` atomically and remembers its hash as an own write.
    fn write(&mut self, text: &str) -> Result<String, StudioError> {
        write_atomic(&self.path, text)
            .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", self.path.display())))?;
        let hash = text_hash(text);
        if self.own_writes.len() == OWN_WRITE_RING {
            self.own_writes.pop_front();
        }
        self.own_writes.push_back(hash.clone());
        Ok(hash)
    }

    /// Writes a snapshot and adopts it as both the document and the disk
    /// text, dropping pending edits (there are none: callers flush first).
    /// Returns the changes from the previous disk text and the new hash.
    fn restore(&mut self, text: &str) -> Result<(Vec<Change>, String), StudioError> {
        let doc = SongDoc::parse(text)
            .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
        let hash = self.write(text)?;
        let changes = diff_songs(&self.disk_song, doc.song());
        self.disk_song = doc.song().clone();
        self.disk_text = text.to_owned();
        self.doc = doc;
        self.clear_pending();
        Ok((changes, hash))
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push_back(entry);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.pop_front();
        }
    }

    fn log_human(&self, changes: &[Change], hash: &str) -> Result<(), StudioError> {
        let ts = Timestamp::now();
        let entries: Vec<EditEntry> = changes
            .iter()
            .map(|c| EditEntry {
                ts,
                source: Source::Human,
                path: c.path.to_string(),
                old: c.old.clone(),
                new: c.new.clone(),
                file_hash: Some(hash.to_owned()),
            })
            .collect();
        self.log.append(&entries).map_err(|e| {
            StudioError::Io(format!(
                "the song was written, but the edit log failed: {e}"
            ))
        })
    }
}

/// Applies one edit command to `doc`, all or nothing. Works on any document,
/// so a pending command can be applied again onto a newer file.
fn apply_command(doc: &mut SongDoc, cmd: &Command) -> Result<(), StudioError> {
    match cmd {
        Command::SetParam {
            track,
            param,
            value,
            commit: true,
        } => {
            let path = SongPath::TrackParam {
                track: track.clone(),
                param: param.clone(),
            };
            doc.set(&path, &Value::from(round_significant(*value)))?;
        }
        Command::SetFxMode { track, mode } => {
            let path = SongPath::FxMode {
                track: track.clone(),
            };
            doc.set(&path, &Value::from(mode.as_str()))?;
        }
        Command::SetAcidStep {
            pattern,
            index,
            note,
            accent,
            slide,
        } => {
            let path = SongPath::Pattern {
                pattern: pattern.clone(),
            };
            let mut steps = match pattern_steps(doc, &path, pattern)? {
                PatternSteps::Acid(p) => p.steps().to_vec(),
                PatternSteps::Drum(_) => return Err(bad(&path, "not an acid pattern").into()),
            };
            let step = step_mut(&mut steps, *index)?;
            *step = AcidStep {
                note: *note,
                accent: note.is_some() && *accent,
                slide: note.is_some() && *slide,
            };
            let p = AcidPattern::new(steps).map_err(|e| bad(&path, &e.to_string()))?;
            doc.set(&path, &Value::from(p.to_string()))?;
        }
        Command::SetDrumStep {
            pattern,
            index,
            hit,
        } => {
            let path = SongPath::Pattern {
                pattern: pattern.clone(),
            };
            let mut hits = match pattern_steps(doc, &path, pattern)? {
                PatternSteps::Drum(p) => p.hits().to_vec(),
                PatternSteps::Acid(_) => return Err(bad(&path, "not a drum pattern").into()),
            };
            *step_mut(&mut hits, *index)? = (*hit).into();
            let p = DrumPattern::new(hits).map_err(|e| bad(&path, &e.to_string()))?;
            doc.set(&path, &Value::from(p.to_string()))?;
        }
        Command::MoveSection { from, to } => doc.move_section(*from, *to)?,
        Command::ResizeSection { index, bars } => {
            doc.set(&SongPath::SectionBars(*index), &Value::from(*bars))?;
        }
        Command::DuplicateSection { index } => doc.duplicate_section(*index)?,
        Command::DeleteSection { index } => doc.delete_section(*index)?,
        Command::SetSectionPlay {
            section,
            track,
            pattern,
        } => {
            let path = SongPath::SectionPlay {
                section: *section,
                track: track.clone(),
            };
            let value = pattern.as_deref().map_or(Value::Null, Value::from);
            doc.set(&path, &value)?;
        }
        Command::SetTrackMix {
            track,
            gain_db,
            pan,
            mute,
            solo,
        } => {
            // One copy for all fields, so a refused field leaves none set.
            let mut next = doc.clone();
            let field = |field| SongPath::TrackField {
                track: track.clone(),
                field,
            };
            let numbers = [(TrackField::GainDb, gain_db), (TrackField::Pan, pan)];
            for (f, v) in numbers {
                if let Some(v) = v {
                    next.set(&field(f), &Value::from(round_significant(*v)))?;
                }
            }
            for (f, v) in [(TrackField::Mute, mute), (TrackField::Solo, solo)] {
                if let Some(v) = v {
                    next.set(&field(f), &Value::Bool(*v))?;
                }
            }
            *doc = next;
        }
        Command::SetParam { commit: false, .. }
        | Command::ListSongs
        | Command::OpenSong { .. }
        | Command::Play { .. }
        | Command::Stop
        | Command::SeekBar { .. }
        | Command::SetLoop { .. }
        | Command::Undo
        | Command::Redo
        | Command::ResolveOffered { .. } => return Err(StudioError::NotAnEdit),
    }
    Ok(())
}

fn pattern_steps<'d>(
    doc: &'d SongDoc,
    path: &SongPath,
    pattern: &str,
) -> Result<&'d PatternSteps, EditError> {
    doc.song()
        .patterns
        .get(pattern)
        .map(|p| &p.steps)
        .ok_or_else(|| EditError::NoSuchTarget(path.to_string()))
}

fn step_mut<T>(steps: &mut [T], index: usize) -> Result<&mut T, EditError> {
    let len = steps.len();
    steps
        .get_mut(index)
        .ok_or(EditError::OutOfBounds { index, len })
}

fn bad(path: &SongPath, message: &str) -> EditError {
    EditError::BadValue {
        path: path.to_string(),
        message: message.to_owned(),
    }
}

/// `v` rounded to [`SIGNIFICANT_DIGITS`] significant digits, as the nearest
/// double to the short decimal (`823.456789` gives `823.5`).
fn round_significant(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 {
        return v;
    }
    format!("{v:.prec$e}", prec = SIGNIFICANT_DIGITS - 1)
        .parse()
        .unwrap_or(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_to_four_significant_digits() {
        for (v, want) in [
            (823.456789, 823.5),
            (2394.0, 2394.0),
            (12_346.0, 12_350.0),
            (-6.123456, -6.123),
            (0.333333, 0.3333),
            (0.000_123_456, 0.000_123_5),
            (500.0, 500.0),
            (0.0, 0.0),
        ] {
            assert_eq!(round_significant(v), want, "{v}");
        }
        assert!(round_significant(f64::NAN).is_nan());
    }
}
