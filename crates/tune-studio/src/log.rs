//! The per-song edit log: who changed what, and when.
//!
//! `<song dir>/edits.jsonl` holds one JSON object per line. The studio
//! appends the producer's edits and external changes; `tune set` appends
//! Claude's, tagged with the hash of the file it wrote. Claude reads the
//! log to see what the producer changed.
//!
//! Each write appends one [`EditEntry`] per changed path, then one
//! [`ActionRecord`]: the write's undo targets, which a run undo reads.
//! A record has no `path`, so it never reads as an entry, and a log
//! written before records existed reads as it did.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::action::RunTag;
use crate::diff::Change;
use crate::target::TargetChange;

/// Who made an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(ts_rs::TS)]
pub enum Source {
    Human,
    Claude,
    External,
}

/// One logged change to one path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditEntry {
    /// RFC 3339 UTC with exactly three fraction digits.
    #[serde(serialize_with = "ser_ts", deserialize_with = "de_ts")]
    pub ts: Timestamp,
    pub source: Source,
    pub path: String,
    pub old: Value,
    pub new: Value,
    /// FNV-1a hash of the file text after the edit (see [`text_hash`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    /// The name of the action that made the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The run the change belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_title: Option<String>,
}

/// One write of the song: who made it, the actions that made it, the run
/// it belongs to and what it changed, as undo targets. Serialized with
/// `"record": "action"` after `source`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "RecordLine", from = "RecordLine")]
pub struct ActionRecord {
    pub ts: Timestamp,
    pub source: Source,
    /// The names of the actions, each once, in the order they first ran:
    /// `undo`, `redo` and `migrate` for those writes, none for an external
    /// change.
    pub actions: Vec<String>,
    pub run: Option<String>,
    pub run_title: Option<String>,
    /// `target_changes(song before, song after)`.
    pub targets: Vec<TargetChange>,
    /// FNV-1a hash of the file text after the write (see [`text_hash`]).
    pub file_hash: Option<String>,
}

impl ActionRecord {
    /// The record of a write stamped `ts` that left the file with hash
    /// `file_hash`, its action names kept once each, in order.
    pub(crate) fn new<'a>(
        ts: Timestamp,
        source: Source,
        actions: impl IntoIterator<Item = &'a str>,
        run: Option<&RunTag>,
        targets: Vec<TargetChange>,
        file_hash: &str,
    ) -> Self {
        let mut names: Vec<String> = Vec::new();
        for name in actions {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
        Self {
            ts,
            source,
            actions: names,
            run: run.map(|r| r.id.clone()),
            run_title: run.and_then(|r| r.title.clone()),
            targets,
            file_hash: Some(file_hash.to_owned()),
        }
    }
}

/// An [`ActionRecord`] as its line holds it.
#[derive(Clone, Serialize, Deserialize)]
struct RecordLine {
    #[serde(serialize_with = "ser_ts", deserialize_with = "de_ts")]
    ts: Timestamp,
    source: Source,
    record: RecordKind,
    actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_title: Option<String>,
    targets: Vec<TargetChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_hash: Option<String>,
}

/// The `record` field: what kind of record a line holds.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecordKind {
    Action,
}

impl From<ActionRecord> for RecordLine {
    fn from(r: ActionRecord) -> Self {
        Self {
            ts: r.ts,
            source: r.source,
            record: RecordKind::Action,
            actions: r.actions,
            run: r.run,
            run_title: r.run_title,
            targets: r.targets,
            file_hash: r.file_hash,
        }
    }
}

impl From<RecordLine> for ActionRecord {
    fn from(l: RecordLine) -> Self {
        let RecordKind::Action = l.record;
        Self {
            ts: l.ts,
            source: l.source,
            actions: l.actions,
            run: l.run,
            run_title: l.run_title,
            targets: l.targets,
            file_hash: l.file_hash,
        }
    }
}

fn ser_ts<S: Serializer>(ts: &Timestamp, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&ts.strftime("%Y-%m-%dT%H:%M:%S%.3fZ"))
}

fn de_ts<'de, D: Deserializer<'de>>(d: D) -> Result<Timestamp, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

