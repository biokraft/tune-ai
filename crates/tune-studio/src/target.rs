//! Undo targets: a song seen as the values and items one edit can change.
//!
//! A [`Target`] is one leaf value (any `tune set` path, a scene's slot
//! among them), one whole item (a track, a track's instrument or fx chain,
//! a clip, a scene), one list merged an element at a time (a track's
//! placements, the stretches of one lane param), the section list or the
//! scene order. [`target_changes`] gives the targets two songs differ in,
//! with each one's value on both sides (see [`target_value`]); the edit log
//! records them per write, so a run can be undone target by target.
//! [`SongDoc::put`] writes a value back.
//!
//! Values are effective (defaults filled in), so writing one back can turn
//! an implicit default into an explicit one. A removed item comes back in
//! the minimal layout, without the comments it had: a restored track, clip
//! or scene table, and a restored lane stretch (an `[[automation]]` table
//! gets no comment back, as the stretch's value holds none).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};
use tune_dsp::STRIP_PARAMS;
use tune_song::{
    ClipKind, Device, Lane, MIX_TRACK_ID, MixUnit, Placement, Registry, Scene, Section, Song, Track,
};

use crate::diff::scenes_reordered;
use crate::doc::{EditError, SongDoc, effective_value, num, placement_json};
use crate::path::{SongPath, TrackField};

/// One thing an edit can change, as a run undo reverts it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A leaf: `song.bpm`, `tracks.<t>.<field>`,
    /// `tracks.<t>.<device>.<param|setting>`, `tracks.<t>.send.delay` …,
    /// `mix.master`, `mix.duck_source`, `mix.<param>`,
    /// `clips.<c>.steps|notes|bars`, `scenes.<s>.slots.<t>`.
    Value(SongPath),
    /// `tracks.<t>`: the whole track.
    Track(String),
    /// `tracks.<t>.instrument`
    Instrument(String),
    /// `tracks.<t>.fx`: the effect chain.
    Fx(String),
    /// `clips.<c>`: the whole clip.
    Clip(String),
    /// `tracks.<t>.play`: the track's placements.
    Play(String),
    /// `automation.<t>.<param id>`: every stretch of one lane param, e.g.
    /// `automation.acid.acid303.cutoff`, `automation.mix.reverb.size`.
    Lanes { track: String, param: String },
    /// `sections`: the section list.
    Sections,
    /// `scenes.<s>`: the whole scene.
    Scene(String),
    /// `scenes`: the scene order.
    SceneOrder,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(path) => path.fmt(f),
            Self::Track(t) => write!(f, "tracks.{t}"),
            Self::Instrument(t) => write!(f, "tracks.{t}.instrument"),
            Self::Fx(t) => write!(f, "tracks.{t}.fx"),
            Self::Clip(c) => write!(f, "clips.{c}"),
            Self::Play(t) => write!(f, "tracks.{t}.play"),
            Self::Lanes { track, param } => write!(f, "automation.{track}.{param}"),
            Self::Sections => f.write_str("sections"),
            Self::Scene(s) => write!(f, "scenes.{s}"),
            Self::SceneOrder => f.write_str("scenes"),
        }
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        parse(s).ok_or_else(|| {
            format!(
                "unknown target `{s}` (expected a value path, tracks.<track>, \
                 tracks.<track>.instrument|fx|play, clips.<clip>, scenes, \
                 scenes.<scene>, automation.<track>.<param id> or sections)"
            )
        })
    }
}

