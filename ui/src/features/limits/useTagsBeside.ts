import { useLayoutEffect, useState, type RefObject } from "react";

type Ref = RefObject<HTMLElement | null>;

export function useTagsBeside(line: Ref, name: Ref, tags: Ref): boolean {
  const [beside, setBeside] = useState(true);
  useLayoutEffect(() => {
    setBeside(fitsBeside(line, name, tags));
  });
  useLayoutEffect(() => {
    const node = line.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => setBeside(fitsBeside(line, name, tags)));
    observer.observe(node);
    return () => observer.disconnect();
  }, [line, name, tags]);
  return beside;
}

function fitsBeside(line: Ref, name: Ref, tags: Ref): boolean {
  if (!line.current || !name.current || !tags.current) return true;
  const widths = [...tags.current.children].map((child) => child.getBoundingClientRect().width);
  if (widths.length === 0) return true;
  const tagsWidth = widths.reduce((sum, width) => sum + width, 0) + columnGap(tags.current) * (widths.length - 1);
  return name.current.scrollWidth + columnGap(line.current) + tagsWidth + 1 <= line.current.clientWidth;
}

function columnGap(node: Element): number {
  return parseFloat(getComputedStyle(node).columnGap) || 0;
}
