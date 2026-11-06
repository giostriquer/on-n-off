import agents from "../../../src-tauri/src/capabilities/fixtures/agents.json";
import { providerLabel } from "$lib/providers";
import type { AgentId, AgentInfo, Capabilities, ItemPlaces } from "$lib/types";

type Row = (typeof agents)[AgentId];
const ROWS: Readonly<Record<AgentId, Row>> = agents;

const MUSE_ITEM_ROOTS: ItemPlaces = {
  global: { skills: "~/.config/muse/skills", agents: null },
  project: { skills: ".agents/skills", agents: null },
};

function itemRoots(row: Row): ItemPlaces {
  return "itemRoots" in row ? (row.itemRoots as ItemPlaces) : MUSE_ITEM_ROOTS;
}

export function declaredCapabilities(id: AgentId): Capabilities {
  return ROWS[id].capabilities as Capabilities;
}

export function agentInfo(id: AgentId, cli: "found" | "missing" = "found"): AgentInfo {
  const row = ROWS[id];
  return {
    id,
    displayName: providerLabel(id),
    ...(cli === "found" ? row.cliFound : row.cliMissing),
    capabilities: declaredCapabilities(id),
    itemRoots: itemRoots(row),
  };
}