fn parse(s: &str) -> Option<Target> {
    if s == "sections" {
        return Some(Target::Sections);
    }
    if let Some(rest) = s.strip_prefix("automation.") {
        let (track, param) = rest.split_once('.')?;
        // The lane's param is a mix param id or a track param id.
        let path = if track == MIX_TRACK_ID {
            format!("mix.{param}")
        } else {
            format!("tracks.{track}.{param}")
        };
        return match path.parse().ok()? {
            SongPath::MixParam { .. } | SongPath::TrackParam { .. } => Some(Target::Lanes {
                track: track.to_owned(),
                param: param.to_owned(),
            }),
            _ => None,
        };
    }
    if let Some(clip) = s.strip_prefix("clips.")
        && !clip.contains('.')
    {
        return match format!("clips.{clip}.steps").parse().ok()? {
            SongPath::Clip { clip } => Some(Target::Clip(clip)),
            _ => None,
        };
    }
    if let Some(rest) = s.strip_prefix("tracks.") {
        let (track, item) = rest.split_once('.').unwrap_or((rest, ""));
        let track_id = match format!("tracks.{track}").parse() {
            Ok(SongPath::Track { track }) => Some(track),
            _ => None,
        };
        if let Some(t) = track_id {
            match item {
                "" => return Some(Target::Track(t)),
                "instrument" => return Some(Target::Instrument(t)),
                "fx" => return Some(Target::Fx(t)),
                "play" => return Some(Target::Play(t)),
                _ => {}
            }
        }
    }
    match s.parse().ok()? {
        path @ (SongPath::Bpm
        | SongPath::TrackField { .. }
        | SongPath::TrackParam { .. }
        | SongPath::MixParam { .. }
        | SongPath::MixMaster
        | SongPath::MixDuckSource
        | SongPath::Clip { .. }
        | SongPath::ClipNotes { .. }
        | SongPath::ClipBars { .. }
        | SongPath::SceneSlot { .. }) => Some(Target::Value(path)),
        SongPath::Scenes => Some(Target::SceneOrder),
        SongPath::Scene { scene } => Some(Target::Scene(scene)),
        _ => None,
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// One target two songs differ in, with its value in each (`null` where it
/// is absent; see [`target_value`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetChange {
    pub target: Target,
    pub old: Value,
    pub new: Value,
}

/// The value of `target` in `song`, as JSON; `null` when it is absent.
/// Every param and setting is effective (defaults filled in):
///
/// - a device: `{"type", "id", "settings": {<name>: <value>}, "params":
///   {<local name>: <n>}}`,
/// - `tracks.<t>`: `{"index", "instrument", "fx": [device], "strip":
///   {<strip param id>: <n>}, "gain_db", "pan", "mute", "solo", "play"}`,
///   `index` its position among the tracks,
/// - `clips.<c>`: `{"kind", "steps"}`, or `{"kind": "notes", "bars",
///   "notes"}` with the notes as `clips.<c>.notes` gives them,
/// - `tracks.<t>.play`: `[placement]` as [`placement_json`] gives them,
/// - `automation.<t>.<p>`: `[{"at", "bars", "points"}]` in bar order (`at`
///   1-based); `[]` when the param has no stretch,
/// - `sections`: `[{"name", "bars"}]`,
/// - `scenes.<s>`: `{"index", "slots": {<track>: <clip>}}`, `index` its
///   position among the scenes (left out when scenes are compared),
/// - `scenes`: `[<scene id>]` in row order,
/// - a leaf: [`effective_value`].
pub fn target_value(song: &Song, target: &Target) -> Value {
    let track = |t: &str| song.tracks.get(t);
    match target {
        Target::Value(path) => effective_value(song, path).unwrap_or(Value::Null),
        Target::Track(t) => track_json(song, t),
        Target::Instrument(t) => track(t).map_or(Value::Null, |t| device_json(&t.instrument)),
        Target::Fx(t) => track(t).map_or(Value::Null, |t| fx_json(&t.fx)),
        Target::Clip(c) => clip_json(song, c),
        Target::Play(t) => track(t).map_or(Value::Null, |t| play_json(&t.play)),
        Target::Lanes { track, param } => lanes_json(song, track, param),
        Target::Sections => sections_json(&song.sections),
        Target::Scene(s) => scene_json(song, s),
        Target::SceneOrder => scene_order_json(song),
    }
}

/// The targets `old` and `new` differ in, in song-file order: `song.bpm`
/// and the mix leaves; per track (in both songs' order), `tracks.<t>` for a
/// track in one song only, else its field and strip leaves,
/// `tracks.<t>.instrument` when the instrument's module or id changed (else
/// its param and setting leaves), `tracks.<t>.fx` when the chain's ids or
/// modules changed, and the param and setting leaves of each effect in
/// both chains; `clips.<c>` for a clip in one song only or of another kind
/// (else its leaves); the scenes as [`diff_songs`](crate::diff_songs) gives
/// them: `scenes.<s>` for a scene in one song only, `scenes` when the
/// scenes in both are in another relative order, and per scene in both one
/// `scenes.<s>.slots.<t>` per track whose clip differs (old's slot order,
/// then new's); `sections`; then the arrangement on top of the sections:
/// `tracks.<t>.play` per track in both songs whose placements
/// changed, and one `automation.<t>.<p>` per lane param whose stretches
/// changed (compared in bar order, so a reorder in the file is no change).
pub fn target_changes(old: &Song, new: &Song) -> Vec<TargetChange> {
    let change = |target: Target, old: Value, new: Value| TargetChange { target, old, new };
    let leaf = |path: SongPath| {
        let a = effective_value(old, &path).unwrap_or(Value::Null);
        let b = effective_value(new, &path).unwrap_or(Value::Null);
        change(Target::Value(path), a, b)
    };
    let mut out = Vec::new();

    let mix_params = MixUnit::ALL.into_iter().flat_map(|unit| {
        unit.params().iter().map(|spec| SongPath::MixParam {
            param: spec.id.to_owned(),
        })
    });
    let song_leaves = [SongPath::Bpm, SongPath::MixMaster, SongPath::MixDuckSource];
    out.extend(song_leaves.into_iter().chain(mix_params).map(leaf));

    let tracks = union(old.tracks.keys(), new.tracks.keys());
    for id in &tracks {
        let (Some(o), Some(n)) = (old.tracks.get(id), new.tracks.get(id)) else {
            let (a, b) = (track_json(old, id), track_json(new, id));
            out.push(change(Target::Track(id.clone()), a, b));
            continue;
        };
        let param = |param: String| {
            leaf(SongPath::TrackParam {
                track: id.clone(),
                param,
            })
        };
        out.extend(TrackField::ALL.into_iter().map(|field| {
            leaf(SongPath::TrackField {
                track: id.clone(),
                field,
            })
        }));
        out.extend(STRIP_PARAMS.iter().map(|spec| param(spec.id.to_owned())));
        if device_key(&o.instrument) == device_key(&n.instrument) {
            out.extend(device_leaves(&o.instrument).into_iter().map(param));
        } else {
            let (a, b) = (device_json(&o.instrument), device_json(&n.instrument));
            out.push(change(Target::Instrument(id.clone()), a, b));
        }
        let old_keys = chain_keys(&o.fx);
        if old_keys != chain_keys(&n.fx) {
            out.push(change(
                Target::Fx(id.clone()),
                fx_json(&o.fx),
                fx_json(&n.fx),
            ));
        }
        for d in n.fx.iter().filter(|d| old_keys.contains(&device_key(d))) {
            out.extend(device_leaves(d).into_iter().map(param));
        }
    }

    for id in union(old.clips.keys(), new.clips.keys()) {
        match (old.clips.get(&id), new.clips.get(&id)) {
            (Some(a), Some(b)) if a.kind() == b.kind() => out.extend(
                [
                    SongPath::Clip { clip: id.clone() },
                    SongPath::ClipBars { clip: id.clone() },
                    SongPath::ClipNotes { clip: id.clone() },
                ]
                .map(leaf),
            ),
            _ => {
                let (a, b) = (clip_json(old, &id), clip_json(new, &id));
                out.push(change(Target::Clip(id), a, b));
            }
        }
    }

    for id in union(old.scenes.keys(), new.scenes.keys()) {
        if old.scenes.contains_key(&id) != new.scenes.contains_key(&id) {
            let (a, b) = (scene_json(old, &id), scene_json(new, &id));
            out.push(change(Target::Scene(id), a, b));
        }
    }
    if scenes_reordered(old, new) {
        let (a, b) = (scene_order_json(old), scene_order_json(new));
        out.push(change(Target::SceneOrder, a, b));
    }
    for (id, o) in &old.scenes {
        if let Some(n) = new.scenes.get(id) {
            out.extend(
                union(o.slots.keys(), n.slots.keys())
                    .into_iter()
                    .map(|track| {
                        leaf(SongPath::SceneSlot {
                            scene: id.clone(),
                            track,
                        })
                    }),
            );
        }
    }

    let (a, b) = (sections_json(&old.sections), sections_json(&new.sections));
    out.push(change(Target::Sections, a, b));

    for id in &tracks {
        if let (Some(o), Some(n)) = (old.tracks.get(id), new.tracks.get(id)) {
            out.push(change(
                Target::Play(id.clone()),
                play_json(&o.play),
                play_json(&n.play),
            ));
        }
    }

    let mut params: Vec<(&str, &str)> = Vec::new();
    for l in old.automation.iter().chain(&new.automation) {
        let key = (l.track.as_str(), l.param.as_str());
        if !params.contains(&key) {
            params.push(key);
        }
    }
    for (track, param) in params {
        let (a, b) = (lanes_json(old, track, param), lanes_json(new, track, param));
        let target = Target::Lanes {
            track: track.to_owned(),
            param: param.to_owned(),
        };
        out.push(change(target, a, b));
    }
    out.retain(|c| c.old != c.new);
    out
}

/// Names in `a` order, then those only in `b`.
fn union<'a>(
    a: impl Iterator<Item = &'a String>,
    b: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let mut out: Vec<String> = a.cloned().collect();
    for k in b {
        if !out.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

/// What makes a device the same one: its id and its module.
fn device_key(d: &Device) -> (&str, &str) {
    (d.id.as_str(), d.module.name)
}

/// A chain as the devices it holds, in order.
fn chain_keys(fx: &[Device]) -> Vec<(&str, &str)> {
    fx.iter().map(device_key).collect()
}

/// The track-relative ids of a device's params, then of its settings.
fn device_leaves(d: &Device) -> Vec<String> {
    let module = d.module;
    let params = (0..module.params.len()).map(|i| d.param_id(module.local_name(i)));
    let settings = module.settings.iter().map(|s| d.param_id(s.name));
    params.chain(settings).collect()
}

fn device_json(d: &Device) -> Value {
    let module = d.module;
    let settings: Map<String, Value> = module
        .settings
        .iter()
        .zip(&d.settings)
        .map(|(spec, value)| (spec.name.to_owned(), Value::from(*value)))
        .collect();
    let params: Map<String, Value> = module
        .params
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let local = module.local_name(i);
            let value = d.params.get(local).copied().unwrap_or(spec.default);
            (local.to_owned(), num(value))
        })
        .collect();
    json!({ "type": module.name, "id": d.id, "settings": settings, "params": params })
}

