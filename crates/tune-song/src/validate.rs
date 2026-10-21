//! Second pass: the raw mirror to the [`Song`] model, checking every rule
//! and recording every violation (nothing stops at the first one).

use std::ops::RangeInclusive;

use indexmap::{IndexMap, IndexSet};
use tune_dsp::acid303::{AcidPattern, PatternError};
use tune_dsp::{
    DIST_PARAMS, DistortionMode, DrumPattern, DrumPatternError, EQ_PARAMS, ParamSpec,
    STRIP_DUCK_DEPTH, STRIP_PARAMS, find_spec,
};

use crate::error::Errors;
use crate::model::{
    FxSpec, InstrumentKind, Lane, MIX_TRACK_ID, MasterMode, Mix, MixUnit, Pattern, PatternSteps,
    Section, Song, Track,
};
use crate::raw::{At, RawFx, RawLane, RawMix, RawPattern, RawSection, RawSong, RawTrack};

const BPM: RangeInclusive<f64> = 40.0..=300.0;
const BARS: RangeInclusive<i64> = 1..=1024;
const SAMPLE_RATE: RangeInclusive<i64> = 8_000..=384_000;
const GAIN_DB: RangeInclusive<f64> = -60.0..=12.0;
const PAN: RangeInclusive<f64> = -1.0..=1.0;
const MAX_SECONDS: f64 = 30.0 * 60.0;

/// Builds the song, or returns `None` when anything was recorded in `errors`.
pub(crate) fn validate(raw: RawSong, errors: &mut Errors) -> Option<Song> {
    let header = raw.song.as_ref();
    let name = header
        .and_then(|h| h.name.as_ref())
        .map(|n| n.value.clone());
    let bpm = header.and_then(|h| h.bpm.as_ref()).and_then(|bpm| {
        let ok = BPM.contains(&bpm.value);
        if !ok {
            errors.at(
                &bpm.span,
                format!("bpm must be 40 to 300, got {}", bpm.value),
            );
        }
        ok.then_some(bpm.value)
    });
    let seed = header.and_then(|h| h.seed.as_ref()).map_or(0, |s| s.value);
    let sample_rate = header.and_then(|h| h.sample_rate.as_ref()).and_then(|sr| {
        let ok = SAMPLE_RATE.contains(&sr.value);
        if !ok {
            errors.at(
                &sr.span,
                format!("sample_rate must be 8000 to 384000, got {}", sr.value),
            );
        }
        ok.then(|| u32::try_from(sr.value).expect("checked range"))
    });

    // Every track the file declares, valid or not, with what is known of
    // its shape: a reference to a declared but broken track is not reported
    // again as unknown, and its patterns and lanes are still checked as far
    // as its instrument and fx types allow.
    let shapes: IndexMap<&str, TrackShape> = raw
        .tracks
        .iter()
        .map(|(id, t)| (id.value.as_str(), TrackShape::of(t)))
        .collect();
    let pattern_owner: IndexMap<&str, Option<&str>> = raw
        .patterns
        .iter()
        .map(|(id, p)| {
            (
                id.value.as_str(),
                p.track.as_ref().map(|t| t.value.as_str()),
            )
        })
        .collect();

    let mix = mix(raw.mix.as_ref(), &shapes, errors);

    let mut tracks = IndexMap::new();
    for (id, t) in &raw.tracks {
        check_id(id, "track", errors);
        if let Some(track) = track(id, t, errors) {
            tracks.insert(id.value.clone(), track);
        }
    }
    let duck = Duck::of(&raw, &shapes);
    check_duck_depths(&raw, &duck, errors);

    let mut patterns = IndexMap::new();
    for (id, p) in &raw.patterns {
        check_id(id, "pattern", errors);
        if let Some(pattern) = pattern(id, p, &shapes, errors) {
            patterns.insert(id.value.clone(), pattern);
        }
    }

    let mut sections = Vec::new();
    match &raw.sections {
        Some(list) if !list.is_empty() => {
            for (i, s) in list.iter().enumerate() {
                if let Some(sec) = section(i, s, &shapes, &pattern_owner, &duck, errors) {
                    sections.push(sec);
                }
            }
        }
        _ => errors.global("a song needs at least one section ([[sections]])"),
    }

    // Counted over every section whose bars are valid, so the length check
    // still runs when another part of a section is broken.
    let total_bars: u64 = raw
        .sections
        .iter()
        .flatten()
        .filter_map(|s| s.bars.as_ref())
        .filter(|b| BARS.contains(&b.value))
        .map(|b| b.value.unsigned_abs())
        .sum();
    if let Some(bpm) = bpm {
        // 4 beats per bar.
        let seconds = total_bars as f64 * 4.0 * 60.0 / bpm;
        if seconds > MAX_SECONDS {
            errors.global(format!(
                "the song is longer than 30 minutes: {total_bars} bars at {bpm} bpm last {:.1} minutes",
                seconds / 60.0
            ));
        }
    }

    if !errors.is_empty() {
        return None;
    }
    Some(Song {
        name: name?,
        bpm: bpm?,
        seed,
        sample_rate,
        tracks,
        patterns,
        sections,
        mix: mix?,
    })
}

