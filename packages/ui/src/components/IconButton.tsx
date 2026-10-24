import { memo, type ReactNode } from "react";
import { cn } from "../lib/utils";

export type IconButtonProps = {
  /** The accessible name. */
  label: string;
  /** Shortcut hint shown as a key chip. */
  kbd?: string;
  size?: "md" | "sm";
  /** Toggle state; leave undefined for a plain button (no aria-pressed). */
  pressed?: boolean;
  disabled?: boolean;
  onClick(): void;
  children: ReactNode;
  className?: string;
};

export const IconButton = memo(function IconButton({
  label,
  kbd,
  size = "md",
  pressed,
  disabled = false,
  onClick,
  children,
  className,
}: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex flex-none items-center justify-center whitespace-nowrap rounded-rs border font-sans font-semibold text-ink",
        "[&_svg]:flex-none [&_svg]:fill-none [&_svg]:stroke-current [&_svg]:[stroke-linecap:round] [&_svg]:[stroke-linejoin:round] [&_svg]:[stroke-width:1.8]",
        "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-play",
        "disabled:cursor-not-allowed disabled:opacity-40",
        size === "md"
          ? "h-8 min-w-8 gap-1.5 px-[9px] text-[12px] [&_svg]:h-4 [&_svg]:w-4"
          : "h-[26px] min-w-[26px] gap-1.5 px-1.5 text-[11px] [&_svg]:h-[14px] [&_svg]:w-[14px]",
        pressed ? "border-line2 bg-ink/12" : "border-line bg-ink/[.04]",
        !disabled && (pressed ? "hover:bg-ink/[.16]" : "hover:bg-ink/[.09]"),
        className,
      )}
    >
      {children}
      {kbd && <span className="kbd text-ink2">{kbd}</span>}
    </button>
  );
});
