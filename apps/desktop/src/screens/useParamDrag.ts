import { useEffect, useMemo, useRef, useState } from "react";
import { toPlain } from "@tune/ui";
import type { Command, ParamView } from "../client/protocol";
import { frameThrottle } from "../lib/frameThrottle";

/** After a commit's reply, how long the released value may wait for the studio's state. */
const SETTLE_MS = 600;

type Live = { value: number; committed: boolean };

/** Within the 4-significant-digit rounding the studio applies to a committed value. */
const settled = (a: number, b: number) => Math.abs(a - b) <= Math.abs(b) * 1e-3 + 1e-9;

export type ParamDrag = {
  /**
   * The value to show on `p`'s knob: the held or just-released value, else the value of the lane
   * driving it (`lane`, normalized), else the studio's.
   */
  valueOf(p: ParamView, lane?: number | null): number;
  /** A knob moved: shown at once, sent at most once per frame (commit false). */
  change(id: string, value: number): void;
  /** A knob was released or stepped: sent with commit true. */
  commit(id: string, value: number): void;
};

/**
 * Drag handling shared by every group of knobs that edits one track's params through
 * `set_param`: the live-value map, the frame-throttled uncommitted sends, and the settle window
 * that keeps a released value on the knob until the studio's state shows it. A release on an
 * automated param moves the lane instead (the studio's job), which the knob shows once the
 * settle window ends.
 */
export function useParamDrag(
  track: string,
  send: (cmd: Command) => Promise<unknown>,
  params: ParamView[],
): ParamDrag {
  const [live, setLive] = useState<Map<string, Live>>(new Map());
  const sendRef = useRef(send);
  sendRef.current = send;

  const throttle = useMemo(
    () =>
      frameThrottle<number>((param, value) => {
        void sendRef
          .current({ cmd: "set_param", args: { track, param, value, commit: false } })
          .catch(() => {});
      }),
    [track],
  );

  // A released value stays on the knob until the studio's state shows it.
  useEffect(() => {
    setLive((prev) => {
      let next: Map<string, Live> | null = null;
      for (const [id, l] of prev) {
        const p = params.find((x) => x.id === id);
        if (l.committed && (!p || settled(p.value, l.value))) {
          next ??= new Map(prev);
          next.delete(id);
        }
      }
      return next ?? prev;
    });
  }, [params]);

  const setLiveValue = (id: string, value: number, committed: boolean) =>
    setLive((prev) => new Map(prev).set(id, { value, committed }));
  const dropLive = (id: string, value: number) =>
    setLive((prev) => {
      const l = prev.get(id);
      if (!l || !l.committed || l.value !== value) return prev;
      const next = new Map(prev);
      next.delete(id);
      return next;
    });

  return {
    valueOf: (p, lane) => live.get(p.id)?.value ?? (lane == null ? p.value : toPlain(p, lane)),
    change(id, value) {
      setLiveValue(id, value, false);
      throttle.push(id, value);
    },
    commit(id, value) {
      throttle.cancel(id);
      setLiveValue(id, value, true);
      sendRef.current({ cmd: "set_param", args: { track, param: id, value, commit: true } }).then(
        () => setTimeout(() => dropLive(id, value), SETTLE_MS),
        () => dropLive(id, value),
      );
    },
  };
}
