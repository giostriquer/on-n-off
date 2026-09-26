import { SCENARIOS } from "./githubFixtures";
import { LIMITS_SCENARIOS } from "./limitsFixtures";

/** Scenarios `mockIpc.ts` answers for itself, beside the pull-request and Limits tables. */
const LOCAL_SCENARIOS = ["accountLogin", "accountLocked", "accountClients", "catalog", "hooks", "mcpSources"];

/**
 * Why `?mock=<name>` names no scenario, or null when it names one. The dev mock logs it as an error,
 * so a mistyped name fails the screenshot harness instead of quietly answering as `ok`.
 */
export function unknownScenario(name: string): string | null {
  const known = [...Object.keys(SCENARIOS), ...Object.keys(LIMITS_SCENARIOS), ...LOCAL_SCENARIOS];
  return known.includes(name) ? null : `[mock] unknown scenario "${name}"; known: ${known.join(", ")}`;
}
