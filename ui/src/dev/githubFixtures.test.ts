import { expect, it } from "vitest";
import { standingFor } from "../features/github/prStandingCases";
import { SCENARIOS } from "./githubFixtures";

const replies = Object.entries(SCENARIOS).filter(([name]) => name !== "loading");
const REMEMBERED = new Set(["stale"]);

it.each(replies)("sends %s with the standing the read sends for its status", async (name, reply) => {
  const { status, standing } = await reply();
  const hint = standing && standing.kind !== "live" ? standing.message.text : "";
  expect(standing).toEqual(standingFor(status, REMEMBERED.has(name), hint));
});
