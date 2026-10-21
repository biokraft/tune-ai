import type { Command, StudioEvent } from "./protocol";
import { LOST_CONNECTION, type StudioClient } from "./StudioClient";

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

type Reply =
  | { reply: number | null; ok: true; result: unknown }
  | {
      reply: number | null;
      ok: false;
      error: string;
    };

/**
 * The studio over `tune serve`'s WebSocket. Calls made before the socket opens are queued.
 * A reply can arrive before the events its call caused, so callers never rely on ordering.
 * There is no auto-reconnect: once the socket closes every pending and later call fails.
 */
export function createWsClient(url: string, Ws: typeof WebSocket = WebSocket): StudioClient {
  const ws = new Ws(url);
  const pending = new Map<number, Pending>();
  const handlers = new Set<(e: StudioEvent) => void>();
  const lostHandlers = new Set<() => void>();
  let queue: string[] = [];
  let nextId = 1;
  let closed = false;

  ws.onopen = () => {
    for (const frame of queue) ws.send(frame);
    queue = [];
  };

  ws.onmessage = (msg: MessageEvent) => {
    let frame: unknown;
    try {
      frame = JSON.parse(String(msg.data));
    } catch {
      return;
    }
    if (typeof frame !== "object" || frame === null) return;
    if ("reply" in frame) {
      const r = frame as Reply;
      const p = r.reply === null ? undefined : pending.get(r.reply);
      if (!p || r.reply === null) return;
      pending.delete(r.reply);
      if (r.ok) p.resolve(r.result);
      else p.reject(new Error(r.error));
    } else if ("event" in frame) {
      for (const h of [...handlers]) h(frame as StudioEvent);
    }
  };

  ws.onclose = () => {
    if (closed) return;
    closed = true;
    queue = [];
    for (const p of pending.values()) p.reject(new Error(LOST_CONNECTION));
    pending.clear();
    for (const h of [...lostHandlers]) h();
  };

  return {
    call<T = unknown>(cmd: Command): Promise<T> {
      if (closed) return Promise.reject(new Error(LOST_CONNECTION));
      const id = nextId++;
      const frame = JSON.stringify({ id, ...cmd });
      return new Promise<T>((resolve, reject) => {
        pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
        if (ws.readyState === Ws.OPEN) ws.send(frame);
        else queue.push(frame);
      });
    },
    subscribe(h) {
      handlers.add(h);
      return () => handlers.delete(h);
    },
    onLost(h) {
      lostHandlers.add(h);
      return () => lostHandlers.delete(h);
    },
  };
}
