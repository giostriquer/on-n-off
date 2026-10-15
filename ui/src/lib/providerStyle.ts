import type { AgentId } from "./types";

const PROVIDER_COLOR: Record<AgentId, string> = {
  codex: "var(--silkscreen)",
  claude: "#d97757",
  antigravity: "var(--mute)",
  cursor: "#7aa2ff",
  muse: "#40c4ac",
};

export function providerColor(provider: AgentId): string {
  return PROVIDER_COLOR[provider];
}
