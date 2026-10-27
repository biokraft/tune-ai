//! Undoing one agent run: every target the run's writes changed goes back
//! to its value before the run, unless someone else changed it since.
//!
//! The edit log's action records say what each write changed, as undo
//! targets (see [`crate::target`]). A run undo reads the records of one
//! run and plans each target: its value before the run's first change and
//! after its last. A target another write touched after the run's last
//! change to it is kept (someone changed it later); one whose value now is
//! not the run's is kept too (changed outside the studio, or by an edit the
//! log missed), and so is one that changed between two of the run's
//! writes with no logged edit to explain it (a hand edit with the studio
//! closed, carried along by the run's next write). A track's placements
//! and a lane's stretches are merged an element at a time, so the
//! producer's own placements stay. Reverts are written newest first, each
//! on a copy of the document; one the song refuses (a clip still placed)
//! waits for the others and is kept if it still cannot be written. When in
//! doubt a target is kept, never overwritten.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tune_dsp::STRIP_PARAMS;

use crate::doc::{EditError, SongDoc};
use crate::error::StudioError;
use crate::log::{ActionRecord, Source};
use crate::path::SongPath;
use crate::target::{Target, TargetChange, target_value, without_index};

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
    /// Each pair of the run's records touching it, one after the other,
    /// between which its value changed: the later one's old value is not
    /// what the earlier one left.
    gaps: Vec<(usize, usize)>,
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

/// Reverts the song refused, waiting for the others: one target's, or a
/// section edit's (all written together).
struct Waiting {
    puts: Vec<Put>,
    error: EditError,
}

