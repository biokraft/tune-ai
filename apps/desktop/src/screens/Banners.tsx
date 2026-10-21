import { cn } from "@tune/ui";
import type { ErrorView } from "../client/protocol";

const bar = "flex flex-col gap-1 border-b px-6 py-3 font-value text-sm";

/** The WebSocket closed; nothing reconnects it. */
export function LostBanner() {
  return (
    <div role="alert" className={cn(bar, "border-led/50 bg-led/15 text-silkscreen")}>
      <p>
        Lost connection to the studio. Restart <code className="font-value">tune serve</code>, then
        reload this page.
      </p>
    </div>
  );
}

const position = (e: ErrorView) =>
  e.line !== null && e.column !== null ? `${e.line}:${e.column} ${e.message}` : e.message;

/**
 * The file on disk does not parse. With a song open the studio keeps playing the last good
 * version; `unopened` means there is none yet.
 */
export function FileErrorBanner({
  errors,
  unopened = false,
}: {
  errors: ErrorView[];
  unopened?: boolean;
}) {
  const n = errors.length;
  const count = `${n} ${n === 1 ? "error" : "errors"}`;
  const headline = unopened
    ? n > 0
      ? `song.toml has ${count}, so it cannot be opened.`
      : "song.toml cannot be opened."
    : `song.toml has ${count}. Still playing the last good version.`;
  return (
    <div role="alert" className={cn(bar, "border-slide/40 bg-slide/10 text-silkscreen")}>
      <p className="font-label text-base">{headline}</p>
      <ul className="flex flex-col gap-0.5 tabular-nums text-silkscreen/85">
        {errors.map((e, i) => (
          <li key={i}>{position(e)}</li>
        ))}
      </ul>
    </div>
  );
}

/** The last failed command or studio error, until dismissed. */
export function MessageBanner({ message, onDismiss }: { message: string; onDismiss(): void }) {
  return (
    <div role="status" className={cn(bar, "flex-row items-center gap-3 border-border bg-muted")}>
      <p className="text-silkscreen">{message}</p>
      <button
        type="button"
        onClick={onDismiss}
        className="rounded-sm border border-border px-2 py-0.5 font-label text-sm text-silkscreen focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen"
      >
        Dismiss
      </button>
    </div>
  );
}