fn fx_json(fx: &[Device]) -> Value {
    Value::Array(fx.iter().map(device_json).collect())
}

fn play_json(play: &[Placement]) -> Value {
    Value::Array(play.iter().map(placement_json).collect())
}

fn track_json(song: &Song, id: &str) -> Value {
    let Some((index, _, t)) = song.tracks.get_full(id) else {
        return Value::Null;
    };
    let strip: Map<String, Value> = STRIP_PARAMS
        .iter()
        .map(|spec| {
            let value = t.strip.get(spec.id).copied().unwrap_or(spec.default);
            (spec.id.to_owned(), num(value))
        })
        .collect();
    json!({
        "index": index,
        "instrument": device_json(&t.instrument),
        "fx": fx_json(&t.fx),
        "strip": strip,
        "gain_db": num(t.gain_db),
        "pan": num(t.pan),
        "mute": t.mute,
        "solo": t.solo,
        "play": play_json(&t.play),
    })
}

fn clip_json(song: &Song, id: &str) -> Value {
    let Some(clip) = song.clips.get(id) else {
        return Value::Null;
    };
    let kind = clip.kind().name();
    match clip.steps_text() {
        Some(steps) => json!({ "kind": kind, "steps": steps }),
        None => json!({
            "kind": kind,
            "bars": effective_value(song, &SongPath::ClipBars { clip: id.to_owned() }),
            "notes": effective_value(song, &SongPath::ClipNotes { clip: id.to_owned() }),
        }),
    }
}

