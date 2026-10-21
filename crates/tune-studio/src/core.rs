//! The open song: edits in memory, debounced write-back, snapshot undo.
//!
//! The producer's edits land on an in-memory [`SongDoc`] at once and are
//! written back as one atomic file write per burst, 150 ms after the last
//! edit of the burst. Each write is one undo step holding the whole text
//! before and after, so undo restores the file byte for byte, comments
//! included. Every write, undo and redo appends `human` entries to the edit
//! log, one per changed path.
//!
//! The file also changes from outside: Claude through `tune set` or by
//! editing it, an editor, git. [`StudioCore::on_disk_text`] takes each new
//! text: the studio's own writes are ignored (unless the file holds the same
//! bytes again), Claude's are recognised by the hash `tune set` logged, and
//! anything else is logged as `external`. Every
//! external change is one undo step. Pending edits are merged onto the new
//! text by path; an edit to a path the external change also touched keeps
//! the external value and is offered back for re-apply, so the producer's
//! edits are never silently lost. While the file does not parse, the last
//! good song and the pending edits are kept and nothing is written.

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
use crate::diff::{Change, diff_songs, is_whole_replace};
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

/// What a change on disk did to the open song.
#[derive(Clone, Debug, Serialize)]
pub struct ExternalChange {
    /// `claude` when the edit log holds a `claude` entry with the file's
    /// hash, otherwise `external`.
    pub source: Source,
    /// The differences from the last text on disk.
    pub changes: Vec<Change>,
    /// The file was replaced by a different song, so every pending edit
    /// was offered instead of merged.
    pub replaced: bool,
    /// Pending edits applied again onto the new text, and written.
    pub merged: Vec<Change>,
    /// Pending edits the change overrode or made impossible, offered for
    /// re-apply (see [`StudioCore::resolve_offered`]).
    pub offered: Vec<Change>,
    /// Why the new text does not parse; empty when it does. When set, the
    /// other fields are empty and the last good song stays open.
    pub errors: Vec<ErrorView>,
}

