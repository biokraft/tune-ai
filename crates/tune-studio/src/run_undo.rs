//! Undoing one agent run: every target the run's writes changed goes back
//! to its value before the run, unless someone else changed it since.
//!
//! The edit log's action records say what each write changed, as undo
//! targets (see [`crate::target`]). A run undo reads the records of one
//! run and plans each target: its value before the run's first change and
//! after its last. A target another write touched after the run's last
//! change to it is kept (someone changed it later); one whose value now is
//! not the run's is kept too (changed outside the studio, or by an edit the
//! log missed). A track's placements and a lane's stretches are merged an
//! element at a time, so the producer's own placements stay. Reverts are
//! written newest first, each on a copy of the document; one the song
//! refuses (a clip still placed) waits for the others and is kept if it
//! still cannot be written. When in doubt a target is kept, never
//! overwritten.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tune_dsp::STRIP_PARAMS;

use crate::doc::{EditError, SongDoc};
use crate::error::StudioError;
use crate::log::{ActionRecord, Source};
use crate::path::SongPath;
use crate::target::{Target, TargetChange, target_value};

/// What undoing a run did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct RunUndo {
    pub run: String,
    pub title: Option<String>,
    /// The targets written back, in the order they were written.
    pub reverted: Vec<TargetItem>,
    /// The targets left as they are, and why.
    pub kept: Vec<KeptTarget>,
    /// The targets that already held their value before the run.
    #[ts(type = "Array<string>")]
    pub already: Vec<Target>,
}

/// One target, or one element of a list target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct TargetItem {
    #[ts(type = "string")]
    pub target: Target,
    /// The placement or lane stretch, for `tracks.<t>.play` and
    /// `automation.<t>.<p>`; `None` for the others.
    pub item: Option<Value>,
}

/// A target (or list element) a run undo left as it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct KeptTarget {
    #[ts(type = "string")]
    pub target: Target,
    pub item: Option<Value>,
    pub reason: KeptReason,
    /// Who changed it later, when the edit log says.
    pub by: Option<Source>,
    /// Why the song refused the revert, for [`KeptReason::StillUsed`].
    pub message: Option<String>,
}

/// Why a target was kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum KeptReason {
    /// Someone changed it after the run.
    ChangedLater,
    /// The song refused the revert: something still uses it.
    StillUsed,
}

/// One target the run changed.
struct Plan {
    target: Target,
    /// The value before the run's first change to it.
    before: Value,
    /// The value after the run's last change to it, its parts' changes
    /// folded in.
    after: Value,
    /// The record (index into all records) of the run's first and last
    /// change to it, and the last change's position in that record.
    first: usize,
    last: usize,
    pos: usize,
    /// Every record of the run that touched it: the record, and the old
    /// value when the change was to the target itself (`None` for a part
    /// folded in).
    touched: Vec<(usize, Option<Value>)>,
}

/// What to do with one target, decided when its turn comes.
enum Decision {
    Already,
    Kept(Vec<KeptTarget>),
    /// Write `value`, reverting `items` (an empty one for a whole target),
    /// and keep `kept`.
    Revert {
        value: Value,
        items: Vec<Option<Value>>,
        kept: Vec<KeptTarget>,
    },
}

/// A revert the song refused, waiting for the others.
struct Waiting {
    target: Target,
    value: Value,
    items: Vec<Option<Value>>,
    error: EditError,
}

