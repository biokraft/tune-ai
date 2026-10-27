// Hand-written mirror of the studio wire protocol (crates/tune-studio: command.rs, action/,
// view.rs, studio.rs, core.rs). The golden file crates/tune-studio/tests/fixtures/protocol.json
// pins the format for both sides; protocol.test.ts checks the names against it.
import type { AcidStep, DrumHit, ParamSpecView, PlacementView, SectionView } from "@tune/ui";

// ---- Commands -------------------------------------------------------------------------------

export type Command =
  // Studio commands: only a studio client sends them.
  | { cmd: "list_songs" }
  /**
   * Resolves with the current `StudioState`; changes nothing. A migration the studio could not
   * save is sent again as an `error` event, for a transport that dropped the first one.
   */
  | { cmd: "get_state" }
  | { cmd: "open_song"; args: { path: string } }
  | { cmd: "resolve_offered"; args: { reapply: boolean } }
  // Actions: `tune do` runs the same ones (`tune actions` lists them).
  /** `from_bar` is 0-based; null starts at the cursor. */
  | { cmd: "play"; args: { from_bar: number | null } }
  | { cmd: "stop" }
  | { cmd: "seek_bar"; args: { bar: number } }
  /** Half-open `[start, end)`, 0-based; null stops looping. */
  | { cmd: "set_loop"; args: { bars: [number, number] | null } }
  | { cmd: "undo" }
  | { cmd: "redo" }
  /** Sets the value at an edit path (`tracks.acid.acid303.cutoff`), as `tune set` does. */
  | { cmd: "set_value"; args: { path: string; value: Json } }
  | {
      cmd: "set_param";
      args: { track: string; param: string; value: number; commit: boolean };
    }
  /** Sets a setting (e.g. `mode`) of the track's device `device` (its id, e.g. `dist`). */
  | {
      cmd: "set_device_setting";
      args: { track: string; device: string; setting: string; value: string };
    }
  | {
      cmd: "set_acid_step";
      args: {
        clip: string;
        index: number;
        note: number | null;
        accent: boolean;
        slide: boolean;
      };
    }
  | { cmd: "set_drum_step"; args: { clip: string; index: number; hit: DrumHit } }
  | { cmd: "move_section"; args: { from: number; to: number } }
  | { cmd: "resize_section"; args: { index: number; bars: number } }
  | { cmd: "duplicate_section"; args: { index: number } }
  | { cmd: "delete_section"; args: { index: number } }
  | { cmd: "set_section_play"; args: { section: number; track: string; clip: string | null } }
  | {
      cmd: "set_track_mix";
      args: {
        track: string;
        gain_db: number | null;
        pan: number | null;
        mute: boolean | null;
        solo: boolean | null;
      };
    };

export const COMMAND_NAMES = [
  "list_songs",
  "get_state",
  "open_song",
  "resolve_offered",
  "play",
  "stop",
  "seek_bar",
  "set_loop",
  "undo",
  "redo",
  "set_value",
  "set_param",
  "set_device_setting",
  "set_acid_step",
  "set_drum_step",
  "move_section",
  "resize_section",
  "duplicate_section",
  "delete_section",
  "set_section_play",
  "set_track_mix",
] as const satisfies readonly Command["cmd"][];

// Compile-time check that COMMAND_NAMES covers every Command variant.
type MissingCommand = Exclude<Command["cmd"], (typeof COMMAND_NAMES)[number]>;
const _allCommands: MissingCommand extends never ? true : MissingCommand = true;
void _allCommands;

// ---- Views ----------------------------------------------------------------------------------

/** A JSON value as the Rust side sends it (`serde_json::Value`). */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** One value that differs between two songs, by edit path (`tracks.acid.acid303.cutoff`). */
export type Change = { path: string; old: Json; new: Json };

/** A problem in the song file, with its position when it has one. */
export type ErrorView = { line: number | null; column: number | null; message: string };

export type ParamView = ParamSpecView;

