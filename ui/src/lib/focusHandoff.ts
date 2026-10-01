import { useLayoutEffect, useState } from "react";

function onPage(element: Element | null): boolean {
  return !element || element === document.body || !element.isConnected;
}

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
