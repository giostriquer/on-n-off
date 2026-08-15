import { describe, expect, it } from "vitest";
import { router } from "./router";

const featurePaths = [
  "/overview",
  "/plugins",
  "/skills",
  "/mcp",
  "/usage",
  "/config",
  "/settings",
] as const;

describe("router bundle boundaries", () => {
  it("resolves every typed feature path through a preloadable lazy component", () => {
    expect(Object.keys(router.routesByPath).sort()).toEqual([
      "/",
      "/config",
      "/mcp",
      "/overview",
      "/plugins",
      "/settings",
      "/skills",
      "/usage",
    ]);

    for (const path of featurePaths) {
      expect(router.buildLocation({ to: path }).pathname).toBe(path);
      const component = router.routesByPath[path].options.component as
        | { preload?: () => Promise<unknown> }
        | undefined;
      expect(component?.preload).toBeTypeOf("function");
    }
  });

  it("keeps intent preloading enabled", () => {
    expect(router.options.defaultPreload).toBe("intent");
  });
});
