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
//!
//! A song file in an older format is migrated when opened: the new text is
//! written at once, logged as one `external` `song.version` change and kept
//! as one undo step, so undo puts the old bytes back. The document in
//! memory stays migrated, so the next edit writes the newest format again.
//! An older-format text that arrives from outside is read migrated and not
//! written back.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tune_song::{FORMAT_VERSION, Song};

use crate::action::{Action, ApplyCtx, apply_action};
use crate::diff::{Change, diff_files, diff_songs, is_whole_replace, version_change};
use crate::doc::SongDoc;
use crate::error::{ErrorView, StudioError};
use crate::log::{EditLog, Source, text_hash};
use crate::path::SongPath;
use crate::set::write_atomic;
use crate::view::SongView;

/// Quiet time after the last edit of a burst before it is written.
pub const WRITE_DEBOUNCE: Duration = Duration::from_millis(150);
/// Most undo steps kept; the oldest goes first.
pub const UNDO_DEPTH: usize = 500;
/// How many of the studio's own recent writes are remembered, so the file
/// watcher can tell them from external edits.
pub const OWN_WRITE_RING: usize = 8;

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
/// at the time. The action is kept so it can be applied again onto a
/// different document (when an external change arrives first).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingEdit {
    pub action: Action,
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
    /// The text last written or read, the format version it is in and the
    /// song it describes.
    disk_text: String,
    disk_version: u32,
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
    /// The version the file was migrated from when opened, and the undo
    /// step the migration made.
    migration: Option<(u32, UndoEntry)>,
    /// Why writing the open-time migration back (or logging it) failed,
    /// until the next successful write.
    migration_error: Option<String>,
}

impl StudioCore {
    /// Opens the song file at `path`. A file in an older format is migrated
    /// and written back (see the module docs); when that write fails, the
    /// song still opens, migrated in memory, and
    /// [`migration_error`](Self::migration_error) says why.
    pub fn open(path: &Path) -> Result<Self, StudioError> {
        let text = fs::read_to_string(path)
            .map_err(|e| StudioError::Io(format!("cannot read {}: {e}", path.display())))?;
        let doc = SongDoc::parse(&text)
            .map_err(|errs| StudioError::Load(errs.iter().map(ErrorView::from).collect()))?;
        let migrated_from = doc.migrated_from();
        let mut core = Self {
            path: path.to_owned(),
            log: EditLog::for_song(path),
            disk_song: doc.song().clone(),
            disk_version: text_version(&doc),
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
            migration: None,
            migration_error: None,
        };
        if let Some(from) = migrated_from {
            core.write_migration(from);
        }
        Ok(core)
    }

    /// Writes the document, migrated from format `from` in memory, over the
    /// older text as one of the studio's own writes, logs it as one
    /// `external` `song.version` change and makes it one undo step. When the
    /// write fails, nothing is logged or pushed and the reason is kept.
    fn write_migration(&mut self, from: u32) {
        let text = self.doc.text();
        let hash = match self.write(&text) {
            Ok(hash) => hash,
            Err(e) => {
                self.migration_error = Some(format!(
                    "the song was upgraded to the new file format in memory but not saved: {e}"
                ));
                return;
            }
        };
        let changes: Vec<Change> = version_change(from, FORMAT_VERSION).into_iter().collect();
        if let Err(e) = self.log.append_changes(Source::External, &changes, &hash) {
            // The file is migrated either way; say that the log missed it.
            self.migration_error = Some(format!(
                "the song was upgraded and saved, but the edit log could not be written: {e}"
            ));
        }
        let before = std::mem::replace(&mut self.disk_text, text.clone());
        self.disk_version = FORMAT_VERSION;
        let entry = UndoEntry {
            before,
            after: text,
            source: Source::External,
            changes,
        };
        self.migration = Some((from, entry.clone()));
        self.push_undo(entry);
    }

    /// The format version the song file was migrated from when opened,
    /// while that migration is the newest undo step (and no edit is
    /// pending), so that one undo reverts it; else `None`.
    pub fn migrated_from(&self) -> Option<u32> {
        let (from, entry) = self.migration.as_ref()?;
        (self.pending.is_empty() && self.undo.back() == Some(entry)).then_some(*from)
    }