/// Checks the `[mix]` table; a song without one gets [`Mix::default`].
fn mix(
    raw: Option<&RawMix>,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<Mix> {
    let Some(raw) = raw else {
        return Some(Mix::default());
    };
    let master = match &raw.master {
        None => Some(MasterMode::default()),
        Some(m) => {
            let mode = MasterMode::from_name(&m.value);
            if mode.is_none() {
                let expected: Vec<&str> = MasterMode::ALL.iter().map(|m| m.name()).collect();
                errors.at(
                    &m.span,
                    format!(
                        "unknown master mode `{}` (expected {})",
                        m.value,
                        expected.join(" or ")
                    ),
                );
            }
            mode
        }
    };
    let duck_source = match &raw.duck_source {
        None => Some(None),
        Some(src) => match shapes.get(src.value.as_str()) {
            None => {
                errors.at(&src.span, unknown_track(&src.value));
                None
            }
            // An unknown instrument already has its own error.
            Some(shape) if shape.kind.is_some_and(|k| !k.is_drum()) => {
                errors.at(
                    &src.span,
                    format!(
                        "duck_source `{}` must be a drum track (ducking follows its hits)",
                        src.value
                    ),
                );
                None
            }
            Some(_) => Some(Some(src.value.clone())),
        },
    };
    let before = errors.len();
    let tables: Vec<&'static [ParamSpec]> = MixUnit::ALL.iter().map(|u| u.params()).collect();
    let params = param_values(&raw.params, &tables, MIX_TRACK_ID, errors);
    (errors.len() == before).then_some(())?;
    Some(Mix {
        master: master?,
        duck_source: duck_source?,
        params,
    })
}

/// Who `duck.depth` follows, as far as the raw file tells.
enum Duck<'a> {
    /// `[mix] duck_source` is set: the track when it names a drum track,
    /// `None` when it is invalid (it has its own error, and nothing else is
    /// derived from it).
    Explicit(Option<&'a str>),
    /// The first kick track, if any.
    FirstKick(Option<&'a str>),
}

impl<'a> Duck<'a> {
    fn of(raw: &'a RawSong, shapes: &IndexMap<&'a str, TrackShape>) -> Self {
        match raw.mix.as_ref().and_then(|m| m.duck_source.as_ref()) {
            Some(src) => Self::Explicit(
                shapes
                    .get(src.value.as_str())
                    .and_then(|s| s.kind)
                    .is_some_and(InstrumentKind::is_drum)
                    .then_some(src.value.as_str()),
            ),
            None => Self::FirstKick(
                shapes
                    .iter()
                    .find(|(_, s)| s.kind == Some(InstrumentKind::Kick))
                    .map(|(id, _)| *id),
            ),
        }
    }

    /// The track that ducks the others, when there is one.
    fn source(&self) -> Option<&'a str> {
        match self {
            Self::Explicit(source) | Self::FirstKick(source) => *source,
        }
    }

    /// No explicit source and no kick: nothing for `duck.depth` to follow.
    fn missing(&self) -> bool {
        matches!(self, Self::FirstKick(None))
    }

    /// The error for `duck.depth` > 0 (or a lane on it) on `track`, if any.
    fn depth_error(&self, track: &str) -> Option<String> {
        if self.missing() {
            Some(NO_DUCK_SOURCE.to_owned())
        } else if self.source() == Some(track) {
            Some(format!(
                "track `{track}` is the duck source and cannot duck itself"
            ))
        } else {
            None
        }
    }
}

const NO_DUCK_SOURCE: &str =
    "`duck.depth` needs a kick to follow: add a kick track or set duck_source in [mix]";

/// Every explicit `duck.depth` above 0 needs a source, and the source may
/// not duck itself. Lanes on `duck.depth` follow the same rules (see [`lane`]).
fn check_duck_depths(raw: &RawSong, duck: &Duck<'_>, errors: &mut Errors) {
    let depth = &STRIP_PARAMS[STRIP_DUCK_DEPTH];
    for (id, t) in &raw.tracks {
        for (param, v) in &t.params {
            // An out-of-range depth already has its own error.
            if param.value != depth.id || v.value <= 0.0 || depth.check(v.value).is_err() {
                continue;
            }
            if let Some(message) = duck.depth_error(&id.value) {
                errors.at(&v.span, message);
            }
        }
    }
}

/// Track and pattern ids match `[a-z0-9_]+`.
fn check_id(id: &At<String>, what: &str, errors: &mut Errors) {
    let ok = !id.value.is_empty()
        && id
            .value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        errors.at(
            &id.span,
            format!("{what} id `{}` may only contain a-z, 0-9 and _", id.value),
        );
    } else if what == "track" && id.value == MIX_TRACK_ID {
        // Stems are written as `<id>.wav`; `mix.wav` is the master.
        errors.at(
            &id.span,
            "track id `mix` is reserved (`mix.wav` is the master render)".to_owned(),
        );
    }
}