/// Undoes run `run` in `doc`, as `records` (the whole edit log, in file
/// order) describe it. Gives what was reverted and what was kept; `doc`
/// holds every revert that could be written. No record of the run gives
/// [`StudioError::NoSuchRun`].
///
/// A section edit's changes (the sections, and the placements and lane
/// stretches of the records that changed them) go back together or not
/// at all: kept whole when any part of it would be kept, written in one
/// go otherwise. So does each rename (of a track, clip, device or scene)
/// with everything its record changed.
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
    let key = |k: usize| (plans[k].last, plans[k].pos);

    // Each unit is one target, or a group that goes back whole (the
    // section edit's, a rename's, merged where they share a target);
    // newest first (a group by its newest member), within one record last
    // change first.
    let section = section_group(&plans, records, &mine);
    let rearranged = rearranged_since(records, &mine, run);
    let mut groups = rename_groups(&plans, records, &mine);
    if !section.is_empty() {
        groups.insert(0, section.clone());
    }
    let groups = merged(groups);
    let mut units: Vec<(Vec<usize>, bool)> = (0..plans.len())
        .filter(|k| !groups.iter().any(|g| g.contains(k)))
        .map(|k| (vec![k], false))
        .collect();
    units.extend(groups.into_iter().map(|g| (g, true)));
    for (unit, _) in &mut units {
        unit.sort_by_key(|&k| std::cmp::Reverse(key(k)));
    }
    units.sort_by_key(|(unit, _)| std::cmp::Reverse(key(unit[0])));

    let mut out = RunUndo {
        run: run.to_owned(),
        title,
        reverted: Vec::new(),
        kept: Vec::new(),
        already: Vec::new(),
    };
    let mut waiting = Vec::new();
    for (unit, grouped) in units {
        // Each part's decision, and for a part the run changed, who last
        // wrote it since (when the log says).
        let decided: Vec<(&Plan, Decision, Option<Source>)> = unit
            .iter()
            .map(|&k| {
                let p = &plans[k];
                let (others, foreign) = other_writes(p, records, run);
                let decision = if is_list(&p.target) {
                    decide_list(doc, p, &others, &foreign, records)
                } else {
                    decide(doc, p, &foreign, records)
                };
                let decision = match track_kept(doc, &plans, &unit, p, records, run) {
                    Some(by) if !matches!(decision, Decision::Kept(_)) => {
                        Decision::Kept(kept_whole(p, by))
                    }
                    _ => decision,
                };
                let by = foreign.last().map(|&j| records[j].source);
                (p, decision, by)
            })
            .collect();
        // The first kept part, if any: it keeps the whole unit.
        let cause = decided.iter().find_map(|(_, d, _)| match d {
            Decision::Already => None,
            Decision::Kept(kept) | Decision::Revert { kept, .. } => kept.first(),
        });
        // A part of a group the run changed that is back at its value
        // before the run was set back since (a rename's new name renamed
        // again or removed): writing the rest back would put the old name
        // beside what became of the new one, so the group is kept, naming
        // who set it back (`None` when the log does not say).
        let writes = decided
            .iter()
            .any(|(_, d, _)| matches!(d, Decision::Revert { .. }));
        let set_back = |p: &Plan, d: &Decision| {
            grouped && writes && matches!(d, Decision::Already) && changed(p)
        };
        let set_back_by = decided
            .iter()
            .find(|(p, d, _)| set_back(p, d))
            .map(|(_, _, by)| *by);
        // A section edit is also kept when anyone has changed the
        // arrangement since: what they placed sits where the section put it.
        let forced = rearranged.filter(|_| unit.iter().any(|k| section.contains(k)));
        let whole = cause.is_none() && forced.is_none() && set_back_by.is_none();
        let cause_by = (cause.map(|k| k.by))
            .or(forced.map(Some))
            .or(set_back_by)
            .flatten();
        let mut puts = Vec::new();
        for (p, decision, by) in decided {
            match decision {
                Decision::Already if set_back(p, &decision) => {
                    out.kept
                        .extend(changed_items(p).into_iter().map(|item| KeptTarget {
                            target: p.target.clone(),
                            item,
                            reason: KeptReason::ChangedLater,
                            by,
                            message: None,
                        }));
                }
                Decision::Already => out.already.push(p.target.clone()),
                Decision::Kept(kept) => out.kept.extend(kept),
                Decision::Revert { value, items, kept } if whole || !grouped => {
                    out.kept.extend(kept);
                    puts.push((p.target.clone(), value, items));
                }
                // A part of a group is kept: so is the rest of it, naming
                // who changed that part.
                Decision::Revert { items, kept, .. } => {
                    out.kept.extend(kept);
                    out.kept.extend(items.into_iter().map(|item| KeptTarget {
                        target: p.target.clone(),
                        item,
                        reason: KeptReason::ChangedLater,
                        by: cause_by,
                        message: None,
                    }));
                }
            }
        }
        if puts.is_empty() {
            continue;
        }
        match put_all(doc, &puts) {
            Ok(()) => out.reverted.extend(reverted_items(puts)),
            Err(error) => waiting.push(Waiting { puts, error }),
        }
    }
    // A refused revert may go through once others are written.
    loop {
        let before = waiting.len();
        let mut still = Vec::new();
        for w in waiting {
            match put_all(doc, &w.puts) {
                Ok(()) => out.reverted.extend(reverted_items(w.puts)),
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
        for (target, _, items) in w.puts {
            out.kept.extend(items.into_iter().map(|item| KeptTarget {
                target: target.clone(),
                item,
                reason: KeptReason::StillUsed,
                by: None,
                message: Some(message.clone()),
            }));
        }
    }
    Ok(out)
}

/// For a part of a track (a lane param's stretches) outside the unit of
/// the run's plan of that whole track: `Some` with its `by` when that
/// plan is kept. A track the run removed that someone made again (by hand
/// too) is kept, and what the run took with it must not come back onto
/// theirs. The track's plan is decided now: the reverts before its turn
/// (its parts, in the same write) do not change its value.
fn track_kept(
    doc: &SongDoc,
    plans: &[Plan],
    unit: &[usize],
    p: &Plan,
    records: &[ActionRecord],
    run: &str,
) -> Option<Option<Source>> {
    let Target::Lanes { track, .. } = &p.target else {
        return None;
    };
    let k = plans
        .iter()
        .position(|t| matches!(&t.target, Target::Track(u) if u == track))?;
    if unit.contains(&k) {
        return None;
    }
    let (_, foreign) = other_writes(&plans[k], records, run);
    match decide(doc, &plans[k], &foreign, records) {
        Decision::Kept(kept) => Some(kept.first().and_then(|k| k.by)),
        _ => None,
    }
}

/// `p` kept whole, changed later by `by`: each element the run added or
/// removed for a list, else the target.
fn kept_whole(p: &Plan, by: Option<Source>) -> Vec<KeptTarget> {
    changed_items(p)
        .into_iter()
        .map(|item| KeptTarget {
            target: p.target.clone(),
            item,
            reason: KeptReason::ChangedLater,
            by,
            message: None,
        })
        .collect()
}

/// Whether the run changed `p`'s target: its value after the run's last
/// change is not the one before its first.
fn changed(p: &Plan) -> bool {
    if is_list(&p.target) {
        p.before != p.after
    } else {
        !approx(p, &p.before, &p.after)
    }
}

/// The items the run changed in `p`'s target: each element it added or
/// removed for a list, else the target itself (`None`).
fn changed_items(p: &Plan) -> Vec<Option<Value>> {
    if !is_list(&p.target) {
        return vec![None];
    }
    let (before, after) = (elements(&p.before), elements(&p.after));
    let added = minus(&after, &before);
    added
        .into_iter()
        .chain(minus(&before, &after))
        .map(Some)
        .collect()
}

/// The other writes since the run first changed `p`'s target, and those
/// of them that touch it.
fn other_writes(p: &Plan, records: &[ActionRecord], run: &str) -> (Vec<usize>, Vec<usize>) {
    let others: Vec<usize> = (p.first + 1..records.len())
        .filter(|&j| records[j].run.as_deref() != Some(run))
        .collect();
    let foreign = others
        .iter()
        .copied()
        .filter(|&j| {
            let mut targets = records[j].targets.iter();
            targets.any(|c| overlap(p.side(), (&c.target, &c.old, &c.new)))
        })
        .collect();
    (others, foreign)
}

/// The source of the first other write, after the run's first section
/// edit, that changed any placements, lane stretches, sections or a whole
/// track; `None` when there is none (or the run changed no section).
fn rearranged_since(records: &[ActionRecord], mine: &[usize], run: &str) -> Option<Source> {
    let changes_sections = |i: usize| {
        let mut targets = records[i].targets.iter();
        targets.any(|c| c.target == Target::Sections)
    };
    let first = mine.iter().copied().find(|&i| changes_sections(i))?;
    // A whole track coming or going (made, removed, brought back) carries
    // its placements with it.
    let arrangement = |c: &TargetChange| {
        is_list(&c.target) || matches!(c.target, Target::Sections | Target::Track(_))
    };
    (first + 1..records.len())
        .filter(|&j| records[j].run.as_deref() != Some(run))
        .find(|&j| records[j].targets.iter().any(arrangement))
        .map(|j| records[j].source)
}

/// The plans of the run's section edit: the sections, every placement
/// and lane target a record of the run changed together with the
/// sections, and the item the run created that such a change folded into
/// (a new track whose placements the edit moved). Empty when the run
/// changed no section.
fn section_group(plans: &[Plan], records: &[ActionRecord], mine: &[usize]) -> Vec<usize> {
    let Some(sections) = plans.iter().position(|p| p.target == Target::Sections) else {
        return Vec::new();
    };
    let section_records: Vec<&ActionRecord> = mine
        .iter()
        .map(|&i| &records[i])
        .filter(|r| r.targets.iter().any(|c| c.target == Target::Sections))
        .collect();
    let mut group = vec![sections];
    group.extend((0..plans.len()).filter(|&k| {
        is_list(&plans[k].target)
            && section_records
                .iter()
                .any(|r| r.targets.iter().any(|c| c.target == plans[k].target))
    }));
    let folded = section_records.iter().flat_map(|r| &r.targets);
    let folded = folded.filter(|c| is_list(&c.target));
    for k in folded.filter_map(|c| owner(plans, &c.target)) {
        if !group.contains(&k) {
            group.push(k);
        }
    }
    group
}

/// One group per record of the run that renamed a track, clip, device or
/// scene: every plan its changes belong to (the target, or the item the run
/// created that it folded into). A rename is a removal and an addition
/// (`tracks.<old>` gone, `tracks.<new>` made, the lanes and slots moved
/// over): reverting one half without the other doubles the item or orphans
/// it.
fn rename_groups(plans: &[Plan], records: &[ActionRecord], mine: &[usize]) -> Vec<Vec<usize>> {
    const RENAMES: [&str; 4] = [
        "rename_track",
        "rename_clip",
        "rename_device",
        "rename_scene",
    ];
    let renames = mine.iter().map(|&i| &records[i]).filter(|r| {
        let mut actions = r.actions.iter();
        actions.any(|a| RENAMES.contains(&a.as_str()))
    });
    renames
        .map(|r| {
            let mut group = Vec::new();
            for c in &r.targets {
                let k = owner(plans, &c.target)
                    .or_else(|| plans.iter().position(|p| p.target == c.target));
                if let Some(k) = k
                    && !group.contains(&k)
                {
                    group.push(k);
                }
            }
            group
        })
        .filter(|g| !g.is_empty())
        .collect()
}

/// `groups` with every two that share a plan joined into one.
fn merged(groups: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for mut g in groups {
        // Fold in every group already there that shares a plan with it.
        let mut i = 0;
        while i < out.len() {
            if out[i].iter().any(|k| g.contains(k)) {
                for k in out.remove(i) {
                    if !g.contains(&k) {
                        g.push(k);
                    }
                }
            } else {
                i += 1;
            }
        }
        out.push(g);
    }
    out
}

/// The run's targets, in the order the run first changed them. A change
/// to a part of an item the run created (a leaf of a new track, effect,
/// instrument or clip, a new track's placements, a new scene's slots) folds
/// into that item.
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
                        let p = &mut plans[k];
                        if !approx(p, &c.old, &p.after) {
                            p.gaps.push((p.last, i));
                        }
                        p.after = c.new.clone();
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
                            gaps: Vec::new(),
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

    /// Whether something the edit log does not show changed the target
    /// between two of the run's writes: a gap no other logged write (of
    /// `foreign`, those touching it) explains. With the studio closed, a
    /// hand edit made then is carried in the run's later write, as if the
    /// run had made it.
    fn unlogged_gap(&self, foreign: &[usize]) -> bool {
        let logged = |a: usize, b: usize| foreign.iter().any(|&j| a < j && j < b);
        self.gaps.iter().any(|&(a, b)| !logged(a, b))
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
    // Changed between the run's writes by an edit the log missed: what it
    // held then is not known, so it is kept.
    if p.unlogged_gap(foreign) {
        return kept(None);
    }
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

/// A track's placements or a lane's stretches, element by element, each
/// rule checked in turn. A list another write touched (or whose container
/// it rebuilt) between the run's first and last change to it is kept
/// whole. An element the run added: kept when another write
/// since took it away or put it there again, or rebuilt what it belongs
/// to (see [`container`]); else it goes if it is still there; else kept.
/// An element the run removed: kept when an element the run added was
/// kept; else kept when another write put it back or took it again, or
/// rebuilt what it belongs to; else kept when something there now
/// overlaps it; else it comes back.
fn decide_list(
    doc: &SongDoc,
    p: &Plan,
    others: &[usize],
    foreign: &[usize],
    records: &[ActionRecord],
) -> Decision {
    let current = target_value(doc.song(), &p.target);
    if current == p.before {
        return Decision::Already;
    }
    let last_by = foreign.last().map(|&j| records[j].source);
    let kept_by = |item: &Value, by: Option<Source>| KeptTarget {
        target: p.target.clone(),
        item: Some(item.clone()),
        reason: KeptReason::ChangedLater,
        by,
        message: None,
    };
    let kept = |item: &Value| kept_by(item, last_by);
    // The last other write that added or took away element `e` here, or
    // (`container`) rebuilt what `e` belongs to.
    let has = |v: &Value, e: &Value| v.as_array().is_some_and(|a| a.contains(e));
    let last_other = |hit: &dyn Fn(&TargetChange) -> bool| {
        let mut changes = others
            .iter()
            .rev()
            .filter(|&&j| records[j].targets.iter().any(hit));
        changes.next().map(|&j| records[j].source)
    };
    let touched = |e: &Value| {
        last_other(&|c: &TargetChange| c.target == p.target && has(&c.old, e) != has(&c.new, e))
    };
    let rebuilt = |e: &Value| last_other(&|c: &TargetChange| container(&p.target, e, &c.target));
    let before = elements(&p.before);
    let after = elements(&p.after);
    let added = minus(&after, &before);
    let removed = minus(&before, &after);
    // Changed between the run's writes by an edit the log missed: the
    // run's later write carried that edit, so what the run added is not
    // known. Kept whole.
    if p.unlogged_gap(foreign) {
        return Decision::Kept(
            added
                .iter()
                .chain(&removed)
                .map(|e| kept_by(e, None))
                .collect(),
        );
    }
    // Another write touched the list (or what it belongs to) while the
    // run was still working on it: the run's later edits started from
    // their state, which is not known element by element, so it is kept.
    let between = others.iter().copied().filter(|&j| j < p.last).find(|&j| {
        foreign.contains(&j)
            || records[j].targets.iter().any(|c| {
                let mut elements = added.iter().chain(&removed);
                elements.any(|e| container(&p.target, e, &c.target))
            })
    });
    if let Some(j) = between {
        let by = Some(records[j].source);
        return Decision::Kept(
            added
                .iter()
                .chain(&removed)
                .map(|e| kept_by(e, by))
                .collect(),
        );
    }
    // The track is gone: nothing to take out or put back into.
    let Some(mut next) = current.as_array().cloned() else {
        return Decision::Kept(added.iter().chain(&removed).map(kept).collect());
    };
    let mut items = Vec::new();
    let mut out = Vec::new();
    for a in &added {
        // Someone else took it away or put it there again, or rebuilt
        // what it belongs to: theirs now.
        if let Some(by) = touched(a).or_else(|| rebuilt(a)) {
            out.push(kept_by(a, Some(by)));
            continue;
        }
        match next.iter().position(|e| e == a) {
            Some(i) => {
                next.remove(i);
                items.push(Some(a.clone()));
            }
            None => out.push(kept(a)),
        }
    }
    // An addition changed later may be a removed element moved on by
    // someone else: putting the original back would set it beside their
    // copy, so the removals are kept too.
    let changed_later = !out.is_empty();
    for r in &removed {
        if changed_later {
            out.push(kept(r));
        } else if let Some(by) = touched(r).or_else(|| rebuilt(r)) {
            out.push(kept_by(r, Some(by)));
        } else if next.iter().any(|e| spans_overlap(e, r)) {
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

/// Whether a change to `other` rebuilds what element `e` of list `list`
/// belongs to, so that `e` would not come back to the thing it left: for a
/// placement, its track, its clip or the sections (which set what its bars
/// mean); for a lane stretch, its track, the track's instrument or effects
/// (whose devices own the param), or the sections.
fn container(list: &Target, e: &Value, other: &Target) -> bool {
    match (list, other) {
        (_, Target::Sections) => true,
        (Target::Play(t), Target::Track(u)) => t == u,
        (Target::Play(_), Target::Clip(c)) => e["clip"].as_str() == Some(c.as_str()),
        (Target::Lanes { track, .. }, Target::Track(u) | Target::Instrument(u) | Target::Fx(u)) => {
            track == u
        }
        _ => false,
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

/// One revert: a target, its value, and the items it reverts.
type Put = (Target, Value, Vec<Option<Value>>);

/// Puts each value at its target on one copy of `doc`, keeping the copy
/// when the song takes all of them. Each put must leave a valid song, so
/// a section edit's sections go last (the placements move back inside the
/// longer song first), else first (the song grows back before they move);
/// failing both, a put the song refuses waits for the others (a renamed
/// track's lane goes back once the track is there).
fn put_all(doc: &mut SongDoc, puts: &[Put]) -> Result<(), EditError> {
    let in_order = |order: Vec<&Put>| -> Result<SongDoc, EditError> {
        let mut copy = doc.clone();
        rename_scenes_back(&mut copy, &order);
        for (target, value, _) in order {
            copy.put(target, value)?;
        }
        Ok(copy)
    };
    let (sections, rest): (Vec<&Put>, Vec<&Put>) =
        puts.iter().partition(|p| p.0 == Target::Sections);
    let copy = match in_order(puts.iter().collect()) {
        Ok(copy) => copy,
        Err(e) => {
            let sections_first = (!sections.is_empty())
                .then(|| in_order(sections.into_iter().chain(rest).collect()).ok())
                .flatten();
            match sections_first.or_else(|| waiting_for_others(doc, puts)) {
                Some(copy) => copy,
                None => return Err(e),
            }
        }
    };
    *doc = copy;
    Ok(())
}

/// Puts each value on one copy of `doc`, a refused one again after the
/// others, until all are in (`Some`) or none of the rest goes in.
fn waiting_for_others(doc: &SongDoc, puts: &[Put]) -> Option<SongDoc> {
    let mut copy = doc.clone();
    let mut rest: Vec<&Put> = puts.iter().collect();
    rename_scenes_back(&mut copy, &rest);
    while !rest.is_empty() {
        let before = rest.len();
        rest.retain(|(target, value, _)| {
            let mut next = copy.clone();
            let refused = next.put(target, value).is_err();
            if !refused {
                copy = next;
            }
            refused
        });
        if rest.len() == before {
            return None;
        }
    }
    Some(copy)
}

/// Renames back each scene `puts` take away whose place and slots one they
/// bring back has: a scene rename the run made, undone in place so its
/// table keeps its comments (a put writes a fresh table). The song is the
/// one the two puts would give; they then change nothing.
fn rename_scenes_back(doc: &mut SongDoc, puts: &[&Put]) {
    for (target, value, _) in puts {
        let Target::Scene(to) = target else {
            continue;
        };
        if value.is_null() || doc.song().scenes.contains_key(to) {
            continue;
        }
        let from = puts.iter().find_map(|(other, gone, _)| match other {
            Target::Scene(from) if gone.is_null() => {
                let index = doc.song().scenes.get_index_of(from)?;
                let now = target_value(doc.song(), other);
                let same = value["index"].as_u64() == u64::try_from(index).ok()
                    && without_index(&now) == without_index(value);
                same.then(|| from.clone())
            }
            _ => None,
        });
        if let Some(from) = from {
            let mut next = doc.clone();
            if next.rename_scene(&from, to).is_ok() {
                *doc = next;
            }
        }
    }
}

fn reverted_items(puts: Vec<Put>) -> Vec<TargetItem> {
    puts.into_iter()
        .flat_map(|(target, _, items)| {
            items.into_iter().map(move |item| TargetItem {
                target: target.clone(),
                item,
            })
        })
        .collect()
}

/// Whether `x` and `y` are the same value of the planned target: a track
/// or a scene regardless of its position (the scene order is a target of
/// its own); an effect chain by its devices' ids and modules in order,
/// plus the whole of each device the run added (the others' params are
/// targets of their own); anything else exactly.
fn approx(p: &Plan, x: &Value, y: &Value) -> bool {
    match &p.target {
        Target::Track(_) | Target::Scene(_) => without_index(x) == without_index(y),
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
/// target with its old and new value. The scene order (`scenes`) holds
/// which scenes there are and their order, not their slots: it overlaps
/// itself and every `scenes.<s>`, not the slot leaves under them.
fn overlap(a: (&Target, &Value, &Value), b: (&Target, &Value, &Value)) -> bool {
    let order_and_slot = |x: &Target, y: &Target| {
        matches!(
            (x, y),
            (
                Target::SceneOrder,
                Target::Value(SongPath::SceneSlot { .. })
            )
        )
    };
    if order_and_slot(a.0, b.0) || order_and_slot(b.0, a.0) {
        return false;
    }
    let (x, y) = (a.0.to_string(), b.0.to_string());
    x == y || under(&x, &y) || under(&y, &x) || holds(a, b.0) || holds(b, a.0)
}

/// Whether `part` names something inside `whole` (`tracks.acid.gain_db`
/// inside `tracks.acid`).
fn under(part: &str, whole: &str) -> bool {
    part.strip_prefix(whole).is_some_and(|r| r.starts_with('.'))
}

/// The holdings the target strings do not show: a track holds its lanes
/// and its slot in every scene, an instrument or chain the params of its
/// devices (in either value).
fn holds((whole, old, new): (&Target, &Value, &Value), part: &Target) -> bool {
    match (whole, part) {
        (
            Target::Track(t),
            Target::Lanes { track, .. } | Target::Value(SongPath::SceneSlot { track, .. }),
        ) => t == track,
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
/// settings), a new instrument (the same), a clip (its leaves) or a scene
/// (its slots).
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
    if let Target::Value(SongPath::SceneSlot { scene, .. }) = target {
        return find(Target::Scene(scene.clone()), &new);
    }
    None
}

/// Folds a change to a part of `item` into the item's value `after`. A
/// part it cannot place is left out, so the item reads as changed later
/// and is kept.
fn fold(item: &Target, after: &mut Value, c: &TargetChange) {
    let new = c.new.clone();
    // A slot is a key of the scene's `slots`: an emptied one has none.
    if let (Target::Scene(_), Target::Value(SongPath::SceneSlot { track, .. })) = (item, &c.target)
    {
        if let Some(slots) = after.get_mut("slots").and_then(Value::as_object_mut) {
            match new {
                Value::Null => slots.remove(track),
                clip => slots.insert(track.clone(), clip),
            };
        }
        return;
    }
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
