import { describe, expect, it } from "vitest";
import { accountAction, loadAppSettings, onSharedReadChanged, readAccounts, readLimits, removeLimitsAccount, saveAppSettings, setLimitsAccountArchived } from "$lib/api";
import type { AppSettings, SharedReadChanged } from "$lib/types";

describe("the dev mock", () => {
  it("removes and archives an account through the one command each sends, and announces what changed as Rust does", async () => {
    window.history.replaceState(null, "", "/?mock=archivedAccounts&latency=0");
    await import("./mockIpc");
    const heard: SharedReadChanged[] = [];
    const stop = await onSharedReadChanged(change => heard.push(change));

    await removeLimitsAccount("codex", "codex-history");
    expect((await readLimits("codex")).map(entry => entry.account?.id)).toEqual(["codex-1", "codex-2"]);
    await accountAction("codex", "use", "work");
    expect((await readAccounts("codex")).profiles.map(profile => profile.id), "only removal drops a saved login").toEqual(["personal", "work"]);
    await removeLimitsAccount("claude", "profile:claude-unread");
    expect((await readAccounts("claude")).profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
    await setLimitsAccountArchived("claude", "claude-2", false);
    expect((await readLimits("claude")).filter(entry => entry.archived), "nothing of Claude's is archived any more").toEqual([]);
    await stop();
    await setLimitsAccountArchived("claude", "claude-2", true);

    expect(heard.map(change => change.source)).toEqual(["limits:codex", "limits:claude", "accounts", "limits:claude", "accounts"]);
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
