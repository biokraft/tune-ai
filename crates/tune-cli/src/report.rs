//! Shared command output: text and JSON results, error reporting, and the
//! small formatters and parsers the commands share.

use std::ops::Range;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use tune_song::{LoadError, SongError};

/// One problem to report. `line` and `column` are 1-based and `None` when the
/// problem has no position.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl Failure {
    /// A problem with no file and no position.
    pub fn msg(message: impl Into<String>) -> Self {
        Self {
            file: None,
            line: None,
            column: None,
            message: message.into(),
        }
    }

    fn at(file: &Path, e: &SongError) -> Self {
        Self {
            file: Some(file.display().to_string()),
            line: e.line,
            column: e.column,
            message: e.message.clone(),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "file": self.file,
            "line": self.line,
            "column": self.column,
            "message": self.message,
        })
    }

    fn to_text(&self) -> String {
        match (&self.file, self.line, self.column) {
            (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}: {}", self.message),
            (Some(f), Some(l), None) => format!("{f}:{l}: {}", self.message),
            _ => format!("error: {}", self.message),
        }
    }
}

/// Failures for a song that did not load.
pub fn load_failures(path: &Path, err: LoadError) -> Vec<Failure> {
    match err {
        LoadError::Invalid(errors) => errors.iter().map(|e| Failure::at(path, e)).collect(),
        LoadError::Io { .. } => vec![Failure::msg(err.to_string())],
    }
}

/// What a command prints on success: the text form and the JSON form (without
/// the `"ok"` key, which `finish` adds first).
pub struct Success {
    pub text: String,
    pub json: Value,
}

/// Prints the outcome and returns the exit code (0 success, 2 failure).
/// JSON goes to stdout. Text errors go to stderr.
pub fn finish(json: bool, result: Result<Success, Vec<Failure>>) -> ExitCode {
    match result {
        Ok(ok) => {
            if json {
                let mut obj = serde_json::Map::new();
                obj.insert("ok".into(), Value::Bool(true));
                if let Value::Object(rest) = ok.json {
                    obj.extend(rest);
                }
                println!("{}", Value::Object(obj));
            } else {
                println!("{}", ok.text);
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            if json {
                let list: Vec<Value> = errors.iter().map(Failure::to_json).collect();
                println!("{}", json!({ "ok": false, "errors": list }));
            } else {
                for e in &errors {
                    eprintln!("{}", e.to_text());
                }
            }
            ExitCode::from(2)
        }
    }
}

/// `m:ss.s`, e.g. `0:08.0`.
pub fn fmt_time(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths % 600 / 10, tenths % 10)
}

/// Peak level in dBFS, floored at -200 so silence stays a finite number.
pub fn dbfs(peak: f64) -> f64 {
    if peak <= 1e-10 {
        -200.0
    } else {
        20.0 * peak.log10()
    }
}

/// Parses a 1-based inclusive `A..B` bar range into a 0-based half-open one.
pub fn parse_bar_range(s: &str, total_bars: u32) -> Result<Range<u32>, String> {
    let bad = || format!("bad --bars `{s}`: expected A..B, e.g. 1..8");
    let (a, b) = s.split_once("..").ok_or_else(bad)?;
    let a: u32 = a.trim().parse().map_err(|_| bad())?;
    let b: u32 = b.trim().parse().map_err(|_| bad())?;
    if a < 1 {
        return Err(format!("bad --bars `{s}`: bars start at 1"));
    }
    if b < a {
        return Err(format!(
            "bad --bars `{s}`: range end is before its start (B < A)"
        ));
    }
    if b > total_bars {
        return Err(format!(
            "bad --bars `{s}`: bar {b} is past the end ({total_bars} bars)"
        ));
    }
    Ok(a - 1..b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_format() {
        assert_eq!(fmt_time(8.0), "0:08.0");
        assert_eq!(fmt_time(65.25), "1:05.3");
        assert_eq!(fmt_time(0.0), "0:00.0");
    }

    #[test]
    fn bar_ranges() {
        assert_eq!(parse_bar_range("1..4", 4), Ok(0..4));
        assert_eq!(parse_bar_range("2..2", 4), Ok(1..2));
        assert!(
            parse_bar_range("0..2", 4)
                .unwrap_err()
                .contains("bars start at 1")
        );
        assert!(parse_bar_range("3..2", 4).unwrap_err().contains("B < A"));
        assert!(
            parse_bar_range("3..9", 4)
                .unwrap_err()
                .contains("past the end (4 bars)")
        );
        assert!(parse_bar_range("3", 4).is_err());
        assert!(parse_bar_range("a..b", 4).is_err());
    }

    #[test]
    fn dbfs_values() {
        assert_eq!(dbfs(1.0), 0.0);
        assert_eq!(dbfs(0.0), -200.0);
    }
}
