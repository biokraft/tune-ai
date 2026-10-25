import { cn } from "@tune/ui";
import { useEffect, useState } from "react";
import type { Command } from "../client/protocol";

export type OpenSongScreenProps = {
  send(cmd: Command): Promise<unknown>;
};

type Songs =
  | { kind: "loading" }
  | { kind: "ready"; paths: string[] }
  | { kind: "failed"; message: string };

const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play";

/** `songs/first-acid/song.toml` → `first-acid`. */
const songName = (path: string) => path.split("/").at(-2) ?? path;

/** Shown while no song is open: the songs under `songs/`, one row each, on a centred card. */
export function OpenSongScreen({ send }: OpenSongScreenProps) {
  const [songs, setSongs] = useState<Songs>({ kind: "loading" });
  const [attempt, setAttempt] = useState(0);
  const [opening, setOpening] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setSongs({ kind: "loading" });
    send({ cmd: "list_songs" }).then(
      (r) => live && setSongs({ kind: "ready", paths: Array.isArray(r) ? (r as string[]) : [] }),
      (e: unknown) =>
        live && setSongs({ kind: "failed", message: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      live = false;
    };
  }, [send, attempt]);

  const open = (path: string) => {
    setOpening(path);
    send({ cmd: "open_song", args: { path } }).catch(() => setOpening(null));
  };

  return (
    <main className="flex min-h-0 flex-1 items-center justify-center p-6">
      <div className="glass flex max-h-full w-full max-w-xl flex-col gap-4 overflow-hidden rounded-r p-6">
        <h1 className="text-xl font-bold text-ink">Open a song</h1>
        {songs.kind === "loading" && <p className="text-ink2">Looking for songs…</p>}
        {songs.kind === "failed" && (
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-kick">{songs.message}</p>
            <button
              type="button"
              className={cn(
                "flex-none rounded-rs border border-line2 px-3 py-1 text-xs font-semibold text-ink hover:bg-ink/[.06]",
                focusRing,
              )}
              onClick={() => setAttempt((n) => n + 1)}
            >
              Try again
            </button>
          </div>
        )}
        {songs.kind === "ready" && songs.paths.length === 0 && (
          <p className="text-ink2">
            No songs yet. Ask Claude to draft one in songs/&lt;name&gt;/song.toml.
          </p>
        )}
        {songs.kind === "ready" && songs.paths.length > 0 && (
          <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto overscroll-none">
            {songs.paths.map((path) => (
              <li key={path} className="border-t border-line first:border-t-0">
                <button
                  type="button"
                  disabled={opening !== null}
                  onClick={() => open(path)}
                  className={cn(
                    "flex w-full items-baseline justify-between gap-4 rounded-rs px-2 py-3 text-left",
                    "hover:bg-ink/[.06] disabled:opacity-60 disabled:hover:bg-transparent",
                    focusRing,
                  )}
                >
                  <span className="text-[15px] font-semibold text-ink">{songName(path)}</span>
                  <span className="font-mono text-[11px] text-ink3">
                    {opening === path ? "Opening…" : path}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </main>
  );
}
