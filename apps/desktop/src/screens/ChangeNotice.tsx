import { cn } from "@tune/ui";
import type { Change, ExternalChange, SongView } from "../client/protocol";
import { describeChanges } from "../lib/describeChange";

export type ChangeNoticeProps = {
  /** The last change on disk, until dismissed. */
  notice: ExternalChange | null;
  /** The studio's offered edits (state.offered): the source of truth once it arrives. */
  offered: Change[];
  /** Whether Undo still reverts this change (no local edit has landed on top of it). */
  canUndo: boolean;
  song: SongView | null;
  onUndo(): void;
  onResolve(reapply: boolean): void;
  onDismiss(): void;
};

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

const btn = cn(
  "h-7 rounded-sm border border-claude/60 px-2 font-label text-sm text-claude",
  "hover:bg-claude/10",
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-silkscreen",
);

/** Whether the notice has anything to say. */
export function hasChangeNotice(notice: ExternalChange | null, offered: Change[]): boolean {
  return notice !== null || offered.length > 0;
}

/** "Claude changed 2 things: … Undo", and the re-apply offer for overridden local edits. */
export function ChangeNotice({
  notice,
  offered,
  canUndo,
  song,
  onUndo,
  onResolve,
  onDismiss,
}: ChangeNoticeProps) {
  // The state's list is current; the event's covers the moment before that state arrives.
  const pending = offered.length > 0 ? offered.length : (notice?.offered.length ?? 0);
  if (!notice && pending === 0) return null;

  const button = (label: string, onClick: () => void, aria?: string, icon = false) => (
    <button
      key={label}
      type="button"
      aria-label={aria}
      className={cn(btn, icon && "w-7 px-0")}
      onClick={onClick}
    >
      {label}
    </button>
  );
  const resolve = [
    button("Re-apply", () => onResolve(true)),
    button("Discard", () => onResolve(false)),
  ];
  const dismiss = button("×", onDismiss, "Dismiss", true);

  let text: string;
  let offer: string | null = null;
  let actions: JSX.Element[];
  if (notice?.replaced) {
    text =
      pending > 0
        ? `The song file was replaced. Re-apply ${plural(pending, "unsaved edit")}?`
        : "The song file was replaced.";
    actions = pending > 0 ? resolve : [dismiss];
  } else if (notice) {
    const who = notice.source === "claude" ? "Claude" : "The song file";
    text = `${who} changed ${plural(notice.changes.length, "thing")}: ${describeChanges(notice.changes, song)}`;
    if (pending > 0) offer = `It overrode ${plural(pending, "unsaved edit")}. Re-apply?`;
    actions = [
      // Named apart from the transport's own Undo.
      ...(canUndo
        ? [
            button(
              "Undo",
              onUndo,
              notice.source === "claude" ? "Undo Claude's change" : "Undo the change on disk",
            ),
          ]
        : []),
      ...(pending > 0 ? resolve : []),
      dismiss,
    ];
  } else {
    text = `A change on disk overrode ${plural(pending, "unsaved edit")}. Re-apply ${pending === 1 ? "it" : "them"}?`;
    actions = resolve;
  }

  return (
    <div className="flex min-w-0 items-center gap-2">
      <span
        aria-hidden="true"
        className={cn(
          "h-2 w-2 shrink-0 rounded-full bg-claude",
          notice?.source === "external" && "bg-transparent ring-1 ring-claude",
        )}
      />
      <span className="min-w-0 truncate" title={text}>
        {text}
      </span>
      {offer && <span className="shrink-0">{offer}</span>}
      <span className="flex shrink-0 gap-1">{actions}</span>
    </div>
  );
}
