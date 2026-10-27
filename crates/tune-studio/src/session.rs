//! The live session actions (`launch_clip`, `launch_scene`, `stop_clip`,
//! `back_to_arrangement`): their check against the open song, the engine
//! cue each one becomes, and the tracks the studio has taken off the
//! arrangement.
//!
//! A session action changes no file: it is checked against the open song
//! when it arrives, and resolved to indices only when the studio sends it,
//! against the song the engine then plays (see the studio's `send_cues`).

use std::collections::BTreeSet;

use tune_engine::{CompiledSong, SessionCue};
use tune_song::{EditOpError, Song};

use crate::action::Action;

/// Checks a session action against the open song: its track and scene exist, and a launched slot holds a clip.
pub(crate) fn check(song: &Song, action: &Action) -> Result<(), EditOpError> {
    let track = |id: &str| {
        song.tracks
            .contains_key(id)
            .then_some(())
            .ok_or_else(|| EditOpError::NoSuchTrack(id.to_owned()))
    };
    let scene = |id: &str| {
        song.scenes
            .get(id)
            .ok_or_else(|| EditOpError::NoSuchScene(id.to_owned()))
    };
    match action {
        Action::LaunchClip { track: t, scene: s } => {
            let slots = &scene(s)?.slots;
            track(t)?;
            if !slots.contains_key(t) {
                return Err(empty_slot(s, t));
            }
        }
        Action::LaunchScene { scene: s } => {
            scene(s)?;
        }
        Action::StopClip { track: t } | Action::BackToArrangement { track: Some(t) } => track(t)?,
        _ => {}
    }
    Ok(())
}

/// The engine cue for a checked session action, resolved against the song the engine plays.
pub(crate) fn resolve(song: &CompiledSong, action: &Action) -> Option<SessionCue> {
    lookup(song, action).ok()
}

/// Why a checked session action no longer resolves against `song`, as the
/// studio reports dropping it: "dropped `launch_clip`: there is no scene
/// `groove` any more".
pub(crate) fn dropped(song: &CompiledSong, action: &Action) -> String {
    match lookup(song, action) {
        Err(why) => format!("dropped `{}`: {why} any more", action.name()),
        Ok(_) => format!("dropped `{}`", action.name()),
    }
}

/// [`resolve`], with the reason when it does not.
fn lookup(song: &CompiledSong, action: &Action) -> Result<SessionCue, EditOpError> {
    let track = |id: &str| {
        song.track_index(id)
            .ok_or_else(|| EditOpError::NoSuchTrack(id.to_owned()))
    };
    let scene = |id: &str| {
        song.scene_index(id)
            .ok_or_else(|| EditOpError::NoSuchScene(id.to_owned()))
    };
    Ok(match action {
        Action::LaunchClip { track: t, scene: s } => {
            let scene_index = scene(s)?;
            let track = track(t)?;
            let clip = song.scenes[scene_index]
                .slots
                .get(track)
                .copied()
                .flatten()
                .ok_or_else(|| empty_slot(s, t))?;
            SessionCue::Launch { track, clip }
        }
        Action::LaunchScene { scene: s } => SessionCue::Scene(scene(s)?),
        Action::StopClip { track: t } => SessionCue::Stop(track(t)?),
        Action::BackToArrangement { track: t } => {
            SessionCue::Back(t.as_deref().map(track).transpose()?)
        }
        other => unreachable!("`{}` is not a session action", other.name()),
    })
}

fn empty_slot(scene: &str, track: &str) -> EditOpError {
    EditOpError::EmptySlot {
        scene: scene.to_owned(),
        track: track.to_owned(),
    }
}

/// The tracks the studio has taken off the arrangement this playback.
#[derive(Debug, Default)]
pub(crate) struct OffArrangement(BTreeSet<String>);

impl OffArrangement {
    /// Notes what a session action sent to the engine does to the tracks of
    /// `song`, the song it was resolved against: `launch_clip` and
    /// `stop_clip` take their track off the arrangement, `launch_scene`
    /// every track, `back_to_arrangement` sends one or all back.
    pub(crate) fn apply(&mut self, action: &Action, song: &CompiledSong) {
        match action {
            Action::LaunchClip { track, .. } | Action::StopClip { track } => {
                self.0.insert(track.clone());
            }
            Action::LaunchScene { .. } => self.0.extend(song.tracks.iter().map(|t| t.id.clone())),
            Action::BackToArrangement { track: Some(track) } => {
                self.0.remove(track);
            }
            Action::BackToArrangement { track: None } => self.clear(),
            _ => {}
        }
    }

    pub(crate) fn contains(&self, track: &str) -> bool {
        self.0.contains(track)
    }