fn lanes_json(song: &Song, track: &str, param: &str) -> Value {
    let mut lanes: Vec<&Lane> = song
        .automation
        .iter()
        .filter(|l| l.track == track && l.param == param)
        .collect();
    lanes.sort_by_key(|l| l.start_bar);
    Value::Array(
        lanes
            .into_iter()
            .map(|l| {
                let points: Vec<Value> = l
                    .points
                    .iter()
                    .map(|&(x, v)| json!([num(x), num(v)]))
                    .collect();
                json!({ "at": l.start_bar + 1, "bars": l.bars, "points": points })
            })
            .collect(),
    )
}

fn scene_json(song: &Song, id: &str) -> Value {
    let Some((index, _, scene)) = song.scenes.get_full(id) else {
        return Value::Null;
    };
    json!({ "index": index, "slots": scene.slots })
}

fn scene_order_json(song: &Song) -> Value {
    Value::Array(song.scenes.keys().cloned().map(Value::from).collect())
}

fn sections_json(sections: &[Section]) -> Value {
    Value::Array(
        sections
            .iter()
            .map(|s| json!({ "name": s.name, "bars": s.bars }))
            .collect(),
    )
}

impl SongDoc {
    /// Writes `value` (as [`target_value`] gives it, `null` for absent) at
    /// `target`, through the same writers as the edit actions but without
    /// their clean-up: a removed track takes nothing else with it, so the
    /// validator may refuse a put that leaves the song inconsistent until
    /// another target is put back (a lane on a track not there yet). A put
    /// of the current value changes nothing.
    ///
    /// - a leaf goes through [`set`](Self::set);
    /// - `tracks.<t>`: `null` removes the track's table; a value inserts it
    ///   at `index` in the minimal layout (replacing the track if it is
    ///   there);
    /// - `tracks.<t>.instrument` replaces the instrument and its params;
    /// - `tracks.<t>.fx` sets the chain to the value's order: an effect in
    ///   both keeps its entry (params, settings, comments), one only in the
    ///   value is written from it, the others go;
    /// - `clips.<c>` removes the clip's table or appends one;
    /// - `tracks.<t>.play` and `automation.<t>.<p>` write the whole list in
    ///   one write; a stretch written into a table there now keeps its
    ///   comments, one that comes back after it went gets none (a lane's
    ///   value holds no comments);
    /// - `sections` rebuilds the list in the value's order, each entry
    ///   reusing the first unused table of that name (comments kept);
    /// - `scenes.<s>`: `null` removes the scene's table; a value inserts it
    ///   at `index` in the [`scene_table_toml`](tune_song::scene_table_toml)
    ///   layout, before the table of the scene now there, else after the
    ///   last (replacing the scene if it is there). A scene is compared
    ///   without its `index`: a put of its slots at another index writes
    ///   nothing, as the place is the `scenes` target's;
    /// - `scenes` reorders the scene tables to the value's order, the
    ///   scenes it does not name following in their order (its ids that are
    ///   not scenes are passed over); each table keeps its comments. A
    ///   reorder is checked against the file's order, which `Song` equality
    ///   ignores.
    pub fn put(&mut self, target: &Target, value: &Value) -> Result<(), EditError> {
        self.put_value(target, value).map_err(|e| match e {
            Bad::Edit(e) => e,
            Bad::Value(message) => EditError::BadValue {
                path: target.to_string(),
                message,
            },
        })
    }

