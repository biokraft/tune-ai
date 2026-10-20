//! Song errors and the byte offset to line:column mapping.

use std::fmt;
use std::ops::Range;
use std::path::PathBuf;

/// One problem in a song file. `line` and `column` are 1-based and the column
/// counts chars, not bytes. Both are `None` for problems that belong to the
/// whole file (e.g. a song with no sections).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongError {
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl fmt::Display for SongError {
    /// `line:column: message`, or just the message without a position.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, "{line}:{column}: {}", self.message),
            (Some(line), None) => write!(f, "{line}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for SongError {}

/// Why [`load_song`](crate::load_song) failed.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}", summary(.0))]
    Invalid(Vec<SongError>),
}

fn summary(errors: &[SongError]) -> String {
    errors
        .iter()
        .map(SongError::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 1-based line and char column of byte `offset` in `src`. An offset past
/// the end (or inside a multi-byte char) is clamped to the nearest char
/// boundary at or before it.
pub(crate) fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(src.len());
    while !src.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &src[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count() + 1;
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}

/// Collects every problem found while reading a song, each with the byte
/// span it points at.
#[derive(Default)]
pub(crate) struct Errors {
    items: Vec<(Option<usize>, String)>,
}

impl Errors {
    /// A problem at the start of `span`.
    pub(crate) fn at(&mut self, span: &Range<usize>, message: impl Into<String>) {
        self.items.push((Some(span.start), message.into()));
    }

    /// A problem with no position (it belongs to the whole file).
    pub(crate) fn global(&mut self, message: impl Into<String>) {
        self.items.push((None, message.into()));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many problems are recorded so far (to tell whether a check added one).
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// The errors ordered by position (stable, so problems at the same spot
    /// keep the order they were found in); positionless errors come last.
    pub(crate) fn finish(mut self, src: &str) -> Vec<SongError> {
        self.items
            .sort_by_key(|(offset, _)| offset.map_or((1, 0), |o| (0, o)));
        self.items
            .into_iter()
            .map(|(offset, message)| {
                let (line, column) = match offset {
                    Some(o) => {
                        let (l, c) = line_col(src, o);
                        (Some(l), Some(c))
                    }
                    None => (None, None),
                };
                SongError {
                    line,
                    column,
                    message,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_chars() {
        let src = "ab\näö x\n";
        assert_eq!(line_col(src, 0), (1, 1));
        assert_eq!(line_col(src, 2), (1, 3));
        assert_eq!(line_col(src, 3), (2, 1));
        // `x` sits after two 2-byte chars and a space.
        assert_eq!(line_col(src, src.find('x').unwrap()), (2, 4));
        assert_eq!(line_col(src, src.len()), (3, 1));
        assert_eq!(line_col(src, 999), (3, 1));
        assert_eq!(line_col(src, 4), (2, 1)); // inside `ä`
    }

    #[test]
    fn display_with_and_without_position() {
        let e = SongError {
            line: Some(3),
            column: Some(7),
            message: "bad".into(),
        };
        assert_eq!(e.to_string(), "3:7: bad");
        let e = SongError {
            line: None,
            column: None,
            message: "bad".into(),
        };
        assert_eq!(e.to_string(), "bad");
    }
}
