const raf: (cb: () => void) => number =
  typeof requestAnimationFrame === "function"
    ? (cb) => requestAnimationFrame(cb)
    : (cb) => setTimeout(cb, 16) as unknown as number;
const caf: (id: number) => void =
  typeof cancelAnimationFrame === "function" ? cancelAnimationFrame : (id) => clearTimeout(id);

/**
 * Coalesces values per key to at most one `flush(key, value)` per animation frame, sending the
 * latest value. `cancel(key)` drops a key's unsent value (a release supersedes it).
 */
export function frameThrottle<T>(flush: (key: string, value: T) => void) {
  const pending = new Map<string, T>();
  let frame: number | null = null;
  const run = () => {
    frame = null;
    const batch = [...pending];
    pending.clear();
    for (const [k, v] of batch) flush(k, v);
  };
  return {
    push(key: string, value: T) {
      pending.set(key, value);
      if (frame === null) frame = raf(run);
    },
    cancel(key: string) {
      pending.delete(key);
      if (pending.size === 0 && frame !== null) {
        caf(frame);
        frame = null;
      }
    },
  };
}
