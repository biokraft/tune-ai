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
];

/// The catalog entry of the action named `name`.
pub fn find_action(name: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|a| a.name == name)
}
