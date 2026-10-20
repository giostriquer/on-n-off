import { describe, expect, it } from "vitest";
import { accountAction, loadAppSettings, onSharedReadChanged, readAccounts, readLimits, removeLimitsAccount, saveAppSettings, setLimitsAccountArchived } from "$lib/api";
import type { AppSettings, SharedReadChanged } from "$lib/types";

describe("the dev mock", () => {
  it("removes and archives an account through the one command each sends, announcing the Limits read and the account list only when each changed, as Rust does", async () => {
    window.history.replaceState(null, "", "/?mock=archivedAccounts&latency=0");
    await import("./mockIpc");
    const heard: SharedReadChanged[] = [];
    const stop = await onSharedReadChanged(change => heard.push(change));
    const since = () => heard.splice(0).map(change => change.source);

    await removeLimitsAccount("codex", "codex-history");
    expect((await readLimits("codex")).map(entry => entry.account?.id)).toEqual(["codex-1", "codex-2"]);
    expect(since(), "history alone: no saved login or category changed").toEqual(["limits:codex"]);
    await accountAction("codex", "use", "work");
    expect((await readAccounts("codex")).profiles.map(profile => profile.id), "only removal drops a saved login").toEqual(["personal", "work"]);
    await removeLimitsAccount("claude", "profile:claude-unread");
    expect((await readAccounts("claude")).profiles.map(profile => profile.id)).toEqual(["personal", "work"]);
    expect(since(), "a saved login goes from the vault").toEqual(["limits:claude", "accounts"]);
    await setLimitsAccountArchived("claude", "claude-2", false);
    expect((await readLimits("claude")).filter(entry => entry.archived), "nothing of Claude's is archived any more").toEqual([]);
    expect(since()).toEqual(["limits:claude", "accounts"]);
    await setLimitsAccountArchived("claude", "claude-2", false);
    await removeLimitsAccount("codex", "codex-history");
    expect(since(), "nothing changed, so nothing is announced").toEqual([]);
    await stop();
    await setLimitsAccountArchived("claude", "claude-2", true);

    expect(since(), "no listener is left").toEqual([]);
    expect((await readLimits("claude")).filter(entry => entry.archived).map(entry => entry.account?.id)).toEqual(["claude-2"]);
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
