import type { AgentId } from "./types";

/** Accent per provider for bars and dots: Claude's orange, Codex in the ink colour. */
const PROVIDER_COLOR: Record<AgentId, string> = {
  codex: "var(--silkscreen)",
  claude: "#e8944a",
  antigravity: "var(--mute)",
  cursor: "#7aa2ff",
};

export function providerColor(provider: AgentId): string {
  return PROVIDER_COLOR[provider];
}