fn track(id: &At<String>, t: &RawTrack, errors: &mut Errors) -> Option<Track> {
    let instrument = t.instrument.as_ref().and_then(|name| {
        let kind = InstrumentKind::from_name(&name.value);
        if kind.is_none() {
            let expected: Vec<&str> = InstrumentKind::ALL.iter().map(|k| k.name()).collect();
            errors.at(
                &name.span,
                format!(
                    "unknown instrument `{}` (expected one of {})",
                    name.value,
                    expected.join(", ")
                ),
            );
        }
        kind
    });

    let params = instrument.map(|kind| {
        param_values(
            &t.params,
            &[kind.params(), STRIP_PARAMS],
            kind.name(),
            errors,
        )
    });

    let gain_db = ranged(
        t.gain_db.as_ref(),
        0.0,
        &GAIN_DB,
        "gain_db must be -60 to +12 dB",
        errors,
    );
    let pan = ranged(t.pan.as_ref(), 0.0, &PAN, "pan must be -1 to +1", errors);

    let mut fx = Vec::new();
    let mut seen_types: IndexSet<&str> = IndexSet::new();
    let mut fx_ok = true;
    for raw in &t.fx {
        if let Some(kind) = &raw.kind
            && FxSpec::TYPE_NAMES.contains(&kind.value.as_str())
            && !seen_types.insert(kind.value.as_str())
        {
            errors.at(
                &raw.span,
                format!(
                    "track `{}` may have only one {} (merge the entries)",
                    id.value, kind.value
                ),
            );
            fx_ok = false;
        }
        match fx_spec(raw, errors) {
            Some(spec) => fx.push(spec),
            None => fx_ok = false,
        }
    }

    Some(Track {
        instrument: instrument?,
        params: params?,
        gain_db: gain_db?,
        pan: pan?,
        mute: t.mute.as_ref().is_some_and(|m| m.value),
        solo: t.solo.as_ref().is_some_and(|s| s.value),
        fx: fx_ok.then_some(fx)?,
    })
}

/// `value` (or `default` when absent) when it lies in `range`.
fn ranged(
    value: Option<&At<f64>>,
    default: f64,
    range: &RangeInclusive<f64>,
    rule: &str,
    errors: &mut Errors,
) -> Option<f64> {
    let Some(v) = value else {
        return Some(default);
    };
    if range.contains(&v.value) {
        Some(v.value)
    } else {
        errors.at(&v.span, format!("{rule}, got {}", v.value));
        None
    }
}