    fn put_value(&mut self, target: &Target, value: &Value) -> Result<(), Bad> {
        let current = target_value(self.song(), target);
        let same = match target {
            // A scene's place is the order's: its `index` is no change.
            Target::Scene(_) => without_index(&current) == without_index(value),
            _ => current == *value,
        };
        if same {
            return Ok(());
        }
        let null = value.is_null();
        match target {
            Target::Value(path) => self.set(path, value)?,
            Target::Track(id) if null => self.put_track(id, None)?,
            Target::Track(id) => {
                let index = field(value, "index")?.as_u64();
                let index = index.and_then(|i| usize::try_from(i).ok());
                let index = index.ok_or("bad `index`")?;
                self.put_track(id, Some((index, track_from(value)?)))?;
            }
            // A part of a track that is gone is gone with it.
            Target::Instrument(t) | Target::Fx(t) | Target::Play(t) if null => {
                if self.song().tracks.contains_key(t) {
                    return Err("expected a value, not null".into());
                }
            }
            Target::Instrument(t) => self.put_instrument(t, device_from(value, true)?)?,
            Target::Fx(t) => {
                let fx = list(value)?.iter().map(|d| device_from(d, false));
                self.put_fx(t, fx.collect::<Result<_, _>>()?)?;
            }
            Target::Clip(c) if null => self.put_clip(c, None)?,
            Target::Clip(c) => {
                let kind = text(value, "kind")?;
                let kind = ClipKind::from_name(kind).ok_or("bad `kind`")?;
                let steps = value.get("steps").and_then(Value::as_str);
                let bars = match value.get("bars") {
                    Some(_) => Some(whole(value, "bars")?),
                    None => None,
                };
                self.put_clip(c, Some((kind, steps, bars, value.get("notes"))))?;
            }
            Target::Play(t) => {
                let play = list(value)?.iter().map(placement_from);
                self.put_play(t, play.collect::<Result<_, _>>()?)?;
            }
            Target::Lanes { track, param } => {
                let lanes = match value {
                    Value::Null => Vec::new(),
                    v => list(v)?
                        .iter()
                        .map(|l| lane_from(l, track, param))
                        .collect::<Result<_, _>>()?,
                };
                self.put_lanes(track, param, lanes)?;
            }
            Target::Sections => {
                let sections = list(value)?.iter().map(section_from);
                self.put_sections(sections.collect::<Result<_, _>>()?)?;
            }
            Target::Scene(id) if null => self.put_scene(id, None)?,
            Target::Scene(id) => {
                let index = field(value, "index")?.as_u64();
                let index = index.and_then(|i| usize::try_from(i).ok());
                let index = index.ok_or("bad `index`")?;
                self.put_scene(id, Some((index, scene_from(value)?)))?;
            }
            Target::SceneOrder => {
                let ids = list(value)?
                    .iter()
                    .map(|id| id.as_str().ok_or("bad scene id"));
                self.put_scene_order(&ids.collect::<Result<Vec<_>, _>>()?)?;
            }
        }
        Ok(())
    }
}