    /// Drops ids `song`, the song now playing, no longer has: a renamed or
    /// new track follows the arrangement.
    pub(crate) fn retain(&mut self, song: &CompiledSong) {
        self.0.retain(|t| song.track_index(t).is_some());
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use tune_song::parse_song;

    use super::*;

    const SESSION: &str = include_str!("../../tune-song/tests/fixtures/session.toml");

    fn song() -> Song {
        parse_song(SESSION).expect("the session fixture parses")
    }

    fn launch_clip(track: &str, scene: &str) -> Action {
        Action::LaunchClip {
            track: track.into(),
            scene: scene.into(),
        }
    }

    fn back(track: Option<&str>) -> Action {
        Action::BackToArrangement {
            track: track.map(Into::into),
        }
    }

    #[test]
    fn resolves_ids_against_the_song_the_engine_plays() {
        let song = song();
        let compiled = CompiledSong::new(&song, 48_000);
        let acid = compiled.track_index("acid").unwrap();
        let a1 = compiled.clip_index("a1").unwrap();
        assert_eq!(
            resolve(&compiled, &launch_clip("acid", "groove")),
            Some(SessionCue::Launch {
                track: acid,
                clip: a1
            })
        );
        let groove = Action::LaunchScene {
            scene: "groove".into(),
        };
        assert_eq!(
            resolve(&compiled, &groove),
            Some(SessionCue::Scene(compiled.scene_index("groove").unwrap()))
        );
        assert_eq!(
            resolve(
                &compiled,
                &Action::StopClip {
                    track: "hat".into()
                }
            ),
            Some(SessionCue::Stop(compiled.track_index("hat").unwrap()))
        );
        assert_eq!(
            resolve(&compiled, &back(Some("acid"))),
            Some(SessionCue::Back(Some(acid)))
        );
        assert_eq!(
            resolve(&compiled, &back(None)),
            Some(SessionCue::Back(None))
        );
    }

    #[test]
    fn an_action_that_no_longer_resolves_says_why() {
        let mut song = song();
        song.scenes.shift_remove("groove");
        let compiled = CompiledSong::new(&song, 48_000);
        let launch = launch_clip("acid", "groove");
        assert_eq!(resolve(&compiled, &launch), None);
        assert_eq!(
            dropped(&compiled, &launch),
            "dropped `launch_clip`: there is no scene `groove` any more"
        );
        assert_eq!(
            dropped(&compiled, &launch_clip("acid", "break")),
            "dropped `launch_clip`: scene `break` has no clip for track `acid` any more"
        );
        assert_eq!(
            dropped(&compiled, &back(Some("bass"))),
            "dropped `back_to_arrangement`: there is no track `bass` any more"
        );
    }

    #[test]
    fn checks_the_track_the_scene_and_the_slot() {
        let song = song();
        assert_eq!(check(&song, &launch_clip("acid", "groove")), Ok(()));
        assert_eq!(
            check(&song, &launch_clip("acid", "drop")),
            Err(EditOpError::NoSuchScene("drop".into()))
        );
        assert_eq!(
            check(&song, &launch_clip("bass", "groove")),
            Err(EditOpError::NoSuchTrack("bass".into()))
        );
        assert_eq!(
            check(&song, &launch_clip("acid", "break")),
            Err(empty_slot("break", "acid"))
        );
        assert_eq!(check(&song, &back(None)), Ok(()));
        assert_eq!(
            check(&song, &back(Some("bass"))),
            Err(EditOpError::NoSuchTrack("bass".into()))
        );
    }

    #[test]
    fn off_arrangement_follows_the_actions() {
        let song = CompiledSong::new(&song(), 48_000);
        let mut off = OffArrangement::default();
        off.apply(&launch_clip("acid", "groove"), &song);
        off.apply(
            &Action::StopClip {
                track: "hat".into(),
            },
            &song,
        );
        assert!(off.contains("acid") && off.contains("hat") && !off.contains("kick"));
        off.apply(&back(Some("acid")), &song);
        assert!(!off.contains("acid") && off.contains("hat"));
        off.apply(&back(None), &song);
        assert!(!off.contains("hat"));
        off.apply(
            &Action::LaunchScene {
                scene: "break".into(),
            },
            &song,
        );
        assert!(["kick", "acid", "hat"].iter().all(|t| off.contains(t)));
        let mut fewer = self::song();
        fewer.tracks.shift_remove("kick");
        off.retain(&CompiledSong::new(&fewer, 48_000));
        assert!(!off.contains("kick") && off.contains("acid"));
        off.clear();
        assert!(!off.contains("acid"));
    }
}
