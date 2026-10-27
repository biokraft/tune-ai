// The studio wire protocol. The types come from the Rust side: generated.ts is written from
// crates/tune-studio by `UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types` and is never
// edited by hand. The golden file crates/tune-studio/tests/fixtures/protocol.json pins the format
// for both sides; protocol.test.ts checks the names against it.
import { ACTION_NAMES, STUDIO_COMMAND_NAMES, type Event, type JsonValue } from "./generated";

export type {
  AcidStepView,
  Action,
  Change,
  Command,
  DrumHitDto,
  ErrorView,
  ExternalChange,
  FxView,
  LaneView,
  MixView,
  NoteView,
  ParamView,
  PatternView,
  PlacementView,
  SectionView,
  SongView,
  Source,
  StudioCommand,
  StudioState,
  TrackView,
} from "./generated";

/** Everything the studio sends to its subscribers. */
export type StudioEvent = Event;

/** A JSON value as the Rust side sends it (`serde_json::Value`). */
export type Json = JsonValue;

/** Every command's wire name: the studio commands, then the actions. */
export const COMMAND_NAMES = [...STUDIO_COMMAND_NAMES, ...ACTION_NAMES] as const;

// ---- Events ---------------------------------------------------------------------------------

export type StateEvent = Extract<StudioEvent, { event: "state" }>;
/** All 0-based: `step` counts 16ths from the song start. */
export type PlayheadEvent = Extract<StudioEvent, { event: "playhead" }>;
export type AppliedEvent = Extract<StudioEvent, { event: "applied" }>;
export type ExternalChangeEvent = Extract<StudioEvent, { event: "external_change" }>;
export type ErrorEvent = Extract<StudioEvent, { event: "error" }>;

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