/// Why a [`SongDoc::put`] failed: the edit's error, or a value that is not
/// one of the target's.
enum Bad {
    Edit(EditError),
    Value(String),
}

impl From<EditError> for Bad {
    fn from(e: EditError) -> Self {
        Self::Edit(e)
    }
}

impl From<&str> for Bad {
    fn from(m: &str) -> Self {
        Self::Value(m.to_owned())
    }
}

impl From<String> for Bad {
    fn from(m: String) -> Self {
        Self::Value(m)
    }
}

fn field<'v>(v: &'v Value, key: &str) -> Result<&'v Value, Bad> {
    v.get(key)
        .ok_or_else(|| Bad::Value(format!("missing `{key}`")))
}

fn list(v: &Value) -> Result<&Vec<Value>, Bad> {
    v.as_array().ok_or_else(|| "expected an array".into())
}

fn number(v: &Value, key: &str) -> Result<f64, Bad> {
    field(v, key)?
        .as_f64()
        .ok_or_else(|| Bad::Value(format!("bad `{key}`")))
}

fn whole(v: &Value, key: &str) -> Result<u32, Bad> {
    field(v, key)?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| Bad::Value(format!("bad `{key}`")))
}

/// A 1-based bar as the 0-based one the model keeps.
fn bar0(v: &Value, key: &str) -> Result<u32, Bad> {
    whole(v, key)?
        .checked_sub(1)
        .ok_or_else(|| Bad::Value(format!("bad `{key}`")))
}

fn text<'v>(v: &'v Value, key: &str) -> Result<&'v str, Bad> {
    field(v, key)?
        .as_str()
        .ok_or_else(|| Bad::Value(format!("bad `{key}`")))
}

