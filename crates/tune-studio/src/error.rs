//! Errors the studio reports, in a form that serializes for the app.

use serde::Serialize;
use tune_song::SongError;

use crate::doc::EditError;

/// A problem in the song file, with its position when it has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct ErrorView {
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl From<&SongError> for ErrorView {
    fn from(e: &SongError) -> Self {
        Self {
            line: e.line,
            column: e.column,
            message: e.message.clone(),
        }
    }
}

/// Why a studio operation failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StudioError {
    #[error("{0}")]
    Io(String),
    #[error("{}", views_summary(.0))]
    Load(Vec<ErrorView>),
    #[error("{0}")]
    Edit(#[from] EditError),
    #[error("the song file has errors; fix it first")]
    FileInvalid,
    #[error("no song is open")]
    NoSong,
    #[error("that command is not an edit")]
    NotAnEdit,
    #[error("`{action}` needs the studio: open the song with `tune serve <song>` or the app first")]
    NeedsStudio { action: String },
    #[error("a batch holds song edits only; run `{action}` on its own")]
    EditsOnly { action: String },
    #[error("no edits of run `{0}` in the edit log")]
    NoSuchRun(String),
    #[error("the studio did not answer; check it before retrying")]
    NoAnswer,
    #[error("the studio has another song open now; nothing was changed")]
    SongChanged,
    #[error("{0}")]
    Audio(String),
    #[error("timed out")]
    Timeout,
    #[error("the studio has shut down")]
    Closed,
    #[error("`{0}` is not under the songs root")]
    OutsideRoot(String),
}

fn views_summary(views: &[ErrorView]) -> String {
    views
        .iter()
        .map(|v| match (v.line, v.column) {
            (Some(l), Some(c)) => format!("{l}:{c}: {}", v.message),
            _ => v.message.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
