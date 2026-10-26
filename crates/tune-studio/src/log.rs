//! The per-song edit log: who changed what, and when.
//!
//! `<song dir>/edits.jsonl` holds one JSON object per line. The studio
//! appends the producer's edits and external changes; `tune set` appends
//! Claude's, tagged with the hash of the file it wrote. Claude reads the
//! log to see what the producer changed.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::diff::Change;

/// Who made an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

    /// Appends one entry per change, all from `source`, stamped now and
    /// carrying `file_hash` (the file after the changes).
    pub(crate) fn append_changes(
        &self,
        source: Source,
        changes: &[Change],
        file_hash: &str,
    ) -> io::Result<()> {
        let ts = Timestamp::now();
        let entries: Vec<EditEntry> = changes
            .iter()
            .map(|c| EditEntry {
                ts,
                source,
                path: c.path.to_string(),
                old: c.old.clone(),
                new: c.new.clone(),
                file_hash: Some(file_hash.to_owned()),
            })
            .collect();
        self.append(&entries)
    }

    /// Appends the entries as lines, in one `O_APPEND` write so concurrent
    /// writers never interleave inside a batch.
    pub fn append(&self, entries: &[EditEntry]) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut buf = Vec::new();
        for e in entries {
            serde_json::to_writer(&mut buf, e).map_err(io::Error::other)?;
            buf.push(b'\n');
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        file.write_all(&buf)
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

/// FNV-1a 64 of the UTF-8 bytes as 16 lowercase hex digits.
pub fn text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