export type FxView = {
  /** The device id (`eq`, `eq2`): the prefix of its param ids. */
  id: string;
  /** The module name (`distortion`, `eq`). */
  kind: string;
  /** The distortion mode; empty for a device without one. */
  mode: string;
  modes: string[];
  params: ParamView[];
};

export type { PlacementView };

export type TrackView = {
  id: string;
  /** The instrument's module name (`acid303`). */
  instrument: string;
  /** The instrument's device id: the prefix of its param ids. */
  instrument_id: string;
  /** The clip kinds the instrument plays (`"acid"`, `"drum"`). */
  plays: string[];
  gain_db: number;
  pan: number;
  mute: boolean;
  solo: boolean;
  params: ParamView[];
  /** The strip params (sends and ducking), in `STRIP_PARAMS` order. */
  strip: ParamView[];
  fx: FxView[];
  /** What the track plays, in time order. */
  placements: PlacementView[];
};

/** The `[mix]` table: the master stage, the duck source and the shared buses. */
export type MixView = {
  /** `"clip"` or `"limit"`. */
  master: string;
  /** Every master mode, in the order the UI offers them. */
  masters: string[];
  /** The track every `duck.depth` follows (the file's, else the first kick). */
  duck_source: string | null;
  delay: ParamView[];
  reverb: ParamView[];
  master_params: ParamView[];
  /** Whether some track sends to the bus. */
  delay_active: boolean;
  reverb_active: boolean;
};

/** One note of a note clip; `start` and `len` are in 16th steps. */
export type NoteView = { start: number; pitch: number; len: number; vel: number };

export type PatternView =
  | { kind: "acid"; id: string; steps: AcidStep[] }
  | { kind: "drum"; id: string; hits: DrumHit[] }
  | { kind: "notes"; id: string; bars: number; notes: NoteView[] };

export type SongView = {
  name: string;
  bpm: number;
  total_bars: number;
  tracks: TrackView[];
  patterns: PatternView[];
  sections: SectionView[];
  mix: MixView;
};

export type StudioState = {
  path: string | null;
  song: SongView | null;
  playing: boolean;
  /** 0-based. */
  cursor_bar: number;
  /** Half-open `[start, end)`, 0-based. */
  loop_bars: [number, number] | null;
  can_undo: boolean;
  can_redo: boolean;
  /** Local changes an external edit overrode, offered for re-apply. */
  offered: Change[];
  /** Problems in the song file on disk; edits wait until they are fixed. */
  file_errors: ErrorView[];
  /** The format version the song file was migrated from when opened, while one undo reverts it. */
  migrated_from: number | null;
};

export type Source = "human" | "claude" | "external";

/** What a change on disk did to the open song. */
export type ExternalChange = {
  source: Source;
  changes: Change[];
  /** The file was replaced by a different song; every pending edit was offered. */
  replaced: boolean;
  merged: Change[];
  offered: Change[];
  /** Why the new text does not parse; when set, the other fields are empty. */
  errors: ErrorView[];
};

// ---- Events ---------------------------------------------------------------------------------

export type StateEvent = { event: "state" } & StudioState;
/** All 0-based: `step` counts 16ths from the song start. */
export type PlayheadEvent = {
  event: "playhead";
  step: number;
  bar: number;
  beat: number;
  playing: boolean;
};
export type AppliedEvent = { event: "applied"; source: Source; changes: Change[] };
export type ExternalChangeEvent = { event: "external_change" } & ExternalChange;
export type ErrorEvent = { event: "error"; message: string };

export type StudioEvent =
  | StateEvent
  | PlayheadEvent
  | AppliedEvent
  | ExternalChangeEvent
  | ErrorEvent;

export const EVENT_NAMES = [
  "state",
  "playhead",
  "applied",
  "external_change",
  "error",
] as const satisfies readonly StudioEvent["event"][];

type MissingEvent = Exclude<StudioEvent["event"], (typeof EVENT_NAMES)[number]>;
const _allEvents: MissingEvent extends never ? true : MissingEvent = true;
void _allEvents;
