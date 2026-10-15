// A long-text box that keeps what she types to itself (F1a). A controlled <textarea> re-renders
// the whole screen around it on every key, and under a Hebrew IME in WebView2 that stalls the
// typing. Here the text lives in the box (and a ref); the screen hears about it rarely: the text
// itself, debounced (and at once on blur and when the box goes away), and only the moments it
// turns empty or not empty, or starts or stops differing from where it began.
import { useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type FocusEvent, type Ref, type TextareaHTMLAttributes } from "react";

/** How long typing must pause before `onText` hears the text. */
export const TEXT_DEBOUNCE_MS = 800;

export interface LocalTextHandle {
  /** What is in the box right now (never a debounced copy). */
  value(): string;
  /** Put text in the box from code (a sent message cleared, a suggestion inserted). Reports it at once. */
  set(text: string): void;
  focus(): void;
}

type Native = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "defaultValue" | "onChange" | "onInput" | "children">;

export type LocalTextProps = Native & {
  ref?: Ref<LocalTextHandle>;
  /** The text the box starts with. Read once, when the box is created: to start it again from a
   *  new text, give it a new `key`. Later changes to `initial` alone are ignored, so that the
   *  screen re-rendering never resets what she typed. */
  initial?: string | undefined;
  /** The text, after typing pauses for TEXT_DEBOUNCE_MS, and at once on blur and when the box is
   *  removed (if anything is still unreported). For consumers that may lag: holding unsaved text,
   *  keeping a draft. Save and send read `value()` from the handle instead. */
  onText?: ((text: string) => void) | undefined;
  /** The text on every change, at once. Only for what costs nothing per key (keeping the text
   *  held for a lock, without calling the core); anything heavier listens to `onText`. */
  onLive?: ((text: string) => void) | undefined;
  /** Only when the box turns empty (blank or spaces only) or stops being empty. It starts as
   *  `!initial.trim()`, which the screen already knows. */
  onEmptyChange?: ((empty: boolean) => void) | undefined;
  /** Only when the text starts or stops differing from `initial`. */
  onEditedChange?: ((edited: boolean) => void) | undefined;
  /** Fit the box to its text (rows or height), when it opens and after every change. */
  resize?: ((box: HTMLTextAreaElement) => void) | undefined;
};

export function LocalText({ ref, initial = "", onText, onLive, onEmptyChange, onEditedChange, resize, onBlur, ...rest }: LocalTextProps) {
  const [first] = useState(initial);
  const box = useRef<HTMLTextAreaElement>(null);
  const live = useRef(first);
  const wasEmpty = useRef(first.trim() === "");
  const wasEdited = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // The latest callbacks, so a change in the screen's handlers never re-creates anything here.
  const latest = useRef({ onText, onLive, onEmptyChange, onEditedChange, resize });
  useLayoutEffect(() => {
    latest.current = { onText, onLive, onEmptyChange, onEditedChange, resize };
  });

  /** Report the text now, if a report is still waiting. */
  const flush = useCallback(() => {
    if (timer.current === null) return;
    clearTimeout(timer.current);
    timer.current = null;
    latest.current.onText?.(live.current);
  }, []);

  const take = useCallback((next: string, now: boolean) => {
    live.current = next;
    latest.current.onLive?.(next);
    const empty = next.trim() === "";
    if (empty !== wasEmpty.current) {
      wasEmpty.current = empty;
      latest.current.onEmptyChange?.(empty);
    }
    const edited = next !== first;
    if (edited !== wasEdited.current) {
      wasEdited.current = edited;
      latest.current.onEditedChange?.(edited);
    }
    if (box.current) latest.current.resize?.(box.current);
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    if (now) {
      latest.current.onText?.(next);
      return;
    }
    timer.current = setTimeout(() => {
      timer.current = null;
      latest.current.onText?.(live.current);
    }, TEXT_DEBOUNCE_MS);
  }, [first]);

  useImperativeHandle(ref, () => ({
    value: () => box.current?.value ?? live.current,
    set: (next: string) => {
      if (box.current) box.current.value = next;
      take(next, true);
    },
    focus: () => box.current?.focus(),
  }), [take]);

  // Fitted before the first paint; and what is still unreported is reported when the box goes.
  useLayoutEffect(() => {
    if (box.current) latest.current.resize?.(box.current);
  }, []);
  useEffect(() => flush, [flush]);

  return (
    <textarea {...rest} ref={box} defaultValue={first}
      onChange={(e) => take(e.currentTarget.value, false)}
      onBlur={(e: FocusEvent<HTMLTextAreaElement>) => {
        flush();
        onBlur?.(e);
      }} />
  );
}