/// Undoes run `run` in `doc`, as `records` (the whole edit log, in file
/// order) describe it. Gives what was reverted and what was kept; `doc`
/// holds every revert that could be written. No record of the run gives
/// [`StudioError::NoSuchRun`].
pub fn undo_run(
    doc: &mut SongDoc,
    records: &[ActionRecord],
    run: &str,
) -> Result<RunUndo, StudioError> {
    let mine: Vec<usize> = (0..records.len())
        .filter(|&i| records[i].run.as_deref() == Some(run))
        .collect();
    if mine.is_empty() {
        return Err(StudioError::NoSuchRun(run.to_owned()));
    }
    let title = mine
        .iter()
        .rev()
        .find_map(|&i| records[i].run_title.clone());
    let plans = plan(records, &mine);

    // Newest first; within one record, last change first.
    let mut order: Vec<usize> = (0..plans.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse((plans[k].last, plans[k].pos)));

    let mut out = RunUndo {
        run: run.to_owned(),
        title,
        reverted: Vec::new(),
        kept: Vec::new(),
        already: Vec::new(),
    };
    let mut waiting = Vec::new();
    for k in order {
        let p = &plans[k];
        let foreign: Vec<usize> = (p.first + 1..records.len())
            .filter(|&j| records[j].run.as_deref() != Some(run))
            .filter(|&j| {
                let mut targets = records[j].targets.iter();
                targets.any(|c| overlap(p.side(), (&c.target, &c.old, &c.new)))
            })
            .collect();
        let decision = if is_list(&p.target) {
            decide_list(doc, p, &foreign, records)
        } else {
            decide(doc, p, &foreign, records)
        };
        match decision {
            Decision::Already => out.already.push(p.target.clone()),
            Decision::Kept(kept) => out.kept.extend(kept),
            Decision::Revert { value, items, kept } => {
                out.kept.extend(kept);
                match put(doc, &p.target, &value) {
                    Ok(()) => out.reverted.extend(item_list(&p.target, items)),
                    Err(error) => waiting.push(Waiting {
                        target: p.target.clone(),
                        value,
                        items,
                        error,
                    }),
                }
            }
        }
    }
    // A refused revert may go through once others are written.
    loop {
        let before = waiting.len();
        let mut still = Vec::new();
        for w in waiting {
            match put(doc, &w.target, &w.value) {
                Ok(()) => out.reverted.extend(item_list(&w.target, w.items)),
                Err(error) => still.push(Waiting { error, ..w }),
            }
        }
        waiting = still;
        if waiting.is_empty() || waiting.len() == before {
            break;
        }
    }
    for w in waiting {
        let message = refusal(&w.error);
        out.kept.extend(w.items.into_iter().map(|item| KeptTarget {
            target: w.target.clone(),
            item,
            reason: KeptReason::StillUsed,
            by: None,
            message: Some(message.clone()),
        }));
    }
    Ok(out)
}

/// The run's targets, in the order the run first changed them. A change
/// to a part of an item the run created (a leaf of a new track, effect,
/// instrument or clip, a new track's placements) folds into that item.
fn plan(records: &[ActionRecord], mine: &[usize]) -> Vec<Plan> {
    let mut plans: Vec<Plan> = Vec::new();
    for &i in mine {
        for (pos, c) in records[i].targets.iter().enumerate() {
            let (k, own) = match owner(&plans, &c.target) {
                Some(k) => {
                    let p = &mut plans[k];
                    fold(&p.target, &mut p.after, c);
                    (k, None)
                }
                None => match plans.iter().position(|p| p.target == c.target) {
                    Some(k) => {
                        plans[k].after = c.new.clone();
                        (k, Some(c.old.clone()))
                    }
                    None => {
                        plans.push(Plan {
                            target: c.target.clone(),
                            before: c.old.clone(),
                            after: c.new.clone(),
                            first: i,
                            last: i,
                            pos,
                            touched: Vec::new(),
                        });
                        (plans.len() - 1, Some(c.old.clone()))
                    }
                },
            };
            let p = &mut plans[k];
            p.last = i;
            p.pos = pos;
            p.touched.push((i, own));
        }
    }
    plans
}

impl Plan {
    fn side(&self) -> (&Target, &Value, &Value) {
        (&self.target, &self.before, &self.after)
    }
}

