import type { ParamSpecView } from "@tune/ui";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { sentenceCase } from "../lib/song";

/** How long a knob note stays on the help line. */
const NOTE_MS = 6000;

export type Help = {
  /** The hovered or focused knob, or null when none is. */
  show(spec: ParamSpecView | null): void;
  /** A short-lived message about the last knob action; shown in place of the help. */
  note(text: string | null): void;
};

type HelpState = { spec: ParamSpecView | null; note: string | null };

const ActionsContext = createContext<Help | null>(null);
const StateContext = createContext<HelpState>({ spec: null, note: null });

const NOOP: Help = { show() {}, note() {} };

/** Holds what the help line shows; knobs anywhere below post to it through `useHelp`. */
export function HelpProvider({ children }: { children: ReactNode }) {
  const [spec, setSpec] = useState<ParamSpecView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(timer.current), []);

  const showNote = useCallback((text: string | null) => {
    clearTimeout(timer.current);
    setNote(text);
    if (text) timer.current = setTimeout(() => setNote(null), NOTE_MS);
  }, []);

  const actions = useMemo<Help>(() => ({ show: setSpec, note: showNote }), [showNote]);
  const state = useMemo(() => ({ spec, note }), [spec, note]);
  return (
    <ActionsContext.Provider value={actions}>
      <StateContext.Provider value={state}>{children}</StateContext.Provider>
    </ActionsContext.Provider>
  );
}

/** The actions only (stable), so knobs posting help never re-render on every change. */
export function useHelp(): Help {
  return useContext(ActionsContext) ?? NOOP;
}

/** One 36px line under the dock explaining the knob in hand, so help never covers a knob. */
export function HelpLine() {
  const { spec, note } = useContext(StateContext);
  return (
    <div
      aria-live="polite"
      className="flex h-9 flex-none items-center gap-2.5 border-t border-line px-3.5 text-xs text-ink2"
    >
      {note ? (
        <p>{note}</p>
      ) : spec ? (
        <>
          <span className="shrink-0 font-bold text-ink">{sentenceCase(spec.label)}</span>
          <p className="truncate">{spec.help}</p>
        </>
      ) : (
        <p>Hover or focus a knob to learn what it does.</p>
      )}
    </div>
  );
}