/// A device from its value: the params that differ from their defaults
/// become explicit, the rest stay implicit.
fn device_from(v: &Value, instrument: bool) -> Result<Device, Bad> {
    let name = text(v, "type")?;
    let module = Registry::builtin()
        .get(name)
        .filter(|m| m.is_instrument() == instrument)
        .ok_or_else(|| Bad::Value(format!("unknown module `{name}`")))?;
    let mut device = Device::new(text(v, "id")?, module);
    if let Some(settings) = v.get("settings").and_then(Value::as_object) {
        for (name, value) in settings {
            let (i, spec) = module
                .setting(name)
                .ok_or_else(|| Bad::Value(format!("unknown setting `{name}`")))?;
            let value = value.as_str().unwrap_or_default();
            device.settings[i] = spec
                .values
                .iter()
                .find(|x| **x == value)
                .ok_or_else(|| Bad::Value(format!("bad value for setting `{name}`")))?;
        }
    }
    if let Some(params) = v.get("params").and_then(Value::as_object) {
        for (local, value) in params {
            let (_, spec) = module
                .param(local)
                .ok_or_else(|| Bad::Value(format!("unknown param `{local}`")))?;
            let value = value
                .as_f64()
                .ok_or_else(|| Bad::Value(format!("bad `{local}`")))?;
            if value != spec.default {
                device.params.insert(local.clone(), value);
            }
        }
    }
    Ok(device)
}

/// A track from its value. Strip params that differ from their defaults
/// become explicit; so an explicit `send.delay = -60`, which builds the
/// delay bus, does not come back.
fn track_from(v: &Value) -> Result<Track, Bad> {
    let mut strip = indexmap::IndexMap::new();
    if let Some(values) = v.get("strip").and_then(Value::as_object) {
        for spec in STRIP_PARAMS {
            if let Some(value) = values.get(spec.id).and_then(Value::as_f64)
                && value != spec.default
            {
                strip.insert(spec.id.to_owned(), value);
            }
        }
    }
    let flag = |key: &str| {
        field(v, key)?
            .as_bool()
            .ok_or_else(|| Bad::Value(format!("bad `{key}`")))
    };
    Ok(Track {
        instrument: device_from(field(v, "instrument")?, true)?,
        fx: list(field(v, "fx")?)?
            .iter()
            .map(|d| device_from(d, false))
            .collect::<Result<_, _>>()?,
        strip,
        gain_db: number(v, "gain_db")?,
        pan: number(v, "pan")?,
        mute: flag("mute")?,
        solo: flag("solo")?,
        play: list(field(v, "play")?)?
            .iter()
            .map(placement_from)
            .collect::<Result<_, _>>()?,
    })
}

fn placement_from(v: &Value) -> Result<Placement, Bad> {
    Ok(Placement {
        start_bar: bar0(v, "at")?,
        bars: whole(v, "bars")?,
        clip: text(v, "clip")?.to_owned(),
        offset_bars: v.get("offset").map_or(Ok(0), |_| whole(v, "offset"))?,
    })
}

fn lane_from(v: &Value, track: &str, param: &str) -> Result<Lane, Bad> {
    let points = list(field(v, "points")?)?
        .iter()
        .map(|p| match p.as_array().map(Vec::as_slice) {
            Some([x, value]) => x.as_f64().zip(value.as_f64()).ok_or("bad `points`"),
            _ => Err("bad `points`"),
        })
        .collect::<Result<_, _>>()?;
    Ok(Lane {
        track: track.to_owned(),
        param: param.to_owned(),
        start_bar: bar0(v, "at")?,
        bars: whole(v, "bars")?,
        points,
    })
}

/// `v` without its `index` key, as a track or scene is compared.
pub(crate) fn without_index(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(map) = v.as_object_mut() {
        map.remove("index");
    }
    v
}

fn scene_from(v: &Value) -> Result<Scene, Bad> {
    let slots = field(v, "slots")?.as_object().ok_or("bad `slots`")?;
    let slots = slots.iter().map(|(track, clip)| match clip.as_str() {
        Some(clip) => Ok((track.clone(), clip.to_owned())),
        None => Err(Bad::Value(format!("bad slot `{track}`"))),
    });
    Ok(Scene {
        slots: slots.collect::<Result<_, _>>()?,
    })
}

fn section_from(v: &Value) -> Result<Section, Bad> {
    Ok(Section {
        name: text(v, "name")?.to_owned(),
        bars: whole(v, "bars")?,
    })
}