impl ExternalChange {
    fn new(source: Source) -> Self {
        Self {
            source,
            changes: Vec::new(),
            replaced: false,
            merged: Vec::new(),
            offered: Vec::new(),
            errors: Vec::new(),
        }
    }
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
    /// Local edits an external change overrode, kept for re-apply, and
    /// their changes for display.
    offered: Vec<PendingEdit>,
    offered_changes: Vec<Change>,
    /// Why the text on disk does not parse; empty when it does.
    file_errors: Vec<ErrorView>,
    /// An external change that [`flush`](Self::flush) took in before the
    /// watcher reported it, waiting to be announced.
    unreported: Option<ExternalChange>,
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
            offered: Vec::new(),
            offered_changes: Vec::new(),
            file_errors: Vec::new(),
            unreported: None,
        })
    }

    /// Applies an edit command in memory and returns what it changed. The
    /// file is written later, by [`flush`](Self::flush). An edit that
    /// changes nothing is not recorded. Commands that are not edits
    /// (transport, song choice, undo, a live knob drag) give
    /// [`StudioError::NotAnEdit`]; while the file on disk does not parse,
    /// every edit gives [`StudioError::FileInvalid`].
    pub fn apply_edit(&mut self, cmd: &Command, now: Instant) -> Result<Vec<Change>, StudioError> {
        if !self.file_errors.is_empty() {
            return Err(StudioError::FileInvalid);
        }
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
    ///
    /// It first reads the file, and a text not seen yet goes through
    /// [`on_disk_text`](Self::on_disk_text), so an external change the
    /// watcher has not reported is merged, never overwritten, and kept for
    /// [`take_unreported_external`](Self::take_unreported_external). While
    /// the file does not parse, nothing is written.
    pub fn flush(&mut self) -> Result<Vec<Change>, StudioError> {
        let mut written = Vec::new();
        if let Ok(text) = fs::read_to_string(&self.path)
            && let Some(ext) = self.on_disk_text(text)
        {
            written.clone_from(&ext.merged);
            self.hold_unreported(ext);
        }
        if !self.file_errors.is_empty() {
            return Ok(Vec::new());
        }
        written.extend(self.write_pending()?);
        Ok(written)
    }

    /// [`flush`](Self::flush) without reading the file first.
    fn write_pending(&mut self) -> Result<Vec<Change>, StudioError> {
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

    /// Takes a new text of the song file (from the watcher, or read by
    /// [`flush`](Self::flush)). Gives `None` for the text already open and
    /// for a late report of the studio's own write once the file has moved
    /// past it.
    ///
    /// A text that does not parse sets [`file_errors`](Self::file_errors)
    /// and keeps the last good song and the pending edits. Otherwise the
    /// text becomes one undo step and is logged (unless Claude's `tune set`
    /// logged it already). Pending edits that touch none of its paths are
    /// applied again on top and written at once, as their own `human` undo
    /// step; the others are offered. A text that replaces the whole song
    /// offers every pending edit.
    pub fn on_disk_text(&mut self, text: String) -> Option<ExternalChange> {
        let hash = text_hash(&text);
        if text == self.disk_text || self.is_stale_own_write(&text, &hash) {
            // An invalid save put back as it was: report the errors gone.
            if self.file_errors.is_empty() {
                return None;
            }
            self.file_errors.clear();
            return Some(ExternalChange::new(Source::External));
        }
        let doc = match SongDoc::parse(&text) {
            Ok(doc) => doc,
            Err(errs) => {
                self.file_errors = errs.iter().map(ErrorView::from).collect();
                return Some(ExternalChange {
                    errors: self.file_errors.clone(),
                    ..ExternalChange::new(Source::External)
                });
            }
        };
        self.file_errors.clear();

        let source = if self.log.has_claude_hash(&hash).unwrap_or(false) {
            Source::Claude
        } else {
            Source::External
        };
        let changes = diff_songs(&self.disk_song, doc.song());
        if source == Source::External {
            // Best effort: the change is adopted even if the log is not
            // writable, as it is already on disk.
            let _ = self.log_changes(Source::External, &changes, &hash);
        }
        let replaced = is_whole_replace(&self.disk_text, &text);
        let before = std::mem::replace(&mut self.disk_text, text.clone());
        self.disk_song = doc.song().clone();
        self.push_undo(UndoEntry {
            before,
            after: text,
            source,
            changes: changes.clone(),
        });
        self.redo.clear();

        let mut ext = ExternalChange {
            changes,
            ..ExternalChange::new(source)
        };
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            self.doc = doc;
        } else if replaced {
            self.doc = doc;
            self.last_edit = None;
            ext.replaced = true;
            ext.offered = self.offer(pending);
        } else {
            self.merge(doc, pending, &mut ext);
        }
        Some(ext)
    }

    /// Applies each pending edit again onto `doc`, in order, unless it
    /// touches a path the external change (or an edit offered before it)
    /// touched, or no longer applies; those are offered. Writes the result.
    fn merge(&mut self, doc: SongDoc, pending: Vec<PendingEdit>, ext: &mut ExternalChange) {
        let mut touched: Vec<SongPath> = ext.changes.iter().map(|c| c.path.clone()).collect();
        let mut next = doc;
        let mut offered = Vec::new();
        for edit in pending {
            let conflicts = edit
                .changes
                .iter()
                .any(|c| touched.iter().any(|p| paths_overlap(p, &c.path)));
            if !conflicts {
                let mut trial = next.clone();
                if apply_command(&mut trial, &edit.command).is_ok() {
                    if trial.text() != next.text() {
                        let changes = diff_songs(next.song(), trial.song());
                        ext.merged.extend(changes.iter().cloned());
                        self.pending.push(PendingEdit {
                            command: edit.command,
                            changes,
                        });
                        next = trial;
                    }
                    continue;
                }
            }
            // Later edits may build on this one (a resize of a duplicated
            // section), so its paths count as touched too.
            touched.extend(edit.changes.iter().map(|c| c.path.clone()));
            offered.push(edit);
        }
        self.doc = next;
        ext.offered = self.offer(offered);
        if self.pending.is_empty() {
            self.last_edit = None;
        } else {
            // On a failed write the edits stay pending and the next due
            // flush tries again.
            let _ = self.write_pending();
        }
    }

    /// The external change [`flush`](Self::flush) (and so
    /// [`undo`](Self::undo), [`redo`](Self::redo) and
    /// [`resolve_offered`](Self::resolve_offered)) found on disk before the
    /// watcher reported it, if any; taken once. The watcher's own report of
    /// that text comes too late to tell (the file then holds the studio's
    /// write), so the caller announces this one instead.
    pub fn take_unreported_external(&mut self) -> Option<ExternalChange> {
        self.unreported.take()
    }

    /// Adds `ext` to the change waiting to be announced.
    fn hold_unreported(&mut self, ext: ExternalChange) {
        let Some(held) = &mut self.unreported else {
            self.unreported = Some(ext);
            return;
        };
        held.source = ext.source;
        held.changes.extend(ext.changes);
        held.replaced |= ext.replaced;
        held.merged.extend(ext.merged);
        held.offered.extend(ext.offered);
        held.errors = ext.errors;
    }

    /// Keeps `edits` for re-apply and returns their changes.
    fn offer(&mut self, edits: Vec<PendingEdit>) -> Vec<Change> {
        let changes: Vec<Change> = edits.iter().flat_map(|e| e.changes.clone()).collect();
        self.offered_changes.extend(changes.iter().cloned());
        self.offered.extend(edits);
        changes
    }

    /// Re-applies the offered edits that still apply, as one `human` write
    /// (`reapply`), or drops them all. Returns what the re-apply changed.
    pub fn resolve_offered(&mut self, reapply: bool) -> Result<Vec<Change>, StudioError> {
        if !reapply {
            self.offered.clear();
            self.offered_changes.clear();
            return Ok(Vec::new());
        }
        if !self.file_errors.is_empty() {
            return Err(StudioError::FileInvalid);
        }
        let offered = std::mem::take(&mut self.offered);
        self.offered_changes.clear();
        let start = self.doc.song().clone();
        let mut applied = false;
        for edit in offered {
            let mut trial = self.doc.clone();
            if apply_command(&mut trial, &edit.command).is_err() || trial.text() == self.doc.text()
            {
                continue;
            }
            let changes = diff_songs(self.doc.song(), trial.song());
            self.doc = trial;
            self.pending.push(PendingEdit {
                command: edit.command,
                changes,
            });
            applied = true;
        }
        if !applied {
            return Ok(Vec::new());
        }
        self.redo.clear();
        self.last_edit.get_or_insert_with(Instant::now);
        self.flush()?;
        Ok(diff_songs(&start, self.doc.song()))
    }

    /// The changes of the offered edits, for display.
    pub fn offered(&self) -> &[Change] {
        &self.offered_changes
    }

    /// Why the file on disk does not parse; empty when it does.
    pub fn file_errors(&self) -> &[ErrorView] {
        &self.file_errors
    }

    /// Flushes pending edits, then restores the text before the last undo
    /// step and logs the reversed changes as `human`. Nothing to undo gives
    /// no changes.
    pub fn undo(&mut self) -> Result<Vec<Change>, StudioError> {
        self.flush()?;
        if !self.file_errors.is_empty() {
            return Err(StudioError::FileInvalid);
        }
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
        if !self.file_errors.is_empty() {
            return Err(StudioError::FileInvalid);
        }
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

    /// Whether `text` is a late report of one of the studio's own writes
    /// that the file has since moved past. Matching an own write's hash is
    /// not enough: Claude or an editor can write the same bytes again (undo
    /// back past a `tune set`, then run it again), and then the file really
    /// holds them, so they are a change.
    fn is_stale_own_write(&self, text: &str, hash: &str) -> bool {
        self.is_own_write(hash) && !fs::read_to_string(&self.path).is_ok_and(|now| now == text)
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
        self.log_changes(Source::Human, changes, hash)
    }

    fn log_changes(
        &self,
        source: Source,
        changes: &[Change],
        hash: &str,
    ) -> Result<(), StudioError> {
        let ts = Timestamp::now();
        let entries: Vec<EditEntry> = changes
            .iter()
            .map(|c| EditEntry {
                ts,
                source,
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
            let path = SongPath::param(track, param);
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
        | Command::GetState
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

/// Whether an edit of one path can change the value at the other: the
/// same path, or one holds the other (`sections` holds `sections[1].bars`,
/// `tracks.acid` holds `tracks.acid.gain_db`).
fn paths_overlap(a: &SongPath, b: &SongPath) -> bool {
    a == b || holds(a, b) || holds(b, a)
}

fn holds(whole: &SongPath, part: &SongPath) -> bool {
    match (whole, part) {
        (
            SongPath::Sections,
            SongPath::SectionBars(_)
            | SongPath::SectionName(_)
            | SongPath::SectionPlay { .. }
            | SongPath::SectionLanes(_),
        ) => true,
        (
            SongPath::Track { track },
            SongPath::TrackField { track: t, .. }
            | SongPath::TrackParam { track: t, .. }
            | SongPath::FxMode { track: t }
            | SongPath::SectionPlay { track: t, .. },
        ) => t == track,
        _ => false,
    }
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
