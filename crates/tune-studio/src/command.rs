//! The studio's commands: what the app (or a browser tab) asks it to do.
//!
//! One serde form serves every transport: `{"cmd": "<snake_case name>",
//! "args": {…}}`, with `args` left out for commands that take none. A
//! command is either an [`Action`] (an edit, a transport control, undo or
//! redo), which `tune do` can run too, or a [`StudioCommand`], which only a
//! studio client sends.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::action::Action;

/// One request to the studio.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Command {
    Studio(StudioCommand),
    Action(Action),
}

impl From<Action> for Command {
    fn from(a: Action) -> Self {
        Self::Action(a)
    }
}

impl From<StudioCommand> for Command {
    fn from(c: StudioCommand) -> Self {
        Self::Studio(c)
    }
}

/// A `cmd` that names a [`StudioCommand`] is one; any other is an
/// [`Action`], whose error names an unknown `cmd`.
impl<'de> Deserialize<'de> for Command {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Value::deserialize(d)?;
        let studio = wire
            .get("cmd")
            .and_then(Value::as_str)
            .is_some_and(|c| STUDIO_NAMES.contains(&c));
        if studio {
            StudioCommand::deserialize(wire)
                .map(Self::Studio)
                .map_err(D::Error::custom)
        } else {
            Action::deserialize(wire)
                .map(Self::Action)
                .map_err(D::Error::custom)
        }
    }
}

/// A request only a studio client makes: the song choice, the state, and
/// the answer to the conflict prompt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
#[derive(ts_rs::TS)]
pub enum StudioCommand {
    /// The songs under the studio's songs root.
    ListSongs,
    /// The current [`StudioState`](crate::StudioState), as the call's
    /// result. Changes nothing; a client calls it right after subscribing,
    /// in case it missed the first `state` event. For the same reason, a
    /// migration the studio could not save is reported again, as an `error`
    /// event.
    GetState,
    OpenSong {
        path: String,
    },
    /// Re-applies (`true`) or drops the local edits an external change
    /// overrode.
    ResolveOffered {
        reapply: bool,
    },
}

/// Every [`StudioCommand`]'s wire name.
const STUDIO_NAMES: [&str; 4] = ["list_songs", "get_state", "open_song", "resolve_offered"];
