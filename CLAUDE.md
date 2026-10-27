# tune-ai

Rust music workstation: TOML song files, 909-style drums, a faithful Rust port
of the Open303 TB-303 emulation, and a sample-accurate engine (offline render
and lock-free live playback) driven through the `tune` CLI.

## Layout

- `crates/tune-dsp` - DSP library (`param`, `acid303/*`, `drums/*`, `fx/*`: eq, delay, reverb, duck, limiter). No Tauri, UI,
  MCP, `cc` or `open303-ref` dependencies (`open303-ref` only as a dev-dependency).
- `crates/tune-modules` - instrument and effect traits (instruments receive
  note events), today's modules and the registry; depends only on tune-dsp.
- `crates/tune-song` - song file model (format v2): parse, validate
  (positional errors), canonical write, the comment-keeping migration of v1
  files to v2, section operations, file watcher. Headless.
- `crates/tune-engine` - compiles a song and renders it: offline (`render`) or
  live (`live`, lock-free swap for hot reload). Headless.
- `crates/tune-studio` - the studio core: in-place song edits by path
  (`SongPath`, comments kept), the action layer, the edit log, undo,
  external-change merge, the `Studio` actor the app talks to and its local
  control endpoint for the CLI. Headless.
- `crates/tune-cli` - the `tune` binary: `validate`, `info`, `params`,
  `render`, `play`, `render-303`, `set`, `migrate`, `changes`, `serve`, `analyze`,
  `compare`, `refs`, `actions`, `do`. `ctrlc`, `tokio` and `axum` live only
  here (and in the Tauri host).
- `crates/tune-analysis` - Claude's ears: decode, loudness (EBU R128), spectrum,
  spectrogram PNG, comparison against a reference. Headless, no unsafe.
- `crates/tune-audio` - audio outputs for the engine's `AudioOut`: `CpalOut`
  (default device) and `NullOut`. `cpal` lives only here.
- `apps/desktop` - the studio app (React + Vite in `src/`, Tauri host in
  `src-tauri/`). `tune serve` serves its built `dist/` to a browser.
  `src/client/generated.ts` is generated from `tune-studio`; regenerate with
  `UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types`.
- `packages/ui` - `@tune/ui`: the studio's components (Knob, sequencers,
  arrangement, mixer, transport) and theme tokens.
- `songs/<name>/song.toml` - committed songs. Renders go to `scratch/` or
  `songs/*/renders/` (gitignored).
- `crates/open303-ref` - dev-only C++ reference build used for parity tests.
- `third_party/open303/` - vendored Open303 sources, unmodified.
- `scripts/` - privacy check and its tests. `.githooks/` - pre-commit hook.
- `docs/` - design spec, plans, research.

## How Claude uses tune

Songs are TOML files, `songs/<name>/song.toml`. Loop:

1. Edit the song file.
2. `tune validate <song> --json`. Errors carry `line` and `column`; fix them all.
3. `tune info <song> --json` for the section, bar and time map.
4. `tune render <song> --bars A..B --json --out scratch/x.wav` for the passage
   in question. Read `master` (`clip` or `limit`), `peak_dbfs`,
   `pre_clip_peak_dbfs` and `max_reduction_db`. In clip mode (the default)
   the soft clipper starts shaping at -6 dBFS; a pre-clip peak above -1 dBFS
   means it is limiting hard, so lower gains. Text mode prints `warning:
   master clipper engaged on N samples` only in that case and `note: soft
   clipper shaped N samples` when only the -6 dBFS knee is crossed (stderr).
   That knee note/warning is clip mode only. In limit mode the text line ends
   "limiter reduced peaks by R dB" instead of the pre-clip peak,
   `max_reduction_db` is R (0 in clip mode), and stderr gets `note: limiter
   reduced peaks by up to R dB` for R > 0, or `warning: ... lower
   mix.master.gain` for R > 9.

Notation:

