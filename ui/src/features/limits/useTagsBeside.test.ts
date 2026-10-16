import { afterEach, describe, expect, it, vi } from "vitest";
import { fitsBeside } from "./useTagsBeside";

function element(sizes: { clientWidth?: number; scrollWidth?: number; width?: number } = {}) {
  const node = document.createElement("div");
  Object.defineProperty(node, "clientWidth", { value: sizes.clientWidth ?? 999 });
  Object.defineProperty(node, "scrollWidth", { value: sizes.scrollWidth ?? 999 });
  node.getBoundingClientRect = () => ({ width: sizes.width ?? 999 }) as DOMRect;
  return node;
}

function header({ line, name, chips, lineGap, tagsGap }: { line: number; name: number; chips: number[]; lineGap: number; tagsGap: number }) {
  const lineNode = element({ clientWidth: line });
  const nameNode = element({ scrollWidth: name });
  const tagsNode = element();
  for (const width of chips) tagsNode.append(element({ width }));
  vi.spyOn(window, "getComputedStyle").mockImplementation(
    (node) => ({ columnGap: node === lineNode ? `${lineGap}px` : node === tagsNode ? `${tagsGap}px` : "normal" }) as CSSStyleDeclaration,
  );
  return fitsBeside(lineNode, nameNode, tagsNode);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("fitsBeside", () => {
  it.each([
    { case: "fits with exactly one pixel to spare", line: 300, name: 143, chips: [80, 60], lineGap: 10, tagsGap: 6, beside: true },
    { case: "a pixel more of name does not fit", line: 300, name: 144, chips: [80, 60], lineGap: 10, tagsGap: 6, beside: false },
    { case: "the line's gap alone tips it over", line: 300, name: 140, chips: [80, 60], lineGap: 14, tagsGap: 6, beside: false },
    { case: "the tags' gap alone tips it over", line: 300, name: 140, chips: [80, 60], lineGap: 10, tagsGap: 10, beside: false },
    { case: "a lone tag adds no gap of its own", line: 300, name: 143, chips: [146], lineGap: 10, tagsGap: 6, beside: true },
    { case: "a third tag adds its width and one more gap", line: 300, name: 120, chips: [80, 60, 20], lineGap: 10, tagsGap: 6, beside: false },
    { case: "no gaps reported reads as none", line: 300, name: 150, chips: [80, 60], lineGap: Number.NaN, tagsGap: Number.NaN, beside: true },
  ])("$case", ({ line, name, chips, lineGap, tagsGap, beside }) => {
    expect(header({ line, name, chips, lineGap, tagsGap })).toBe(beside);
  });

  it("treats a header without tags as fitting", () => {
    expect(header({ line: 10, name: 400, chips: [], lineGap: 10, tagsGap: 6 })).toBe(true);
  });
});
