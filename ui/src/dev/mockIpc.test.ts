import { describe, expect, it } from "vitest";
import { accountAction, forgetLimitsSnapshot, loadAppSettings, readAccounts, readLimits, saveAppSettings } from "$lib/api";
import type { AppSettings } from "$lib/types";

describe("the dev mock", () => {
  it("removes an account through the commands Remove account sends", async () => {
    window.history.replaceState(null, "", "/?mock=archivedAccounts&latency=0");
    await import("./mockIpc");

    await forgetLimitsSnapshot("codex", "codex-history");
    expect((await readLimits("codex")).map(entry => entry.account?.id)).toEqual(["codex-1", "codex-2"]);
    await accountAction("codex", "use", "work");
    expect((await readAccounts("codex")).profiles.map(profile => profile.id), "only remove drops a saved login").toEqual(["personal", "work"]);
    await accountAction("claude", "remove", "unread");
    expect((await readAccounts("claude")).profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
  });

  it("saves the app settings as Rust does, keeping only the keys Rust's document has", async () => {
    window.history.replaceState(null, "", "/?mock=ok&latency=0");
    await import("./mockIpc");
    const loaded = await loadAppSettings();

    const saved = await saveAppSettings({ ...loaded, closeToTray: true, uiOnlySetting: "dropped" } as AppSettings);

    expect(saved).toEqual({ ...loaded, closeToTray: true });
    expect(await loadAppSettings()).toEqual(saved);
  });
});