- Acid pattern: one token per 16th. `C2` note, `.` rest, `!` accent,
  `~` slide into the next step (same pitch ties), e.g. `C2 C2! C3~ . D#2`.
- Drum pattern: `x` hit, `X` accent, `.` rest, e.g. `x...x...x...X...`.
- Song format v2 (`version = 2` in `[song]`; a file without it is v1). Bars
  are 1-based and ranges inclusive in CLI flags.
- Clips: `[clips.<id>]` with `kind = "acid" | "drum" | "notes"`. An acid or
  drum clip has `steps`; an acid clip plays on an instrument that plays acid,
  a drum clip on a drum module.
- Note clips: `kind = "notes"` with `bars` (1 to 64) and optional `notes`; any
  instrument plays them. They are timed notes, not steps:

  ```toml
  [clips.lead]
  kind = "notes"
  bars = 1
  notes = [
    { start = 0, pitch = "C3", len = 3 },               # start and len in 16th steps
    { start = 0.333, pitch = "G3", len = 0.5 },         # 0.333 reads as a third of a step
    { start = 2.5, pitch = "D#3", len = 2, vel = 120 }, # vel 1 to 127, default 100
  ]
  ```

  Positions sit on a grid of 24 ticks per 16th step: a value within 0.001
  step of a multiple of 1/24 is that multiple, anything else (`0.3`) is an
  error. `start >= 0`, `len > 0`, and a note may not end past the clip
  (`16 * bars` steps). Keep the notes in time order (`start` never
  decreases); overlaps and chords are fine. `pitch` is a note name as in an
  acid pattern, without `!` or `~` (`C3` = MIDI 48, `Bb1`, `C-1` to `G9`).
  A note with `vel` 110 or more is accented. On acid303 a note slides into
  a later note that starts before it ends (the slide holds to that note's
  end); notes that only touch, one ending where the next starts, retrigger.
  acid303 plays one note at a time, so a chord (notes with the same
  `start`) starts on its first note and slides straight to its last. On
  a drum voice each note is a hit: its pitch and length do nothing, its
  `vel` still accents. A notes clip has no `steps`; an acid or drum clip
  has no `bars` or `notes`. The engine plays each note at
  its own sample; a note cut by a placement's end, a loop wrap or a seek
  stops there. A placement that picks up where the one before it stopped
  (the same clip, its offset lining up, as when a section operation splits
  a placement) plays on, held notes included; so does a hot reload that
  keeps the clip and the clip step. The studio shows no editor for them
  yet. `tune info` lists every clip
  (`clips: k4 drum 16 steps, lead notes 1 bar 5 notes`; `--json` has
  `clips`), and `tune params` says what each instrument plays (`plays`).
- Placements: the track's `play = [{ at, clip, bars, offset }]`. `at` is the
  1-based start bar, `offset` the bars into the clip (default 0); `at`,
  `bars` and `offset` are whole numbers. Keep them in time order and never
  overlapping. Two placements whose offsets differ by a clip length play the
  same but count as a change.
- Scenes: `[scenes.<id>]` tables after the clips, in file order (the row
  order), each with an optional `slots = { <track id> = "<clip id>" }`; a
  track without a key has an empty slot, and `[scenes.empty]` is an empty
  scene. The scene id follows the track and clip id rules (`a-z`, `0-9`, `_`).
  A slot must name a track of the song and a clip that track's instrument can
  play (the placement rule: an acid clip on a drum track is an error). Scenes
  are scenes of live play only: renders play the arrangement and ignore them.
  They are part of format v2, and a v1 file with `[scenes.*]` gets the
  "version 2" hint. `tune info` lists them
  (`scenes: groove (kick k4, acid a1), break (hat h)`; `--json` has `scenes`).
- Sections: `[[sections]]` with `name` and `bars` (1 to 1024), laid end to end
  from bar 1. They set the song length and hold no content.
