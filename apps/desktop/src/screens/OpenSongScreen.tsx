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
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen";

/** `songs/first-acid/song.toml` → `first-acid`. */
const songName = (path: string) => path.split("/").at(-2) ?? path;

/** Shown while no song is open: the songs under `songs/`, one button each. */
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
    <main className="mx-auto flex w-full max-w-xl flex-col gap-6 px-8 py-16">
      <h1 className="font-label text-3xl text-silkscreen">Open a song</h1>
      {songs.kind === "loading" && (
        <p className="font-value text-sm text-silkscreen/70">Looking for songs…</p>
      )}
      {songs.kind === "failed" && (
        <div className="flex items-center gap-3">
          <p className="font-value text-sm text-led">{songs.message}</p>
          <button
            type="button"
            className={cn(
              "rounded-sm border border-border px-3 py-1 font-label text-sm",
              focusRing,
            )}
            onClick={() => setAttempt((n) => n + 1)}
          >
            Try again
          </button>
        </div>
      )}
      {songs.kind === "ready" && songs.paths.length === 0 && (
        <p className="font-value text-base text-silkscreen/80">
          No songs yet. Ask Claude to draft one in songs/&lt;name&gt;/song.toml.
        </p>
      )}
      {songs.kind === "ready" && songs.paths.length > 0 && (
        <ul className="flex flex-col gap-2">
          {songs.paths.map((path) => (
            <li key={path}>
              <button
                type="button"
                disabled={opening !== null}
                onClick={() => open(path)}
                className={cn(
                  "faceplate flex w-full items-baseline justify-between gap-4 rounded-md px-4 py-3 text-left",
                  "hover:brightness-110 disabled:opacity-60",
                  focusRing,
                )}
              >
                <span className="font-label text-xl text-silkscreen">{songName(path)}</span>
                <span className="font-value text-xs text-silkscreen/60">
                  {opening === path ? "Opening…" : path}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </main>
  );
}