/// A target other than a list: back to its value before the run, unless
/// it holds that already or someone changed it later.
fn decide(doc: &SongDoc, p: &Plan, foreign: &[usize], records: &[ActionRecord]) -> Decision {
    let current = target_value(doc.song(), &p.target);
    if approx(p, &current, &p.before) {
        return Decision::Already;
    }
    let kept = |by: Option<Source>| {
        Decision::Kept(vec![KeptTarget {
            target: p.target.clone(),
            item: None,
            reason: KeptReason::ChangedLater,
            by,
            message: None,
        }])
    };
    let mut before = p.before.clone();
    if let Some(&j) = foreign.last() {
        let by = Some(records[j].source);
        if j > p.last {
            return kept(by);
        }
        // Someone changed it between the run's changes: the run's later
        // changes started from their value. A part's change gives no
        // value of the whole, so the item is kept.
        match p.touched.iter().find(|(i, _)| *i > j) {
            Some((_, Some(old))) => before = old.clone(),
            _ => return kept(by),
        }
        if approx(p, &current, &before) {
            return Decision::Already;
        }
    }
    if !approx(p, &current, &p.after) {
        return kept(None);
    }
    Decision::Revert {
        value: before,
        items: vec![None],
        kept: Vec::new(),
    }
}

/// A track's placements or a lane's stretches, element by element: an
/// element the run added goes if it is still there; one it removed comes
/// back if nothing there now overlaps it.
fn decide_list(doc: &SongDoc, p: &Plan, foreign: &[usize], records: &[ActionRecord]) -> Decision {
    let current = target_value(doc.song(), &p.target);
    if current == p.before {
        return Decision::Already;
    }
    let by = foreign.last().map(|&j| records[j].source);
    let kept = |item: &Value| KeptTarget {
        target: p.target.clone(),
        item: Some(item.clone()),
        reason: KeptReason::ChangedLater,
        by,
        message: None,
    };
    let before = elements(&p.before);
    let after = elements(&p.after);
    let added = minus(&after, &before);
    let removed = minus(&before, &after);
    // The track is gone: nothing to take out or put back into.
    let Some(mut next) = current.as_array().cloned() else {
        return Decision::Kept(added.iter().chain(&removed).map(kept).collect());
    };
    let mut items = Vec::new();
    let mut out = Vec::new();
    for a in &added {
        match next.iter().position(|e| e == a) {
            Some(i) => {
                next.remove(i);
                items.push(Some(a.clone()));
            }
            None => out.push(kept(a)),
        }
    }
    for r in &removed {
        if next.iter().any(|e| spans_overlap(e, r)) {
            out.push(kept(r));
        } else {
            next.push(r.clone());
            items.push(Some(r.clone()));
        }
    }
    if items.is_empty() {
        return Decision::Kept(out);
    }
    next.sort_by_key(|e| e["at"].as_u64());
    Decision::Revert {
        value: Value::Array(next),
        items,
        kept: out,
    }
}

fn is_list(t: &Target) -> bool {
    matches!(t, Target::Play(_) | Target::Lanes { .. })
}