- Devices: a track's `instrument` (a module name, or a table with `type` and
  optional `id`) and `fx` entries (`type`, optional `id`, settings such as
  `mode`, `params`). The device id defaults to the module's prefix; a second
  instance of a module needs an explicit `id`, e.g. `id = "eq2"`. Params are
  keyed `<device id>.<param>`, e.g. `"eq2.low_cut"`.
- Automation: `[[automation]]` with `param = "<track>.<device>.<param>"` (or
  `<track>.send.delay`, `mix.<param id>`), `at`, `bars` and
  `points = [[bar_offset, normalized_0_to_1], ...]`; offsets are relative to
  `at`. Outside every stretch of a param, its static value plays.
- `tune migrate <song>` rewrites a v1 file as v2, keeping comments.
  `tune set` also writes a v1 file back as v2 when it changes a value, and
  `tune serve` and the studio rewrite it as v2 on open (logged `external`,
  one undo step). Every other command reads a v1 file in memory and prints
  a `note:`. Migrating a CRLF file produces LF line endings.
- `tune params --json` is the source of param ids, ranges and units.

### Mixing

Signal flow: instrument -> fx chain (any effects, in order) -> duck ->
gain/pan -> sends -> delay and reverb buses -> master gain -> clip or limit.

- Strip params live in the track's `params` table: `send.delay`, `send.reverb`
  (dB, -60 = off), `duck.depth` (dB), `duck.release` (ms). EQ is an fx entry:
  `fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }]`
  (`eq.{low_cut,mid_freq,mid_gain,mid_q,high_gain,high_cut}`). A second EQ
  needs its own id and prefixes its params with it:
  `fx = [{ type = "eq", params = { "eq.low_cut" = 85 } }, { type = "eq", id = "eq2", params = { "eq2.low_cut" = 120 } }]`.
- `[mix]` holds the master mode, the duck source and the shared bus params
  (`delay.*`, `reverb.*`, `master.*`):

  ```toml
  [mix]
  master = "limit"            # "clip" (default, soft clipper) or "limit"
  duck_source = "kick"        # optional; default = the first kick track
  params = { "delay.feedback" = 40, "reverb.size" = 30, "master.gain" = 6 }
  ```

- Lane params: `<track>.<device>.<param>`, `<track>.send.delay` (delay
  throws), `<track>.duck.depth`, `mix.<param id>` (e.g.
  `param = "mix.reverb.size"`).
- A delay/reverb bus is built only if some track sets `send.delay` /
  `send.reverb` explicitly (even at -60) or a lane drives it. Ducking follows
  the source's hits even if it is muted; no hits, no ducking.
- `master = "limit"` is for loud masters; `master.ceiling` defaults to -1.5 dB.
  Its 84-frame lookahead (1.75 ms at 48 kHz) is not compensated.
- A studio send knob on a track with no static send and no lane is silent
  until released. Stems carry dry tracks only (no buses, no limiter); in limit
  mode they run 84 frames (the lookahead) ahead of the mix.
- A hot reload keeps the reverb tail and the limiter. Only a bpm change or a
  delay bus appearing or going restarts the delay (its echoes stop). A
  master-mode switch crossfades clipper and limiter over 10 ms, with no silent
  frames. A swap that adds, removes, reorders or rebuilds a device, swaps the
  instrument, or adds or removes a track crossfades that track over 10 ms;
  value, step and clip edits switch at the step.

Every command takes `--json`; exit code 2 means failure. `tune play` is for the
producer: it plays through the speakers and reloads the file when it changes.
Claude may run `tune play <song> --device null --json` to check a reload (no
sound), never the default device. Never commit renders or any audio.

### Listening (Claude cannot hear)

`tune analyze` and `tune compare` turn a render into numbers and a picture.
They are hints about the mix, not verdicts on the music: the producer's ears
are the final judge.

