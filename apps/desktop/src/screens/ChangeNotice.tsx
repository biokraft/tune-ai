import { cn, IconButton } from "@tune/ui";
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

/** Whether the notice has anything to say. */
export function hasChangeNotice(notice: ExternalChange | null, offered: Change[]): boolean {
  return notice !== null || offered.length > 0;
}

/**
 * The change toast, floating top-right under the transport: "Claude changed 2 things: … Undo",
 * and the re-apply offer for overridden local edits. Cyan marks only Claude's own changes.
 */
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

  // A replaced file and a bare re-apply offer are file events, whoever wrote them.
  const claude = notice?.source === "claude" && !notice.replaced;

  const button = (label: string, onClick: () => void, aria = label, agent = false) => (
    <IconButton
      key={label}
      label={aria}
      size="sm"
      onClick={onClick}
      className={cn(agent && "border-agent/45 text-agent")}
    >
      {label}
    </IconButton>
  );
  const resolve = [
    button("Re-apply", () => onResolve(true)),
    button("Discard", () => onResolve(false)),
  ];
  const dismiss = (
    <IconButton key="dismiss" label="Dismiss" size="sm" onClick={onDismiss}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M6 6l12 12M18 6L6 18" />
      </svg>
    </IconButton>
  );

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
              claude,
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
    <div
      role="status"
      aria-label={claude ? "Claude's change" : "Song file change"}
      data-source={claude ? "claude" : "file"}
      className={cn(
        "glass-solid absolute right-2 top-full z-40 mt-2 flex w-[360px] flex-col gap-2 rounded-r p-3 text-ink",
        claude ? "border-agent" : "border-line2",
      )}
      style={
        claude
          ? {
              boxShadow:
                "0 0 22px rgb(var(--agent) / .22), inset 0 1px 0 rgba(255, 255, 255, .06), 0 10px 30px rgba(0, 0, 0, .35)",
            }
          : undefined
      }
    >
      <p className="flex items-center gap-2 text-[11.5px] font-bold">
        <span
          aria-hidden="true"
          className={cn(
            "h-2 w-2 shrink-0 rounded-full",
            claude ? "bg-agent shadow-[0_0_8px_rgb(var(--agent))]" : "border border-ink2",
          )}
        />
        <span className={claude ? "text-agent" : "text-ink2"}>
          {claude ? "Claude" : "Song file"}
        </span>
      </p>
      <p className="text-[12.5px] leading-snug">{text}</p>
      {offer && <p className="text-[12.5px] leading-snug text-ink2">{offer}</p>}
      <div className="flex flex-wrap justify-end gap-1.5">{actions}</div>
    </div>
  );
}
