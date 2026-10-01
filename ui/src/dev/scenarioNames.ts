import { SCENARIOS } from "./githubFixtures";
import { LIMITS_SCENARIOS } from "./limitsFixtures";

const LOCAL_SCENARIOS = ["accountLogin", "accountLocked", "accountClients", "catalog", "hooks", "mcpSources"];

export function unknownScenario(name: string): string | null {
  const known = [...Object.keys(SCENARIOS), ...Object.keys(LIMITS_SCENARIOS), ...LOCAL_SCENARIOS];
  return known.includes(name) ? null : `[mock] unknown scenario "${name}"; known: ${known.join(", ")}`;
}