/// Checks `{ "<param id>" = plain }` against `tables` (the first one that
/// has the id wins); `owner` names them in messages ("unknown parameter `x`
/// for kick").
fn param_values(
    values: &[(At<String>, At<f64>)],
    tables: &[&'static [ParamSpec]],
    owner: &str,
    errors: &mut Errors,
) -> IndexMap<String, f64> {
    let mut out = IndexMap::new();
    for (id, v) in values {
        match find_in(tables, &id.value) {
            None => errors.at(&id.span, unknown_param(&id.value, owner, tables)),
            Some(spec) => match spec.check(v.value) {
                Err(e) => errors.at(&v.span, e.to_string()),
                Ok(()) => {
                    out.insert(id.value.clone(), v.value);
                }
            },
        }
    }
    out
}

/// The spec with `id` in the first of `tables` that has it.
fn find_in(tables: &[&'static [ParamSpec]], id: &str) -> Option<&'static ParamSpec> {
    tables
        .iter()
        .find_map(|table| find_spec(table, id).ok().map(|(_, spec)| spec))
}

fn unknown_param(id: &str, owner: &str, tables: &[&[ParamSpec]]) -> String {
    let known: Vec<&str> = tables.iter().flat_map(|t| t.iter()).map(|s| s.id).collect();
    format!(
        "unknown parameter `{id}` for {owner} (known: {})",
        known.join(", ")
    )
}

fn fx_spec(raw: &RawFx, errors: &mut Errors) -> Option<FxSpec> {
    let kind = raw.kind.as_ref()?;
    match kind.value.as_str() {
        "distortion" => {
            let mode = match &raw.mode {
                None => Some(DistortionMode::Soft),
                Some(m) => {
                    let mode = DistortionMode::from_name(&m.value);
                    if mode.is_none() {
                        errors.at(
                            &m.span,
                            format!(
                                "unknown distortion mode `{}` (expected soft, hard or overdrive)",
                                m.value
                            ),
                        );
                    }
                    mode
                }
            };
            let before = errors.len();
            let params = param_values(&raw.params, &[DIST_PARAMS], "distortion", errors);
            (errors.len() == before).then_some(())?;
            Some(FxSpec::Distortion {
                mode: mode?,
                params,
            })
        }
        "eq" => {
            let before = errors.len();
            if let Some(m) = &raw.mode {
                errors.at(&m.span, "eq has no mode");
            }
            let params = param_values(&raw.params, &[EQ_PARAMS], "eq", errors);
            (errors.len() == before).then_some(())?;
            Some(FxSpec::Eq { params })
        }
        other => {
            errors.at(
                &kind.span,
                format!(
                    "unknown fx type `{other}` (expected one of {})",
                    FxSpec::TYPE_NAMES.join(", ")
                ),
            );
            None
        }
    }
}

fn pattern(
    id: &At<String>,
    p: &RawPattern,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<Pattern> {
    let track = p.track.as_ref()?;
    let Some(shape) = shapes.get(track.value.as_str()) else {
        errors.at(&track.span, unknown_track(&track.value));
        return None;
    };
    let steps = p.steps.as_ref()?;
    // An unknown instrument already has its own error.
    let kind = shape.kind?;
    let parsed = if kind.is_drum() {
        DrumPattern::parse(&steps.value)
            .map(PatternSteps::Drum)
            .map_err(|e| match e {
                DrumPatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            })
    } else {
        AcidPattern::parse(&steps.value)
            .map(PatternSteps::Acid)
            .map_err(|e| match e {
                PatternError::Length(n) => Err(n),
                other => Ok(other.to_string()),
            })
    };
    match parsed {
        Ok(steps) => Some(Pattern {
            track: track.value.clone(),
            steps,
        }),
        Err(Err(n)) => {
            errors.at(
                &steps.span,
                format!("pattern `{}` needs 1 to 64 steps, got {n}", id.value),
            );
            None
        }
        Err(Ok(why)) => {
            errors.at(
                &steps.span,
                format!("pattern `{}` has a bad step: {why}", id.value),
            );
            None
        }
    }
}

fn unknown_track(id: &str) -> String {
    format!("unknown track `{id}`")
}

fn section(
    index: usize,
    s: &RawSection,
    shapes: &IndexMap<&str, TrackShape>,
    pattern_owner: &IndexMap<&str, Option<&str>>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<Section> {
    let label = match &s.name {
        Some(n) => format!("section `{}`", n.value),
        None => format!("section {}", index + 1),
    };
    let bars = s.bars.as_ref().and_then(|b| {
        let ok = BARS.contains(&b.value);
        if !ok {
            errors.at(&b.span, format!("bars must be 1 to 1024, got {}", b.value));
        }
        ok.then(|| u32::try_from(b.value).expect("checked range"))
    });

    let before = errors.len();
    let mut play = IndexMap::new();
    for (track, pat) in &s.play {
        if !shapes.contains_key(track.value.as_str()) {
            errors.at(&track.span, unknown_track(&track.value));
            continue;
        }
        match pattern_owner.get(pat.value.as_str()) {
            None => errors.at(&pat.span, format!("unknown pattern `{}`", pat.value)),
            Some(Some(owner)) if *owner != track.value => errors.at(
                &pat.span,
                format!(
                    "pattern `{}` belongs to track `{owner}`, not `{}`",
                    pat.value, track.value
                ),
            ),
            _ => {
                play.insert(track.value.clone(), pat.value.clone());
            }
        }
    }

    let mut automation = Vec::new();
    let mut seen: IndexSet<(String, String)> = IndexSet::new();
    for raw in &s.automation {
        if let Some(lane) = lane(raw, bars, shapes, duck, errors) {
            let key = (lane.track.clone(), lane.param.clone());
            if !seen.insert(key) {
                let param = raw.param.as_ref().expect("a built lane has a param");
                errors.at(
                    &param.span,
                    format!(
                        "{label} already has a lane for `{}` (one lane per param per section)",
                        param.value
                    ),
                );
                continue;
            }
            automation.push(lane);
        }
    }

    (errors.len() == before).then_some(())?;
    Some(Section {
        name: s.name.as_ref()?.value.clone(),
        bars: bars?,
        play,
        automation,
    })
}

fn lane(
    raw: &RawLane,
    bars: Option<u32>,
    shapes: &IndexMap<&str, TrackShape>,
    duck: &Duck<'_>,
    errors: &mut Errors,
) -> Option<Lane> {
    let before = errors.len();
    let target = raw.param.as_ref().and_then(|p| resolve(p, shapes, errors));
    if let (Some((track, param)), Some(p)) = (&target, &raw.param)
        && track != MIX_TRACK_ID
        && param == STRIP_PARAMS[STRIP_DUCK_DEPTH].id
        && let Some(message) = duck.depth_error(track)
    {
        errors.at(&p.span, message);
    }

    let points = raw.points.as_ref()?;
    if points.value.is_empty() {
        errors.at(&points.span, "a lane needs at least one point");
    }
    let mut prev: Option<f64> = None;
    for point in &points.value {
        let bar = point.bar.value;
        if let Some(bars) = bars
            && !(0.0..=f64::from(bars)).contains(&bar)
        {
            errors.at(
                &point.bar.span,
                format!("bar {bar} is outside the section (0..={bars})"),
            );
        }
        if let Some(p) = prev
            && bar < p
        {
            errors.at(
                &point.bar.span,
                format!("points go backwards: bar {bar} comes after bar {p}"),
            );
        }
        prev = Some(bar);
        let value = point.value.value;
        if !(0.0..=1.0).contains(&value) {
            errors.at(
                &point.value.span,
                format!("value {value} is outside 0..=1 (lane values are normalized)"),
            );
        }
    }

    let (track, param) = target?;
    (errors.len() == before).then_some(())?;
    Some(Lane {
        track,
        param,
        points: points
            .value
            .iter()
            .map(|p| (p.bar.value, p.value.value))
            .collect(),
    })
}

/// Splits `<track id>.<param id>` at the first `.` and resolves the param
/// against the mix tables for the `mix` pseudo-track, else the instrument
/// table, the strip table, then each fx table of the track.
fn resolve(
    p: &At<String>,
    shapes: &IndexMap<&str, TrackShape>,
    errors: &mut Errors,
) -> Option<(String, String)> {
    let Some((track_id, param_id)) = p.value.split_once('.') else {
        errors.at(
            &p.span,
            format!(
                "automation param `{}` must be `<track id>.<param id>`, e.g. `acid.acid303.cutoff`",
                p.value
            ),
        );
        return None;
    };
    if track_id == MIX_TRACK_ID {
        let tables: Vec<&'static [ParamSpec]> = MixUnit::ALL.iter().map(|u| u.params()).collect();
        if find_in(&tables, param_id).is_none() {
            errors.at(&p.span, unknown_param(param_id, MIX_TRACK_ID, &tables));
            return None;
        }
        return Some((track_id.to_owned(), param_id.to_owned()));
    }
    let Some(shape) = shapes.get(track_id) else {
        errors.at(&p.span, unknown_track(track_id));
        return None;
    };
    // An unknown instrument already has its own error.
    let kind = shape.kind?;
    let mut tables = vec![kind.params(), STRIP_PARAMS];
    tables.extend(shape.fx_tables.iter().copied());
    if find_in(&tables, param_id).is_none() {
        errors.at(
            &p.span,
            unknown_param(param_id, kind.name(), &[kind.params(), STRIP_PARAMS]),
        );
        return None;
    }
    Some((track_id.to_owned(), param_id.to_owned()))
}

/// What a declared track's raw entry says about the params it has.
struct TrackShape {
    /// `None` for a missing or unknown instrument.
    kind: Option<InstrumentKind>,
    /// Param tables of its fx entries whose `type` is known.
    fx_tables: Vec<&'static [ParamSpec]>,
}

impl TrackShape {
    fn of(t: &RawTrack) -> Self {
        Self {
            kind: t
                .instrument
                .as_ref()
                .and_then(|i| InstrumentKind::from_name(&i.value)),
            fx_tables: t
                .fx
                .iter()
                .filter_map(|fx| match fx.kind.as_ref()?.value.as_str() {
                    "distortion" => Some(DIST_PARAMS),
                    "eq" => Some(EQ_PARAMS),
                    _ => None,
                })
                .collect(),
        }
    }
}
