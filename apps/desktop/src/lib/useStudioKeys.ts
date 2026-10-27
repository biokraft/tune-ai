import { useEffect, useRef } from "react";

export type StudioKeys = {
  playStop(): void;
  undo(): void;
  redo(): void;
  loop(): void;
  /** Open or close the mixer in the dock. */
  mixer(): void;
  /** Close the dock. */
  escape(): void;
};

const TYPING = 'input, textarea, select, [contenteditable=""], [contenteditable="true"]';
const POPOVER = '[data-radix-popper-content-wrapper], [role="dialog"], [role="menu"]';

/**
 * Whether a key event belongs to something else: a text field, a popover (unless `inPopover`),
 * or a handler.
 */
function ignored(e: KeyboardEvent, inPopover = false): boolean {
  if (e.defaultPrevented) return true;
  const t = e.target;
  if (!(t instanceof Element)) return false;
  if (t instanceof HTMLElement && t.isContentEditable) return true;
  return t.closest(TYPING) !== null || (!inPopover && t.closest(POPOVER) !== null);
}

/** Cmd/Ctrl+Z, with or without Shift. */
const isUndoKey = (e: KeyboardEvent) =>
  (e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "z";

const isSpace = (e: KeyboardEvent) => e.code === "Space" || e.key === " ";

/**
 * Window-wide shortcuts: Space play/stop, Cmd+Z undo and Shift+Cmd+Z redo (also inside popovers
 * and menus, never in a text field), L loop the selected
 * section, M the mixer, Esc close the dock. Controls that handle a key themselves stop it or
 * prevent its default first; an open popover closes on Esc itself (Radix prevents its default).
 */
export function useStudioKeys(keys: StudioKeys) {
  const ref = useRef(keys);
  ref.current = keys;

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      // Undo and redo work anywhere but a text field, which has its own.
      if (ignored(e, isUndoKey(e))) return;
      const k = ref.current;
      const mod = e.metaKey || e.ctrlKey;
      if (isSpace(e) && !mod && !e.altKey) {
        // Also keeps Space from pressing a focused button.
        e.preventDefault();
        if (!e.repeat) k.playStop();
      } else if (isUndoKey(e)) {
        e.preventDefault();
        if (e.shiftKey) k.redo();
        else k.undo();
      } else if (!mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        if (!e.repeat) k.loop();
      } else if (!mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        if (!e.repeat) k.mixer();
      } else if (e.key === "Escape" && !mod && !e.altKey && !e.shiftKey) {
        k.escape();
      }
    };
    // Some engines press a focused button on Space's keyup; swallow that too.
    const up = (e: KeyboardEvent) => {
      if (isSpace(e) && !ignored(e)) e.preventDefault();
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);
}
