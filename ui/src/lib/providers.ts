import catalogue from "../../../src-tauri/src/provider_catalogue/fixtures/providers.json";
import type { AgentId } from "./types";

export type LimitsProvider = Extract<AgentId, "claude" | "codex" | "muse">;
export type AccountProvider = Extract<AgentId, "claude" | "codex">;

type CatalogueRow = {
  readonly displayName: string;
  readonly binary: string;
  readonly binaryAliases: readonly string[];
  readonly limitsReader: boolean;
  readonly usage: string | null;
  readonly savedAccounts: boolean;
};

const ROWS: Readonly<Record<AgentId, CatalogueRow>> = catalogue.providers;

export function isAgentId(value: unknown): value is AgentId {
  return typeof value === "string" && Object.hasOwn(ROWS, value);
}

export function hasLimits(id: AgentId): id is LimitsProvider {
  return ROWS[id].limitsReader;
}

export function hasSavedAccounts(id: AgentId): id is AccountProvider {
  return ROWS[id].savedAccounts;
}

export const PROVIDER_IDS: readonly AgentId[] = catalogue.tabOrder.filter(isAgentId);

export const RAIL_ORDER: readonly AgentId[] = catalogue.railOrder.filter(isAgentId);

export const LIMITS_PROVIDERS: readonly LimitsProvider[] = PROVIDER_IDS.filter(hasLimits);

export const ACCOUNT_PROVIDERS: readonly AccountProvider[] = PROVIDER_IDS.filter(hasSavedAccounts);

export const USAGE_PROVIDERS: readonly AgentId[] = PROVIDER_IDS.filter((id) => ROWS[id].usage !== null);

export function providerLabel(id: AgentId): string {
  return ROWS[id].displayName;
}

export function providerBinary(id: AgentId): string {
  return ROWS[id].binary;
}

export function providerNames(ids: readonly AgentId[]): string {
  const names = ids.map(providerLabel);
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} or ${names.at(-1)}` : names.join("");
}

export function providerRecord<T>(value: (id: AgentId) => T): Record<AgentId, T> {
  return Object.fromEntries(PROVIDER_IDS.map((id) => [id, value(id)])) as Record<AgentId, T>;
}