1. `tune info <song> --json` for the section map (bars are 1-based).
2. `tune analyze <song> --sections --png --json` for the whole song, or
   `--bars A..B` for a passage. Put INPUT before `--png` (or write
   `--png=PATH`): a bare `--png` takes an optional value and would swallow a
   following path. Bare `--png` writes `scratch/<name>[-bars-A-B].png`.
3. Read the PNG with the image tool (1600x856). Bright = loud. x is time, with
   1-based bar numbers in the ruler and white section lines. y is 30 Hz to
   16 kHz, log scale. The strip below shows each band's share of the total
   over time.
4. Read the JSON. Per-section `integrated_lufs` and `delta_lu` are the energy
   curve: a drop should be louder and brighter than its breakdown. A
   `nothing changes here` entry in a section's `notes` is a problem.
5. `tune compare <song> --bars <drop> --ref <name> [--ref-seconds A..B] --png --json`.
   Find the reference's drop first with `tune analyze --ref <name> --png`
   (never look up its path), then compare our drop with it. Give exactly one of `--ref <name>` or a
   reference path (`tune compare <song> <path>`). The compare PNG is 1600x1712:
   ours on top, the reference below, same frequency axis and colour scale.
6. For -8..-6 LUFS, set `master = "limit"` and raise `mix.master.gain`; keep
   `max_reduction_db` under about 9.
7. Change the song (`tune set`), re-render, repeat.

Input is a song file (rendered in memory) or audio (WAV, MP3, FLAC, AAC; at
most 2 channels). `--song <song> --from-bar N` maps an audio file onto a song's
bars; audio past the song's last bar is left out with an entry in `notes`
(`note:` on stderr in text mode), and a section cut by the file's end is
`partial`. Failures exit 2 with a message.

| Key | Meaning |
| --- | --- |
| `loudness.integrated_lufs` | whole-passage loudness; acid target -8 to -6 |
| `loudness.true_peak_dbtp` | at most -1; above risks distortion after encoding |
| `loudness.lra_lu` | loudness range; `null` on passages under about 3 s |
| `loudness.crest_db`, `plr_db` | peak vs RMS / vs loudness; lower = more squashed |
| `targets` | the acid targets and whether `integrated` / `true_peak` meet them |
| `bands.{sub,low,low_mid,mid,high,air}` | dB vs the total; independent of loudness |
| `low_end` | `kick_region_db` (20-90 Hz) vs `bass_region_db` (90-250 Hz), `ratio_db` = kick - bass |
| `stereo.low_correlation` | L/R correlation below 120 Hz; 0.9 or more = mono low end |
| `stereo.side_to_mid_db` | stereo width |
| `brightness.*` | whole-mix spectral centroid: `median_hz`, `p10_hz`, `p90_hz`, `profile_hz`; `range_oct` = log2(p90 / p10), how far the centroid moves. It tracks a filter sweep only where the 303 dominates the mix; under a loud kick it barely moves (0.17 oct across a 400-960 Hz cutoff ramp in the first session) |
| `series` | 1 s grid: `short_term_lufs`, `centroid_hz` |
| `sections[]` | `integrated_lufs`, `short_term_max_lufs`, `delta_lu`, `bands`, `centroid_median_hz`, `notes` |
| `deltas`, `verdicts` (compare) | ours minus reference, plus one plain-language line each |

A quantity that does not exist (silence, too short) is `null`; a mono
signal's `side_to_mid_db` floors at -200. In `compare`, a band delta is `null`
with a "not compared" verdict when one side lacks content there, and the width
delta is `null` when exactly one side is mono, with a verdict naming that side
(0 when both are). A side 60 dB or more below the mid counts as mono: a mono
master in a lossy stereo file keeps a faint side residue, so its
`side_to_mid_db` reads about -90, not -200. A side with no mid at all (left
and right in anti-phase, +200) is not compared either.

