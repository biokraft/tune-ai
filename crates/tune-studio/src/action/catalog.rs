//! The catalog: every action's kind, help and arguments, in enum order.

use super::{ActionKind, ActionSpec, ArgSpec, ArgType};
use ActionKind::{Edit, History, Live};
use ArgType::{Bool, Id, Int, Json, Number, ParamId, Path, Text};

const fn arg(name: &'static str, ty: ArgType, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        ty,
        required: true,
        help,
    }
}

const fn opt(name: &'static str, ty: ArgType, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        ty,
        required: false,
        help,
    }
}

const fn action(
    name: &'static str,
    kind: ActionKind,
    help: &'static str,
    args: &'static [ArgSpec],
) -> ActionSpec {
    ActionSpec {
        name,
        kind,
        help,
        args,
    }
}

const TRACK: ArgSpec = arg("track", Id, "track id");
const SECTION_INDEX: ArgSpec = arg("index", Int, "section index, 0-based");

/// Every action, in the order of the [`Action`](super::Action) enum.
pub const ACTIONS: &[ActionSpec] = &[
    // ----- live -----
    action(
        "play",
        Live,
        "Starts playback.",
        &[opt(
            "from_bar",
            Int,
            "bar to start at, 0-based; the cursor when left out",
        )],
    ),
    action("stop", Live, "Stops playback.", &[]),
    action(
        "seek_bar",
        Live,
        "Moves the cursor to a bar.",
        &[arg("bar", Int, "bar, 0-based")],
    ),
    action(
        "set_loop",
        Live,
        "Loops a range of bars, or stops looping.",
        &[opt(
            "bars",
            Json,
            "[start, end): 0-based bars, end not included, e.g. [2, 6]; left out stops looping",
        )],
    ),
    // ----- live session -----
    action(
        "launch_clip",
        Live,
        "Launches the clip in a scene's slot for a track at the next bar line; from that bar \
         the track is off the arrangement (its lanes do not drive it) until it is sent back. \
         Starts playback when stopped.",
        &[TRACK, arg("scene", Id, "scene id")],
    ),
    action(
        "launch_scene",
        Live,
        "Launches a scene at the next bar line: each track plays its slot's clip, and a track \
         with an empty slot stops. Starts playback when stopped.",
        &[arg("scene", Id, "scene id")],
    ),
    action(
        "stop_clip",
        Live,
        "Silences a track from the next bar line until it is sent back to the arrangement.",
        &[TRACK],
    ),
    action(
        "back_to_arrangement",
        Live,
        "Sends a track, or every track, back to the arrangement at the next bar line.",
        &[opt("track", Id, "track id; every track when left out")],
    ),
    // ----- history -----
    action(
        "undo",
        History,
        "Undoes the last write to the song (one burst of edits, or one outside change).",
        &[],
    ),
    action("redo", History, "Redoes the last undone write.", &[]),
    // ----- song edits -----
    action(
        "set_value",
        Edit,
        "Sets one value by its edit path, as `tune set` does.",
        &[
            arg(
                "path",
                Path,
                "edit path, e.g. song.bpm, tracks.acid.acid303.cutoff, mix.master or \
                 sections[1].bars (section indices are 0-based)",
            ),
            arg(
                "value",
                Json,
                "TOML value (820, true, \"hard\"); a bare word is a string, an empty value clears",
            ),
        ],
    ),
    action(
        "set_param",
        Edit,
        "Sets a knob: a param of a track's device or strip, or a mix param.",
        &[
            arg(
                "track",
                Id,
                "track id, or `mix` for a delay, reverb or master param",
            ),
            arg(
                "param",
                ParamId,
                "param id, e.g. acid303.cutoff, dist.drive, send.delay, reverb.size",
            ),
            arg(
                "value",
                Number,
                "plain value in the param's unit, rounded to 4 significant digits",
            ),
            opt(
                "commit",
                Bool,
                "false is the studio's live knob drag, heard but not written (not for \
                 `tune do`); default true",
            ),
        ],
    ),
    action(
        "set_device_setting",
        Edit,
        "Sets a setting of one of a track's devices, such as a distortion's mode.",
        &[
            TRACK,
            arg("device", Id, "device id, e.g. dist or eq2"),
            arg("setting", Id, "setting name, e.g. mode"),
            arg(
                "value",
                Text,
                "one of the setting's values (`tune params` lists them), e.g. hard",
            ),
        ],
    ),
    action(
        "set_acid_step",
        Edit,
        "Sets one step of an acid clip; a rest carries no accent or slide.",
        &[
            arg("clip", Id, "acid clip id"),
            arg("index", Int, "step, 0-based"),
            opt("note", Int, "MIDI note (C2 = 36); left out is a rest"),
            arg("accent", Bool, "accent the step"),
            arg("slide", Bool, "slide into the next step"),
        ],
    ),
    action(
        "set_drum_step",
        Edit,
        "Sets one step of a drum clip.",
        &[
            arg("clip", Id, "drum clip id"),
            arg("index", Int, "step, 0-based"),
            arg("hit", Text, "rest, hit or accent"),
        ],
    ),
    action(
        "move_section",
        Edit,
        "Moves a section, with everything it plays, to another place in the arrangement.",
        &[
            arg("from", Int, "index of the section to move, 0-based"),
            arg(
                "to",
                Int,
                "index it ends up at, 0-based (removed first, then inserted)",
            ),
        ],
    ),
    action(
        "resize_section",
        Edit,
        "Changes a section's length; placements and stretches that ran to its end run to the \
         new end (grow) or are cut at it (shrink), later ones move.",
        &[
            SECTION_INDEX,
            arg("bars", Int, "new length in bars, 1 to 1024"),
        ],
    ),
    action(
        "duplicate_section",
        Edit,
        "Inserts a copy of a section, with everything it plays, right after it.",
        &[SECTION_INDEX],
    ),
    action(
        "delete_section",
        Edit,
        "Deletes a section and everything inside it; the rest closes up. The last section \
         cannot be deleted.",
        &[SECTION_INDEX],
    ),
    action(
        "set_section_play",
        Edit,
        "Sets what a track plays in a section: one placement of a clip covering it.",
        &[
            arg("section", Int, "section index, 0-based"),
            TRACK,
            opt(
                "clip",
                Id,
                "clip id; left out or empty silences the track in the section",
            ),
        ],
    ),
    action(
        "set_track_mix",
        Edit,
        "Sets a track's mixer fields; only the given ones change, numbers are rounded to 4 \
         significant digits.",
        &[
            TRACK,
            opt("gain_db", Number, "gain in dB"),
            opt("pan", Number, "pan, -1 (left) to 1 (right)"),
            opt("mute", Bool, "mute the track"),
            opt("solo", Bool, "solo the track"),
        ],
    ),
    // ----- tracks and devices -----
    action(
        "add_track",
        Edit,
        "Appends a track playing a new instrument: gain and pan 0, no effects or placements.",
        &[
            arg(
                "track",
                Id,
                "new track id: lower-case letters, digits and `_`",
            ),
            arg(
                "instrument",
                Text,
                "instrument module, e.g. acid303, kick, clap, hat_closed, hat_open or ride",
            ),
        ],
    ),
    action(
        "remove_track",
        Edit,
        "Removes a track with its placements and lane stretches (the result lists the stretches).",
        &[TRACK],
    ),
    action(
        "rename_track",
        Edit,
        "Renames a track, keeping its place; its lane stretches and `mix.duck_source` follow. \
         The track's noise changes: it is seeded from the song seed and the track id.",
        &[TRACK, arg("to", Id, "new track id")],
    ),
    action(
        "add_effect",
        Edit,
        "Inserts an effect into a track's chain; the result is the new device's id.",
        &[
            TRACK,
            arg("module", Text, "effect module: distortion or eq"),
            opt(
                "id",
                Id,
                "device id; default the module's prefix, else the prefix and a number (eq2)",
            ),
            opt("index", Int, "place in the chain, 0-based; default the end"),
        ],
    ),
    action(
        "remove_effect",
        Edit,
        "Removes an effect from a track's chain with its lane stretches (the result lists them).",
        &[TRACK, arg("device", Id, "effect id, e.g. dist or eq2")],
    ),
    action(
        "move_effect",
        Edit,
        "Moves an effect to another place in a track's chain; its settings move with it.",
        &[
            TRACK,
            arg("device", Id, "effect id, e.g. dist or eq2"),
            arg("index", Int, "place it ends up at, 0-based"),
        ],
    ),
    action(
        "rename_device",
        Edit,
        "Renames a device of a track (its instrument or an effect); its param keys and lane \
         stretches follow.",
        &[
            TRACK,
            arg("device", Id, "device id, e.g. acid303 or dist"),
            arg("to", Id, "new device id; not `send` or `duck`"),
        ],
    ),
    action(
        "set_instrument",
        Edit,
        "Swaps a track's instrument. The new device gets the module's prefix as its id and \
         default settings; the old instrument's params and lane stretches go, as do placements \
         of clips the new module cannot play (the clips stay) and a `mix.duck_source` naming the \
         track when the new module plays no drum clips. The result lists what went.",
        &[
            TRACK,
            arg(
                "module",
                Text,
                "instrument module, e.g. acid303, kick, clap, hat_closed, hat_open or ride",
            ),
        ],
    ),
    // ----- clips -----
    action(
        "add_clip",
        Edit,
        "Adds a clip to the pool, after the last one. `steps` is for acid and drum clips (16 \
         rests by default), `bars` and `notes` for notes clips (1 bar and no notes by default).",
        &[
            arg(
                "clip",
                Id,
                "new clip id: lower-case letters, digits and `_`",
            ),
            arg("kind", Text, "clip kind: acid, drum or notes"),
            opt(
                "steps",
                Text,
                "steps in the kind's notation (acid and drum)",
            ),
            opt("bars", Int, "length in bars (notes)"),
            opt(
                "notes",
                Json,
                "array of notes, each { start, pitch, len, vel } (notes)",
            ),
        ],
    ),
    action(
        "remove_clip",
        Edit,
        "Removes a clip with every placement of it (the result lists the placements).",
        &[arg("clip", Id, "clip id")],
    ),
    action(
        "rename_clip",
        Edit,
        "Renames a clip, keeping its place in the pool; its placements follow.",
        &[arg("clip", Id, "clip id"), arg("to", Id, "new clip id")],
    ),
    action(
        "duplicate_clip",
        Edit,
        "Copies a clip as a new one, right after the original in the pool.",
        &[arg("clip", Id, "clip id"), arg("to", Id, "id of the copy")],
    ),
    // ----- placements and sections -----
    action(
        "add_placement",
        Edit,
        "Adds a placement of a clip on a track from bar `at` (1-based) for `bars` bars, \
         starting `offset` bars into the clip (0 by default).",
        &[
            arg("track", Id, "track id"),
            arg("clip", Id, "clip id"),
            arg("at", Int, "first bar, from 1"),
            arg("bars", Int, "length in bars"),
            opt("offset", Int, "bars into the clip at which it starts"),
        ],
    ),
    action(
        "move_placement",
        Edit,
        "Moves the placement that starts at bar `at` to start at bar `to` (both 1-based).",
        &[
            arg("track", Id, "track id"),
            arg("at", Int, "start bar of the placement, from 1"),
            arg("to", Int, "new start bar, from 1"),
        ],
    ),
    action(
        "resize_placement",
        Edit,
        "Makes the placement that starts at bar `at` (1-based) `bars` bars long.",
        &[
            arg("track", Id, "track id"),
            arg("at", Int, "start bar of the placement, from 1"),
            arg("bars", Int, "new length in bars"),
        ],
    ),
    action(
        "remove_placement",
        Edit,
        "Removes the placement that starts at bar `at` (1-based); the result lists it.",
        &[
            arg("track", Id, "track id"),
            arg("at", Int, "start bar of the placement, from 1"),
        ],
    ),
    action(
        "add_section",
        Edit,
        "Inserts an empty section of `bars` bars before section `index` (appended by default). \
         A placement or lane stretch crossing the insert point is split there, and everything \
         from it on moves later.",
        &[
            arg("name", Text, "section name"),
            arg("bars", Int, "length in bars, 1 to 1024"),
            opt(
                "index",
                Int,
                "0-based section to insert before; the end by default",
            ),
        ],
    ),
    action(
        "rename_section",
        Edit,
        "Renames a section.",
        &[
            arg("index", Int, "0-based section"),
            arg("name", Text, "new name"),
        ],
    ),
    // ----- scenes -----
    action(
        "add_scene",
        Edit,
        "Adds an empty scene before scene `index` (appended by default). `index` is 0-based.",
        &[
            arg(
                "scene",
                Id,
                "new scene id: lower-case letters, digits and `_`",
            ),
            opt(
                "index",
                Int,
                "0-based position among the scenes; the end by default",
            ),
        ],
    ),
    action(
        "remove_scene",
        Edit,
        "Removes a scene with its slots (the result lists them).",
        &[arg("scene", Id, "scene id")],
    ),
    action(
        "rename_scene",
        Edit,
        "Renames a scene, keeping its place.",
        &[arg("scene", Id, "scene id"), arg("to", Id, "new scene id")],
    ),
    action(
        "move_scene",
        Edit,
        "Moves a scene to `index` (0-based), as a removal then an insertion.",
        &[
            arg("scene", Id, "scene id"),
            arg("index", Int, "0-based position among the scenes"),
        ],
    ),
    action(
        "set_slot",
        Edit,
        "Puts a clip in a scene's slot for a track; no clip empties the slot. The track's \
         instrument must be able to play the clip.",
        &[
            arg("scene", Id, "scene id"),
            TRACK,
            opt("clip", Id, "clip id; left out or empty clears the slot"),
        ],
    ),
    // ----- lanes -----
    action(
        "add_lane",
        Edit,
        "Adds a stretch of a param's automation lane from bar `at` (1-based) for `bars` bars. \
         Points are [bar offset from `at`, value 0 to 1] pairs; a stretch may not overlap \
         another of the same param.",
        &[
            arg(
                "param",
                ParamId,
                "lane param as in the file, e.g. acid.acid303.cutoff or mix.reverb.size",
            ),
            arg("at", Int, "first bar, from 1"),
            arg("bars", Int, "length in bars"),
            arg(
                "points",
                Json,
                "[[bar offset, value 0 to 1], ...] in time order",
            ),
        ],
    ),
    action(
        "remove_lane",
        Edit,
        "Removes the lane stretch of `param` that starts at bar `at` (1-based); the result \
         lists it.",
        &[
            arg("param", ParamId, "lane param as in the file"),
            arg("at", Int, "start bar of the stretch, from 1"),
        ],
    ),
    action(
        "set_lane_points",
        Edit,
        "Replaces the points of the lane stretch of `param` that starts at bar `at` (1-based).",
        &[
            arg("param", ParamId, "lane param as in the file"),
            arg("at", Int, "start bar of the stretch, from 1"),
            arg(
                "points",
                Json,
                "[[bar offset, value 0 to 1], ...] in time order",
            ),
        ],
    ),
    // ----- runs -----
    action(
        "undo_run",
        Edit,
        "Undoes every edit of a run, keeping (and listing) what someone changed since.",
        &[arg("run", Text, "run id, as given with --run")],
    ),
];

/// The catalog entry of the action named `name`.
pub fn find_action(name: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|a| a.name == name)
}