    /// Why the migrated text could not be written back when the song was
    /// opened (the song is open, migrated in memory), or why the edit log
    /// missed the migration, as a sentence for the producer; `None` when it
    /// went well, there was none, or the studio has written the file since.
    pub fn migration_error(&self) -> Option<&str> {
        self.migration_error.as_deref()
    }

    /// Applies an edit action in memory and returns what it changed. The
    /// file is written later, by [`flush`](Self::flush). An edit that
    /// changes nothing is not recorded. Actions that are not edits
    /// (transport, undo, a live knob drag) give [`StudioError::NotAnEdit`];
    /// while the file on disk does not parse, every edit gives
    /// [`StudioError::FileInvalid`].
    pub fn apply_edit(
        &mut self,
        action: &Action,
        now: Instant,
    ) -> Result<Vec<Change>, StudioError> {
        if !self.file_errors.is_empty() {
            return Err(StudioError::FileInvalid);
        }
        let mut doc = self.doc.clone();
        self.apply(&mut doc, action)?;
        if doc.text() == self.doc.text() {
            return Ok(Vec::new());
        }
        let changes = diff_songs(self.doc.song(), doc.song());
        self.doc = doc;
        self.pending.push(PendingEdit {
            action: action.clone(),
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
        let changes = diff_files(
            self.disk_version,
            &self.disk_song,
            FORMAT_VERSION,
            self.doc.song(),
        );
        let before = std::mem::replace(&mut self.disk_text, text.clone());
        self.disk_version = FORMAT_VERSION;
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
        let version = text_version(&doc);
        let changes = diff_files(self.disk_version, &self.disk_song, version, doc.song());
        if source == Source::External {
            // Best effort: the change is adopted even if the log is not
            // writable, as it is already on disk.
            let _ = self.log_changes(Source::External, &changes, &hash);
        }
        let replaced = is_whole_replace(&self.disk_text, &text);
        let before = std::mem::replace(&mut self.disk_text, text.clone());
        self.disk_version = version;
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
                if self.apply(&mut trial, &edit.action).is_ok() {
                    if trial.text() != next.text() {
                        let changes = diff_songs(next.song(), trial.song());
                        ext.merged.extend(changes.iter().cloned());
                        self.pending.push(PendingEdit {
                            action: edit.action,
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
            if self.apply(&mut trial, &edit.action).is_err() || trial.text() == self.doc.text() {
                continue;
            }
            let changes = diff_songs(self.doc.song(), trial.song());
            self.doc = trial;
            self.pending.push(PendingEdit {
                action: edit.action,
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

    /// Whether `bar` (0-based) lies in a stretch of the lane for this
    /// track's `param`.
    pub fn is_automated(&self, track: &str, param: &str, bar: u32) -> bool {
        self.doc
            .song()
            .automation
            .iter()
            .any(|l| l.track == track && l.param == param && l.span().contains(&bar))
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

    /// Applies `action` to `doc`, a copy of the document (see
    /// [`apply_action`]).
    fn apply(&self, doc: &mut SongDoc, action: &Action) -> Result<(), StudioError> {
        let ctx = ApplyCtx {
            log: Some(&self.log),
        };
        apply_action(doc, action, &ctx).map(drop)
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.last_edit = None;
    }

    /// Writes `text` atomically and remembers its hash as an own write.
    fn write(&mut self, text: &str) -> Result<String, StudioError> {
        write_atomic(&self.path, text)
            .map_err(|e| StudioError::Io(format!("cannot write {}: {e}", self.path.display())))?;
        // The file now holds the newest format, whatever the open-time
        // migration could not do.
        self.migration_error = None;
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
        let version = text_version(&doc);
        let changes = diff_files(self.disk_version, &self.disk_song, version, doc.song());
        self.disk_song = doc.song().clone();
        self.disk_text = text.to_owned();
        self.disk_version = version;
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
        self.log.append_changes(source, changes, hash).map_err(|e| {
            StudioError::Io(format!(
                "the song was written, but the edit log failed: {e}"
            ))
        })
    }
}

/// The format version of the text `doc` was parsed from.
fn text_version(doc: &SongDoc) -> u32 {
    doc.migrated_from().unwrap_or(FORMAT_VERSION)
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
            | SongPath::SectionPlay { track: t, .. },
        ) => t == track,
        _ => false,
    }
}