/// How many trailing lines [`EditLog::has_claude_hash`] looks at.
const HASH_WINDOW: usize = 256;

/// The edit log of one song.
#[derive(Clone, Debug)]
pub struct EditLog {
    file: PathBuf,
}

impl EditLog {
    /// The log next to `song_path`: `<dir>/edits.jsonl`.
    pub fn for_song(song_path: &Path) -> Self {
        let dir = song_path.parent().unwrap_or_else(|| Path::new(""));
        Self {
            file: dir.join("edits.jsonl"),
        }
    }

    /// Appends the entries as lines, in one `O_APPEND` write so concurrent
    /// writers never interleave inside a batch.
    pub fn append(&self, entries: &[EditEntry]) -> io::Result<()> {
        self.append_write(entries, None)
    }

    /// The entries, then the record, in one `O_APPEND` write.
    pub fn append_write(
        &self,
        entries: &[EditEntry],
        record: Option<&ActionRecord>,
    ) -> io::Result<()> {
        if entries.is_empty() && record.is_none() {
            return Ok(());
        }
        let mut buf = Vec::new();
        for e in entries {
            serde_json::to_writer(&mut buf, e).map_err(io::Error::other)?;
            buf.push(b'\n');
        }
        if let Some(record) = record {
            serde_json::to_writer(&mut buf, record).map_err(io::Error::other)?;
            buf.push(b'\n');
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        file.write_all(&buf)
    }

    /// [`append_write`](Self::append_write) for one write of the song: its
    /// record is left out, and so nothing is written, when the write logs
    /// no entry and changes no target.
    pub(crate) fn append_logged(
        &self,
        entries: &[EditEntry],
        record: &ActionRecord,
    ) -> io::Result<()> {
        let changed = !entries.is_empty() || !record.targets.is_empty();
        self.append_write(entries, changed.then_some(record))
    }

    /// Every action record, in file order; lines that are not records are
    /// skipped.
    pub fn read_records(&self) -> io::Result<Vec<ActionRecord>> {
        Ok(self
            .read_lines()?
            .iter()
            .filter_map(|l| serde_json::from_str::<ActionRecord>(l).ok())
            .collect())
    }

    /// Entries with `ts > since` (all when `None`), oldest first. Lines that
    /// do not parse are skipped; a missing log reads as empty.
    pub fn read_since(&self, since: Option<Timestamp>) -> io::Result<Vec<EditEntry>> {
        Ok(self
            .read_lines()?
            .iter()
            .filter_map(|l| serde_json::from_str::<EditEntry>(l).ok())
            .filter(|e| since.is_none_or(|t| e.ts > t))
            .collect())
    }

    /// Whether one of the last 256 lines is a `claude` entry with this file
    /// hash.
    pub fn has_claude_hash(&self, hash: &str) -> io::Result<bool> {
        let lines = self.read_lines()?;
        let tail = &lines[lines.len().saturating_sub(HASH_WINDOW)..];
        Ok(tail
            .iter()
            .filter_map(|l| serde_json::from_str::<EditEntry>(l).ok())
            .any(|e| e.source == Source::Claude && e.file_hash.as_deref() == Some(hash)))
    }

    fn read_lines(&self) -> io::Result<Vec<String>> {
        match fs::read(&self.file) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_owned)
                .collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

/// One entry per change, all stamped `ts`.
pub(crate) fn entries_for(
    ts: Timestamp,
    source: Source,
    changes: &[Change],
    file_hash: &str,
    action: Option<&str>,
    run: Option<&RunTag>,
) -> Vec<EditEntry> {
    changes
        .iter()
        .map(|c| EditEntry {
            ts,
            source,
            path: c.path.to_string(),
            old: c.old.clone(),
            new: c.new.clone(),
            file_hash: Some(file_hash.to_owned()),
            action: action.map(str::to_owned),
            run: run.map(|r| r.id.clone()),
            run_title: run.and_then(|r| r.title.clone()),
        })
        .collect()
}

/// FNV-1a 64 of the UTF-8 bytes as 16 lowercase hex digits.
pub fn text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