References live outside git. Put `[refs]` `<name> = "~/path/file.mp3"` in
`tune.local.toml` at the repo root (gitignored) or drop files into `refs/`
(name = file stem); `tune refs --json` checks that each resolves. It echoes
the configured paths and also the expanded `resolved` path and repo root, so
keep its output out of reports and anything committed. With `--ref <name>`,
`tune compare` shows only the configured path (`~/...` or `refs/<file>`) in
`source.path` and in error messages, never the expanded one; a reference given
as a path is echoed as typed, so prefer `--ref <name>`. Never read
or print `tune.local.toml` itself. Never commit references, PNGs or the local
config.

### Working with the studio open

The producer may have the studio (app or browser) open on the same song.

- Start each turn with `tune changes <song> --since <until> --json`, passing
  the `until` from the last call (omit `--since` the first time). It lists the
  producer's knob moves and step edits (`human`), Claude's (`claude`) and
  anything else (`external`), oldest first. `--source human` narrows it.
- `tune do <song> <action> [name=value]... --json` runs any action from
  `tune actions` (`tune do <song> set_value path=song.bpm value=130`); with
  `--batch` it reads a JSON array of `{"cmd", "args"}` objects from stdin and
  applies them all or nothing (song edits only; `play`, `undo` and the other
  live or history actions need the studio open and run on their own).
  With the studio open on the song (the app or `tune serve`), `tune do` and
  `tune set` go through it: the change shows up at once, is written before
  the command returns and is one undo step per command (the producer's
  ⌘Z steps back one command at a time), and the JSON says `"via":
  "studio"` (`"file"` without a studio). Live and history actions need it.
  A studio that does not answer the first probe (it asks which song is
  open) is skipped and the file is written; once the request is sent, no
  answer makes the command fail rather than write the file behind its
  back, so check the studio before retrying. The studio's
  endpoint files live under `~/.tune/studios` (`TUNE_STUDIO_DIR` moves
  them and must be an absolute path, else `do` and `set` refuse; a
  directory with any group or other permission bit is not trusted, as if
  no studio ran); they hold a secret, so never read or print them.
  Tag the edits of one piece of work with a run: `--run ID --run-title TEXT`
  (or `TUNE_RUN`, `TUNE_RUN_TITLE`; the flags win). `tune changes` shows
  `(run ID)` on those entries. `tune set` takes the same flags and is a
  shorthand for `set_value`.
- Edit with `tune set <song> path=value... --json` while the app is open: the
  edits are all-or-nothing, keep the file's comments, and are tagged `claude`,
  so the app rings them in cyan with an Undo. Plain file edits still work and
  show up as `external`. The value is TOML (`820`, `true`, `"hard"`); a bare
  word is a string, and an empty value clears (`sections[1].play.acid=`).

  | Path | Value |
  | --- | --- |
  | `song.bpm` | number |
  | `tracks.<t>.gain_db` / `pan` / `mute` / `solo` | dB / -1 to 1 / bool / bool |
  | `tracks.<t>.<device>.<param>` | plain value in the param's unit, e.g. `tracks.acid.acid303.cutoff=820`, `tracks.acid.dist.drive=18`, `tracks.acid.eq2.low_cut=200` |
  | `tracks.<t>.<device>.mode` | a device setting, e.g. `tracks.acid.dist.mode`: `soft`, `hard` or `overdrive` |
  | `tracks.<t>.send.delay` / `send.reverb` / `duck.depth` / `duck.release` | dB / dB / dB / ms |
  | `tracks.<t>.eq.<name>` | `low_cut`, `mid_freq`, `mid_gain`, `mid_q`, `high_gain`, `high_cut`; the track needs an `eq` fx entry (a second one: `eq2.<name>`) |
  | `mix.<param id>` | `delay.*`, `reverb.*`, `master.gain` / `ceiling` / `release`, e.g. `mix.master.gain=6` |
  | `mix.master` | `clip` or `limit` |
  | `mix.duck_source` | track id; empty = first kick |
  | `clips.<c>.steps` | the clip in notation, quoted (`patterns.<c>.steps` is still accepted) |
  | `clips.<c>.notes` | a notes clip's notes as a TOML array of `{ start, pitch, len, vel }`, quoted, e.g. `clips.lead.notes='[{ start = 0, pitch = "C3", len = 2 }]'`; rewrites the whole array and drops the comments inside it (those around it stay) |
  | `clips.<c>.bars` | a notes clip's length in bars, 1 to 64 |
  | `sections[<i>].name` | string; `<i>` is 0-based |
  | `sections[<i>].bars` | integer, 1 to 1024; resizes the section: placements and stretches that ran to its end run to the new end (grow) or are cut at it (shrink), later ones move |
  | `sections[<i>].play.<t>` | clip id: replaces what the track plays inside the section with one placement covering it; empty removes them |
  | `scenes.<s>.slots.<t>` | clip id: puts the clip in the scene's slot for track `t` (a clip the track's instrument can play); empty clears the slot |
  | `tracks.<t>`, `sections`, `sections[<i>].lanes`, `scenes`, `scenes.<s>`, `song.version` | read-only (appear in `tune changes`) |

  Section operations (`play`, `bars`) rewrite a track's `play` list and drop
  the comments inside it.

