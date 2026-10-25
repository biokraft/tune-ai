import { cn } from "@tune/ui";
import type { ReactNode } from "react";
import type { ErrorView } from "../client/protocol";

/** A full-width solid strip above the transport. Errors carry the kick colour, never cyan. */
const bar =
  "glass-solid flex w-full flex-none gap-3 rounded-rs border-kick/50 px-4 py-2.5 text-ink";

/** The kick-coloured dot that marks a strip as an error. */
function ErrorDot() {
  return (
    <span
      aria-hidden="true"
      className="mt-[5px] h-[7px] w-[7px] flex-none self-start rounded-full bg-kick shadow-[0_0_8px_rgb(var(--kick)/.7)]"
    />
  );
}

function Strip({
  role,
  className,
  children,
}: {
  role: "alert" | "status";
  className?: string;
  children: ReactNode;
}) {
  return (
    <div role={role} className={cn(bar, className)}>
      <ErrorDot />
      {children}
    </div>
  );
}

/** The WebSocket closed; nothing reconnects it. */
export function LostBanner() {
  return (
    <Strip role="alert">
      <p>
        Lost connection to the studio. Restart <code className="font-mono text-xs">tune serve</code>
        , then reload this page.
      </p>
    </Strip>
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
    <Strip role="alert">
      <div className="flex min-w-0 flex-col gap-1">
        <p className="font-semibold">{headline}</p>
        <ul className="flex max-h-[30vh] flex-col gap-0.5 overflow-auto font-mono text-xs tabular-nums text-ink2">
          {errors.map((e, i) => (
            <li key={i}>{position(e)}</li>
          ))}
        </ul>
      </div>
    </Strip>
  );
}

/** The last failed command or studio error, until dismissed. */
export function MessageBanner({ message, onDismiss }: { message: string; onDismiss(): void }) {
  return (
    <Strip role="status" className="items-center">
      <p className="min-w-0 flex-1">{message}</p>
      <button
        type="button"
        onClick={onDismiss}
        className="flex-none rounded-rs border border-line2 px-2.5 py-0.5 text-xs font-semibold text-ink hover:bg-ink/[.06] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play"
      >
        Dismiss
      </button>
    </Strip>
  );
}
