import { GITHUB_STATUSES, type GithubStatus, type PrReadStanding, type PrStandingMessage } from "$lib/githubTypes";
import standings from "../../../../src-tauri/src/github/standing/fixtures/standing-cases.json";
import { exactly } from "../../testing/exactly";
import type { PrBanner } from "./prStanding";

export type PrStandingCase = {
  name: string;
  status: GithubStatus;
  hint: string | null;
  stale: boolean;
  standing: PrReadStanding;
  screen: { banner: PrBanner | null; lastRead: boolean };
};

function githubStatus(value: unknown): GithubStatus {
  const status = GITHUB_STATUSES.find((known) => known === value);
  if (!status) throw new Error(`${JSON.stringify(value)} is not a status the UI names`);
  return status;
}

function standingMessage(value: unknown): PrStandingMessage {
  const { text, tone } = exactly(value, ["text", "tone"]);
  if (typeof text !== "string" || (tone !== "error" && tone !== "warning")) throw new Error(`${JSON.stringify(value)} is not a message`);
  return { text, tone };
}

function prReadStanding(value: unknown): PrReadStanding {
  const { kind } = value as { kind?: unknown };
  if (kind === "live") {
    exactly(value, ["kind"]);
    return { kind };
  }
  if (kind === "lastObserved" || kind === "unavailable") {
    return { kind, message: standingMessage(exactly(value, ["kind", "message"]).message) };
  }
  throw new Error(`${JSON.stringify(value)} is not a read standing the UI names`);
}

function banner(value: unknown): PrBanner | null {
  if (value === null) return null;
  const { headline, hint, tone } = exactly(value, ["headline", "hint", "tone"]);
  if (typeof headline !== "string" || typeof hint !== "string" || (tone !== "trip" && tone !== "warn")) throw new Error(`${JSON.stringify(value)} is not a banner`);
  return { headline, hint, tone };
}

function prStandingCase(value: unknown): PrStandingCase {
  const { name, status, hint, stale, standing, screen } = exactly(value, ["name", "status", "hint", "stale", "rows", "standing", "screen", "notch"]);
  const { banner: shown, lastRead } = exactly(screen, ["banner", "lastRead"]);
  if (typeof name !== "string" || (hint !== null && typeof hint !== "string") || typeof stale !== "boolean" || typeof lastRead !== "boolean") {
    throw new Error(`${JSON.stringify(value)} is not a pull request read standing case`);
  }
  return { name, status: githubStatus(status), hint, stale, standing: prReadStanding(standing), screen: { banner: banner(shown), lastRead } };
}

export const PR_STANDING_CASES: readonly PrStandingCase[] = standings.cases.map(prStandingCase);

export function standingFor(status: GithubStatus, stale: boolean, hint: string): PrReadStanding | undefined {
  const row = PR_STANDING_CASES.find((sent) => sent.status === status && sent.stale === stale);
  if (!row) throw new Error(`no ${status} case${stale ? " over the last read" : ""}`);
  if (row.standing.kind === "live") return undefined;
  return { kind: row.standing.kind, message: { text: hint, tone: row.standing.message.tone } };
}
