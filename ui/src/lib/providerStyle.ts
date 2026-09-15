import type { AgentId } from "./types";

/** Accent per provider for bars and dots: Claude's brand terracotta, Codex in the ink colour.
 * The side notch paints the same value, so one quota reads the same colour on every surface. */
const PROVIDER_COLOR: Record<AgentId, string> = {
  codex: "var(--silkscreen)",
  claude: "#d97757",
  antigravity: "var(--mute)",
  cursor: "#7aa2ff",
};

export function providerColor(provider: AgentId): string {
  return PROVIDER_COLOR[provider];
}
