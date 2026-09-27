import { useLayoutEffect, useState } from "react";

/**
 * Focus handed on when an action takes away the control that holds it, as archiving, unarchiving or
 * removing an account unmounts the button the user pressed. Focus moves only while it is still where
 * the user left it, so an action that resolves late never pulls focus from wherever the user went.
 */

/** Focus is on the page itself: nothing, the body, or an element that has left the document. */
function onPage(element: Element | null): boolean {
  return !element || element === document.body || !element.isConnected;
}

/**
 * Notes the focused element before an action that may take it away. `unmoved()` answers afterwards
 * whether focus is still on it, or fell to the page because it was disabled or removed.
 */
export function holdFocus(): () => boolean {
  const held = document.activeElement;
  return () => document.activeElement === held || onPage(document.activeElement);
}

/**
 * Hands focus on once an item of `keys` has gone. Call `handOff(key, detail)` when the action that
 * removes the item has gone through. In the first commit without `key`, if focus fell to the page
 * with it, focus goes to the first of `targets(index, detail)` that takes it; `index` is where the
 * item was, so `keys[index]` is now the item after it and `keys[index - 1]` the one before.
 */
export function useFocusHandoff<T>(
  keys: readonly string[],
  targets: (index: number, detail: T) => Array<HTMLElement | null | undefined>,
): (key: string, detail: T) => void {
  const [pending, setPending] = useState<{ key: string; index: number; detail: T } | null>(null);
  useLayoutEffect(() => {
    if (!pending || keys.includes(pending.key)) return;
    setPending(null);
    if (!onPage(document.activeElement)) return;
    for (const target of targets(pending.index, pending.detail)) {
      target?.focus();
      if (target && document.activeElement === target) return;
    }
  });
  return (key, detail) => {
    const index = keys.indexOf(key);
    if (index >= 0) setPending({ key, index, detail });
  };
}
