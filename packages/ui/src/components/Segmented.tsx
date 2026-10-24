import { memo, type KeyboardEvent } from "react";
import { cn } from "../lib/utils";
import type { Highlight } from "./Knob";

export type SegmentedProps = {
  /** The radiogroup's accessible name. */
  label: string;
  options: { value: string; label: string }[];
  value: string;
  disabled?: boolean;
  /** Claude (cyan ring) or an outside edit (dashed ring) just changed the value. */
  highlight?: Highlight;
  onChange(v: string): void;
};

/** A radiogroup of buttons with a roving tabindex; arrow keys move and select, wrapping. */
export const Segmented = memo(function Segmented({
  label,
  options,
  value,
  disabled = false,
  highlight = null,
  onChange,
}: SegmentedProps) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const step =
      e.key === "ArrowDown" || e.key === "ArrowRight"
        ? 1
        : e.key === "ArrowUp" || e.key === "ArrowLeft"
          ? -1
          : 0;
    if (step === 0 || options.length === 0) return;
    e.preventDefault();
    const i = options.findIndex((o) => o.value === value);
    const next = options[(i + step + options.length) % options.length]!.value;
    onChange(next);
    e.currentTarget
      .querySelectorAll<HTMLButtonElement>("[data-value]")
      .forEach((b) => b.dataset.value === next && b.focus());
  };
  return (
    <div
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled ? "true" : undefined}
      data-highlight={highlight ?? undefined}
      onKeyDown={onKeyDown}
      className={cn(
        "inline-flex gap-0.5 rounded-rs border border-line bg-ink/[.04] p-0.5",
        highlight === "claude" && "ring-2 ring-agent",
        highlight === "external" && "outline-dashed outline-2 outline-ink2",
        disabled && "opacity-40",
      )}
    >
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            disabled={disabled}
            tabIndex={on ? 0 : -1}
            data-value={o.value}
            onClick={() => !on && onChange(o.value)}
            className={cn(
              "h-6 rounded-[7px] px-2.5 font-sans text-[11px] font-semibold leading-none",
              "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-play",
              "disabled:cursor-not-allowed",
              on ? "bg-ink/[.14] text-ink" : "text-ink2 hover:text-ink",
            )}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
});