fn elements(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// The elements of `a` not in `b`, each counted once per occurrence.
fn minus(a: &[Value], b: &[Value]) -> Vec<Value> {
    let mut rest = b.to_vec();
    a.iter()
        .filter(|x| match rest.iter().position(|y| y == *x) {
            Some(i) => {
                rest.remove(i);
                false
            }
            None => true,
        })
        .cloned()
        .collect()
}

/// Whether two placements or stretches (`at`, `bars`) share a bar.
fn spans_overlap(a: &Value, b: &Value) -> bool {
    let span = |v: &Value| {
        let at = v["at"].as_u64().unwrap_or(0);
        (at, at + v["bars"].as_u64().unwrap_or(0))
    };
    let ((a0, a1), (b0, b1)) = (span(a), span(b));
    a0 < b1 && b0 < a1
}

/// Why the song refused a revert, in one line: the validator's first
/// message (without its position, which is in a text never written), else
/// the error's first line.
fn refusal(e: &EditError) -> String {
    match e {
        EditError::Invalid(errors) if !errors.is_empty() => errors[0].message.clone(),
        e => e.to_string().lines().next().unwrap_or_default().to_owned(),
    }
}

/// Puts `value` at `target` on a copy of `doc`, keeping the copy when the
/// song takes it.
fn put(doc: &mut SongDoc, target: &Target, value: &Value) -> Result<(), EditError> {
    let mut copy = doc.clone();
    copy.put(target, value)?;
    *doc = copy;
    Ok(())
}

fn item_list(target: &Target, items: Vec<Option<Value>>) -> Vec<TargetItem> {
    let target = target.clone();
    items
        .into_iter()
        .map(|item| TargetItem {
            target: target.clone(),
            item,
        })
        .collect()
}

/// Whether `x` and `y` are the same value of the planned target: a track
/// regardless of its position; an effect chain by its devices' ids and
/// modules in order, plus the whole of each device the run added (the
/// others' params are targets of their own); anything else exactly.
fn approx(p: &Plan, x: &Value, y: &Value) -> bool {
    match &p.target {
        Target::Track(_) => without_index(x) == without_index(y),
        Target::Fx(_) => {
            let before = device_keys(&p.before);
            let added: Vec<(&str, &str)> = device_keys(&p.after)
                .into_iter()
                .filter(|k| !before.contains(k))
                .collect();
            let device = |v: &Value, key: (&str, &str)| {
                v.as_array()
                    .and_then(|a| a.iter().find(|d| device_key(d) == Some(key)))
                    .cloned()
            };
            device_keys(x) == device_keys(y) && added.iter().all(|&k| device(x, k) == device(y, k))
        }
        _ => x == y,
    }
}

fn without_index(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(map) = v.as_object_mut() {
        map.remove("index");
    }
    v
}

/// A device value's (id, module).
fn device_key(d: &Value) -> Option<(&str, &str)> {
    Some((d["id"].as_str()?, d["type"].as_str()?))
}

/// The (id, module) of each device in an effect chain's value, in order.
fn device_keys(v: &Value) -> Vec<(&str, &str)> {
    let devices = v.as_array().map(Vec::as_slice).unwrap_or_default();
    devices.iter().filter_map(device_key).collect()
}

/// The ids of the devices in an instrument's (one device) or a chain's
/// value.
fn device_ids(v: &Value) -> Vec<&str> {
    match v {
        Value::Array(devices) => devices.iter().filter_map(|d| d["id"].as_str()).collect(),
        Value::Object(_) => v["id"].as_str().into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Whether a change to one target can change the other. Each side is a
/// target with its old and new value.
fn overlap(a: (&Target, &Value, &Value), b: (&Target, &Value, &Value)) -> bool {
    let (x, y) = (a.0.to_string(), b.0.to_string());
    x == y || under(&x, &y) || under(&y, &x) || holds(a, b.0) || holds(b, a.0)
}

/// Whether `part` names something inside `whole` (`tracks.acid.gain_db`
/// inside `tracks.acid`).
fn under(part: &str, whole: &str) -> bool {
    part.strip_prefix(whole).is_some_and(|r| r.starts_with('.'))
}

/// The holdings the target strings do not show: a track holds its lanes,
/// an instrument or chain the params of its devices (in either value).
fn holds((whole, old, new): (&Target, &Value, &Value), part: &Target) -> bool {
    match (whole, part) {
        (Target::Track(t), Target::Lanes { track, .. }) => t == track,
        (
            Target::Instrument(t) | Target::Fx(t),
            Target::Value(SongPath::TrackParam { track, param }),
        ) if t == track => param
            .split_once('.')
            .is_some_and(|(id, _)| device_ids(old).contains(&id) || device_ids(new).contains(&id)),
        _ => false,
    }
}

/// The plan of the item the run created that `target` is a part of: a
/// track (anything under `tracks.<t>`), an effect (its params and
/// settings), a new instrument (the same) or a clip (its leaves).
fn owner(plans: &[Plan], target: &Target) -> Option<usize> {
    let find = |item: Target, created: &dyn Fn(&Plan) -> bool| {
        plans.iter().position(|p| p.target == item && created(p))
    };
    let new = |p: &Plan| p.before.is_null() && !p.after.is_null();
    let track = match target {
        Target::Value(SongPath::TrackField { track, .. } | SongPath::TrackParam { track, .. })
        | Target::Instrument(track)
        | Target::Fx(track)
        | Target::Play(track) => Some(track),
        _ => None,
    };
    if let Some(k) = track.and_then(|t| find(Target::Track(t.clone()), &new)) {
        return Some(k);
    }
    if let Target::Value(SongPath::TrackParam { track, param }) = target
        && let Some((id, _)) = param.split_once('.')
    {
        let added = |p: &Plan| {
            let before = device_keys(&p.before);
            device_keys(&p.after)
                .into_iter()
                .any(|k| k.0 == id && !before.contains(&k))
        };
        if let Some(k) = find(Target::Fx(track.clone()), &added) {
            return Some(k);
        }
        let swapped = |p: &Plan| {
            device_key(&p.before) != device_key(&p.after) && p.after["id"].as_str() == Some(id)
        };
        if let Some(k) = find(Target::Instrument(track.clone()), &swapped) {
            return Some(k);
        }
    }
    if let Target::Value(
        SongPath::Clip { clip } | SongPath::ClipNotes { clip } | SongPath::ClipBars { clip },
    ) = target
    {
        return find(Target::Clip(clip.clone()), &new);
    }
    None
}

/// Folds a change to a part of `item` into the item's value `after`. A
/// part it cannot place is left out, so the item reads as changed later
/// and is kept.
fn fold(item: &Target, after: &mut Value, c: &TargetChange) {
    let new = c.new.clone();
    let slot = match (item, &c.target) {
        (Target::Track(_), Target::Instrument(_)) => after.get_mut("instrument"),
        (Target::Track(_), Target::Fx(_)) => after.get_mut("fx"),
        (Target::Track(_), Target::Play(_)) => after.get_mut("play"),
        (Target::Track(_), Target::Value(SongPath::TrackField { field, .. })) => {
            after.get_mut(field.key())
        }
        (Target::Track(_), Target::Value(SongPath::TrackParam { param, .. }))
            if STRIP_PARAMS.iter().any(|s| s.id == param) =>
        {
            after.get_mut("strip").and_then(|s| s.get_mut(param))
        }
        (Target::Track(_), Target::Value(SongPath::TrackParam { param, .. })) => {
            if has_id(&after["instrument"], param) {
                after
                    .get_mut("instrument")
                    .and_then(|d| device_slot(d, param))
            } else {
                after
                    .get_mut("fx")
                    .and_then(Value::as_array_mut)
                    .and_then(|fx| fx.iter_mut().find(|d| has_id(d, param)))
                    .and_then(|d| device_slot(d, param))
            }
        }
        (Target::Fx(_), Target::Value(SongPath::TrackParam { param, .. })) => after
            .as_array_mut()
            .and_then(|fx| fx.iter_mut().find(|d| has_id(d, param)))
            .and_then(|d| device_slot(d, param)),
        (Target::Instrument(_), Target::Value(SongPath::TrackParam { param, .. })) => {
            device_slot(after, param)
        }
        (Target::Clip(_), Target::Value(SongPath::Clip { .. })) => after.get_mut("steps"),
        (Target::Clip(_), Target::Value(SongPath::ClipNotes { .. })) => after.get_mut("notes"),
        (Target::Clip(_), Target::Value(SongPath::ClipBars { .. })) => after.get_mut("bars"),
        _ => None,
    };
    if let Some(slot) = slot {
        *slot = new;
    }
}

/// Whether device value `d` is the device of track param `param`
/// (`<id>.<name>`).
fn has_id(d: &Value, param: &str) -> bool {
    param
        .split_once('.')
        .is_some_and(|(id, _)| d["id"].as_str() == Some(id))
}

/// The param or setting `param` (`<id>.<name>`) names in device value `d`.
fn device_slot<'v>(d: &'v mut Value, param: &str) -> Option<&'v mut Value> {
    let (_, name) = param.split_once('.')?;
    let key = if d["params"].get(name).is_some() {
        "params"
    } else {
        "settings"
    };
    d.get_mut(key)?.get_mut(name)
}