- Actions (`tune actions` lists them all; each is `tune do <song> <name> name=value...`).
  The track and device ones, each refused with a plain message when the id is
  taken, malformed or unknown, and the file left as it was (so are the
  effect ones on a chain written as `[[tracks.<t>.fx]]` tables, and
  `add_track` on a root `tracks = { … }` table: edit those forms by hand):

  | Action | Example |
  | --- | --- |
  | `add_track` | `tune do <song> add_track track=bass instrument=acid303` appends a default track (gain and pan 0, no effects or placements) |
  | `remove_track` | `tune do <song> remove_track track=bass` removes the track, its placements, its lanes and its scene slots (the result lists the lanes and the slots) |
  | `rename_track` | `tune do <song> rename_track track=bass to=lead`; lanes, scene slots and `mix.duck_source` follow, but its noise changes (seeded from the track id) |
  | `add_effect` | `tune do <song> add_effect track=acid module=eq index=0` returns the new device's id (`eq`, then `eq2`); `id=` names it |
  | `remove_effect` | `tune do <song> remove_effect track=acid device=eq2` removes the effect and its lanes |
  | `move_effect` | `tune do <song> move_effect track=acid device=dist index=0` |
  | `rename_device` | `tune do <song> rename_device track=acid device=acid303 to=bass303` renames the instrument or an effect, its param keys and lanes |
  | `set_instrument` | `tune do <song> set_instrument track=acid module=kick` swaps the instrument; the new device is `kick` with default settings, so the old instrument's params, lane stretches and placements and scene slots of clips the new module cannot play go (the result lists them; the clips stay; a notes clip's stay) and so does a `mix.duck_source` naming the track when the new module plays no drum clips; strip params stay. Dropping placements rewrites the `play` list, which drops the comments inside it, and a swap the song would not take (the only kick, while a track ducks to it) is refused with the validator's message |

  The clip ones, refused the same way:

  | Action | Example |
  | --- | --- |
  | `add_clip` | `tune do <song> add_clip clip=lead kind=notes bars=2 notes='[{ start = 0, pitch = "C3", len = 2 }]'`; `kind` is `acid`, `drum` or `notes`, `steps` only for acid and drum (16 rests by default), `bars` and `notes` only for notes; goes after the last clip |
  | `remove_clip` | `tune do <song> remove_clip clip=k4` removes the clip, its placements and the scene slots holding it (the result lists them) |
  | `rename_clip` | `tune do <song> rename_clip clip=a1 to=bass1`; keeps its place, placements and scene slots follow |
  | `duplicate_clip` | `tune do <song> duplicate_clip clip=h to=h2` copies it right after the original, without its comments |

  The placement and section ones, refused the same way (`at` and `to` are 1-based
  bars, and a placement is named by the bar it starts at):

  | Action | Example |
  | --- | --- |
  | `add_placement` | `tune do <song> add_placement track=acid clip=a1 at=1 bars=2` (`offset=` starts it that many bars into the clip); kept in time order, and an overlap, a clip the track cannot play or a run past the song's end is refused with the validator's message |
  | `move_placement` | `tune do <song> move_placement track=acid at=3 to=1` |
  | `resize_placement` | `tune do <song> resize_placement track=acid at=3 bars=1` |
  | `remove_placement` | `tune do <song> remove_placement track=kick at=3` (the result lists it) |
  | `add_section` | `tune do <song> add_section name=break bars=2 index=1` inserts an empty section before section 1 (0-based; the end by default); a placement or lane stretch crossing the insert point is split there, and everything from it on moves `bars` later |
  | `rename_section` | `tune do <song> rename_section index=1 name=drop` |

  These rewrite a track's `play` list (and `add_section` the lane tables), which drops
  the comments inside a rewritten list.

  The lane ones, refused the same way (`param` is spelled as in the file, split at its
  first `.` into the track, or `mix`, and the param id; `at` is the 1-based bar a
  stretch starts at, and a stretch is named by it):

  | Action | Example |
  | --- | --- |
  | `add_lane` | `tune do <song> add_lane param=mix.reverb.size at=1 bars=4 points='[[0, 0.2], [4, 0.6]]'` writes a new `[[automation]]` table (the first one creates the list) right after the stretches of the section it starts in; an overlap, an unknown param or a point out of range is refused with the validator's message |
  | `remove_lane` | `tune do <song> remove_lane param=acid.acid303.cutoff at=3` drops the stretch's table with its comments (the result lists it) |
  | `set_lane_points` | `tune do <song> set_lane_points param=acid.acid303.cutoff at=3 points='[[0, 0.3], [2, 0.9]]'` changes only the `points` line |

  The scene ones, refused the same way (`index` is 0-based, like a section's; a
  file that writes `scenes = { … }` as one root inline table refuses `add_scene` and
  `move_scene`, and one with `[scenes]` and inline entries refuses `move_scene`: edit
  those forms by hand):

  | Action | Example |
  | --- | --- |
  | `add_scene` | `tune do <song> add_scene scene=jam index=0` adds an empty scene before scene 0 (the end by default); its table goes right before that scene's comments |
  | `remove_scene` | `tune do <song> remove_scene scene=jam` removes the scene, the comments above it and its slots (the result lists them; the text line is `removed slot a1 on acid in scene groove`) |
  | `rename_scene` | `tune do <song> rename_scene scene=groove to=drop`; keeps its place and comments |
  | `move_scene` | `tune do <song> move_scene scene=break index=0` removes the scene and inserts it at `index`; its comments travel with it |
  | `set_slot` | `tune do <song> set_slot scene=groove track=acid clip=a1` puts the clip in the slot (a clip the track's instrument can play); `clip=` empties the slot |

- Undo a whole run: `tune do <song> undo_run <run id> --json` (the studio
  open or closed; the app's "Undo this run" does the same). It reads the
  run's action records in `edits.jsonl` and puts back every target the run
  changed, newest first, as one write and one undo step (⌘Z brings the run
  back; nothing to revert writes nothing). It never overwrites a later
  edit: a target someone changed after the run is kept and listed
  (`changed_later`, `by` `human`, `claude` for another run, or `external`
  / `null` for a change outside the studio or one the log missed), and so
  is one the song refuses to take back (`still_used`, e.g. a clip that is
  placed again, with the validator's `message`). A track's placements and
  a lane's stretches go back one at a time: a placement the run added
  goes if it is still there, one it removed comes back if nothing there
  now overlaps it; when one the run added was changed later, every one it
  removed from that list is kept too (it may have been moved on). An
  element another write took away or put there again since (a ⌘Z then
  ⌘⇧Z, an identical placement made again) is kept, and so is one, added
  or removed, when another write rebuilt what it belongs to (its track,
  its clip or the sections for a placement; its track, the track's
  instrument or effects, or the sections for a lane stretch); a lane
  stretch is kept whenever the run's change to its whole track is (a
  track made again by hand under a removed one's id gets no old lane). A
  list another edit touched while the run was still working on it is kept
  whole. A target that changed between two of the run's writes with no
  edit in the log to say so (a hand edit with the studio closed, which
  the run's next write carried along) is kept, a list whole, with `by`
  `null`. A section
  edit (`add_section`, `move_section`, `resize_section` and the like)
  goes back whole with the placements and stretches it moved, or is kept
  whole when any part of it changed later, and also when any other edit
  since changed placements, lane stretches, sections or a whole track
  anywhere in the song. A rename (`rename_track`, `rename_clip`,
  `rename_device`) likewise goes back whole with everything its write
  changed (the lanes, placements and duck source that followed it), or
  is kept whole when any part of it changed later (the new name renamed
  again or removed included), so the old name never comes back beside the
  new one. A target someone set back to
  its value before the run is listed in `already`, not `kept`. The
  result is `{"run", "title", "reverted": [{"target",
  "item"}], "kept": [{"target", "item", "reason", "by", "message"}],
  "already": [<target>]}` (`item` is the placement or stretch for
  `tracks.<t>.play` and `automation.<t>.<param>`, else `null`; `already`
  lists targets back at their value before the run); the text is `undid
  run r1: 4 reverted, 1 kept` and one `kept <target>: changed later by
  human` (`by another run`, `outside the studio`, `still used: <message>`)
  line each. A damaged record line in the log refuses it ("the edit log
  has a damaged record at line N; nothing was changed"). Costs: a removed
  lane, track or clip comes back without its comments (so do the lanes
  `rename_device` moved, and a section `rename_section` renamed loses the
  comment above it), a restored clip goes to the end of the clip pool, and
  an explicit `send.* = -60` (a silent bus) is not restored.
- To see the UI: `tune serve <song> --device null --port 4580` (in the
  background; `task ui:build` first), then drive Chrome with the
  `agent-browser` skill: start the agent Chrome, pick a session
  (`session id --prefix <task>`), `open http://127.0.0.1:4580/`, `snapshot`,
  interact, and `screenshot scratch/<name>.png`. Read the PNG to see the UI.
  Ctrl-C or `kill` stops the server and writes pending edits (it prints
  `stopped`). Never commit screenshots.
- `edits.jsonl` sits next to the song: local and gitignored, never committed.
  Besides one entry per changed path, each write logs one action record
  (`"record": "action"`: the actions, the run and each target's value before
  and after) for run undo; `tune changes` leaves the records out.

## Rules

- Run `task verify` before every commit. Run `task setup` once per clone to
  enable the pre-commit privacy hook.
- Atomic commits, one concern each, conventional-commit messages.
- Privacy: never commit audio files, absolute home paths, email addresses, real
  names or other machine-specific data. `scripts/check-privacy.sh` enforces it.
  Use generic placeholders such as `~/Downloads/...` in docs.
- Never commit audio of any kind (renders, samples, reference tracks).
- Parity: the Rust port must match the C++ reference (compiled with
  `-O2 -std=c++14 -ffp-contract=off`). Module-level tolerance is 1e-12 max abs
  diff, full-voice 1e-9. If a test fails, find the cause (a missed `float` cast,
  operation order, a default); never loosen the tolerance. Mirror every C++
  `float` cast and member exactly (`as f32 as f64` where C++ narrows).
- Determinism: same inputs give bitwise-identical output; no unseeded
  randomness, no time-dependent behaviour.
- No GPL code (e.g. JC303) may be copied. Open303 is MIT and attributed in
  `THIRD_PARTY.md`.
