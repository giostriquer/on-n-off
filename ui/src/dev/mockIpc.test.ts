import { describe, expect, it } from "vitest";
import { accountAction, forgetLimitsSnapshot, readAccounts, readLimits } from "$lib/api";

describe("the dev mock", () => {
  it("removes an account through the commands Remove account sends", async () => {
    window.history.replaceState(null, "", "/?mock=archivedAccounts&latency=0");
    await import("./mockIpc");

    await forgetLimitsSnapshot("codex", "codex-history");
    expect((await readLimits("codex")).map(entry => entry.account?.id)).toEqual(["codex-1", "codex-2"]);
    await accountAction("codex", "category", "work", "Client B");
    await accountAction("codex", "use", "work");
    expect((await readAccounts("codex")).profiles.map(profile => profile.id), "only remove drops a saved login").toEqual(["personal", "work"]);
    await accountAction("claude", "remove", "unread");
    expect((await readAccounts("claude")).profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
  });
});
