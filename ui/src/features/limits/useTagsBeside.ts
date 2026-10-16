import { useLayoutEffect, useRef, useState } from "react";

export function useTagsBeside() {
  const line = useRef<HTMLDivElement>(null);
  const name = useRef<HTMLDivElement>(null);
  const tags = useRef<HTMLDivElement>(null);
  const [beside, setBeside] = useState(true);
  useLayoutEffect(() => {
    setBeside(fitsBeside(line.current, name.current, tags.current));
  });
  useLayoutEffect(() => {
    const observer = new ResizeObserver(() => setBeside(fitsBeside(line.current, name.current, tags.current)));
    for (const node of [line.current, name.current]) {
      if (node) observer.observe(node);
    }
    return () => observer.disconnect();
  }, []);
  return { beside, line, name, tags };
}

export function fitsBeside(line: HTMLElement | null, name: HTMLElement | null, tags: HTMLElement | null): boolean {
  if (!line || !name || !tags) return true;
  const widths = [...tags.children].map((child) => child.getBoundingClientRect().width);
  if (widths.length === 0) return true;
  const tagsWidth = widths.reduce((sum, width) => sum + width, 0) + columnGap(tags) * (widths.length - 1);
  return name.scrollWidth + columnGap(line) + tagsWidth + 1 <= line.clientWidth;
}

function columnGap(node: Element): number {
  return parseFloat(getComputedStyle(node).columnGap) || 0;
}
