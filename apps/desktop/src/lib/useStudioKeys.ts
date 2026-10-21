import { useEffect, useRef } from "react";

export type StudioKeys = {
  playStop(): void;
  undo(): void;
  redo(): void;
  loop(): void;
};

const TYPING = 'input, textarea, select, [contenteditable=""], [contenteditable="true"]';
const POPOVER = '[data-radix-popper-content-wrapper], [role="dialog"], [role="menu"]';

/** Whether a key event belongs to something else: a text field, a popover, or a handler. */
function ignored(e: KeyboardEvent): boolean {
  if (e.defaultPrevented) return true;
  const t = e.target;
  if (!(t instanceof Element)) return false;
  if (t instanceof HTMLElement && t.isContentEditable) return true;
  return t.closest(TYPING) !== null || t.closest(POPOVER) !== null;
}

const isSpace = (e: KeyboardEvent) => e.code === "Space" || e.key === " ";

/**
 * Window-wide shortcuts: Space play/stop, Cmd+Z undo, Shift+Cmd+Z redo, L loop the selected
 * section. Controls that handle a key themselves stop it or prevent its default first.
 */
export function useStudioKeys(keys: StudioKeys) {
  const ref = useRef(keys);
  ref.current = keys;

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (ignored(e)) return;
      const k = ref.current;
      const mod = e.metaKey || e.ctrlKey;
      if (isSpace(e) && !mod && !e.altKey) {
        // Also keeps Space from pressing a focused button.
        e.preventDefault();
        if (!e.repeat) k.playStop();
      } else if (mod && !e.altKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) k.redo();
        else k.undo();
      } else if (!mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        if (!e.repeat) k.loop();
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
