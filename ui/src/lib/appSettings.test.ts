import { describe, expect, it } from "vitest";
import { visibleAgentIds, setAgentHidden, resetSpendLimit, withBinaryPath } from "./appSettings";

describe("appSettings", () => {
  it("defaults to every provider visible, in tab order", () => {
    expect(visibleAgentIds([])).toEqual(["claude", "codex", "antigravity", "cursor", "muse"]);
  });

  it("hides a provider from tabs without emptying the list", () => {
    expect(visibleAgentIds(["antigravity"])).toEqual(["claude", "codex", "cursor", "muse"]);
  });

  it("refuses to hide the last visible provider", () => {
    const hidden = setAgentHidden(["codex", "antigravity", "cursor", "muse"], "claude", true);
    expect(hidden).toEqual(["codex", "antigravity", "cursor", "muse"]);
    expect(visibleAgentIds(hidden)).toEqual(["claude"]);
  });

  it("can show a hidden provider again", () => {
    expect(setAgentHidden(["antigravity"], "antigravity", false)).toEqual([]);
  });

  it("sets a provider's binary path trimmed and drops one left blank", () => {
    const paths = { claude: "/opt/acme/bin/claude" };

    expect(withBinaryPath(paths, "codex", "  /opt/acme/bin/codex ")).toEqual({
      claude: "/opt/acme/bin/claude",
      codex: "/opt/acme/bin/codex",
    });
    expect(withBinaryPath(paths, "claude", "   ")).toEqual({});
    expect(paths).toEqual({ claude: "/opt/acme/bin/claude" });
  });

  it("warns before, and spends automatically only at, 5% left or an account's lower share", () => {
    const alerts = {
      low: { label: null, maxLeftPercent: 4, minHoursToRenewal: 24, automatic: false },
      high: { label: null, maxLeftPercent: 40, minHoursToRenewal: 24, automatic: false },
    };

    expect(resetSpendLimit(alerts, "none")).toBe(5);
    expect(resetSpendLimit(alerts, "low")).toBe(4);
    expect(resetSpendLimit(alerts, "high")).toBe(5);
  });
});
