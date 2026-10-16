import { describe, expect, it, vi } from "vitest";
import { DEFAULT_APP_SETTINGS } from "./appSettings";
import { createAppSettingsDocument, type AppSettingsTransport } from "./appSettingsDocument";
import type { AppSettings } from "./types";

const LOADED: AppSettings = { ...DEFAULT_APP_SETTINGS, githubScopes: ["org:acme"], limitsPollMinutes: 10 };

type Deferred<T> = { promise: Promise<T>; resolve: (value: T) => void };

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}

function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function rust(loaded: AppSettings = LOADED) {
  const transport = {
    load: vi.fn<AppSettingsTransport["load"]>(async () => loaded),
    save: vi.fn<AppSettingsTransport["save"]>(async (settings) => settings),
  };
  return { transport, document: createAppSettingsDocument(transport) };
}

describe("the app settings document", () => {
  it("holds the default settings as a placeholder until Rust's document loads", async () => {
    const { document } = rust();
    const changed = vi.fn();
    document.subscribe(changed);

    expect(document.state()).toEqual({ settings: DEFAULT_APP_SETTINGS, loaded: false });
    await expect(document.load()).resolves.toEqual({ settings: LOADED, loaded: true });

    expect(document.state()).toEqual({ settings: LOADED, loaded: true });
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("shares a load already under way", async () => {
    const { transport, document } = rust();

    const [first, second] = await Promise.all([document.load(), document.load()]);

    expect(first).toEqual({ settings: LOADED, loaded: true });
    expect(second).toEqual({ settings: LOADED, loaded: true });
    expect(transport.load).toHaveBeenCalledTimes(1);
    await document.load();
    expect(transport.load).toHaveBeenCalledTimes(2);
  });

  it("keeps what it holds when a load fails, and says whether that is a document Rust sent", async () => {
    const { transport, document } = rust();
    transport.load.mockRejectedValueOnce(new Error("settings unavailable"));

    await expect(document.load()).resolves.toEqual({ settings: DEFAULT_APP_SETTINGS, loaded: false });
    expect(document.state()).toEqual({ settings: DEFAULT_APP_SETTINGS, loaded: false });

    await document.load();
    transport.load.mockRejectedValueOnce(new Error("settings unavailable"));
    await expect(document.load()).resolves.toEqual({ settings: LOADED, loaded: true });
    expect(document.state()).toEqual({ settings: LOADED, loaded: true });
  });

  it("answers with what it holds when a read throws before it starts", async () => {
    const { transport, document } = rust();
    transport.load.mockImplementationOnce(() => {
      throw new Error("no IPC");
    });

    await expect(document.load()).resolves.toEqual({ settings: DEFAULT_APP_SETTINGS, loaded: false });
    await expect(document.load()).resolves.toEqual({ settings: LOADED, loaded: true });
  });

  it("saves a change over the whole document and holds the document Rust saved", async () => {
    const { transport, document } = rust();
    await document.load();
    const normalised = { ...LOADED, closeToTray: true, githubScopes: ["org:acme", "repo:acme/webapp"] };
    transport.save.mockResolvedValueOnce(normalised);

    await expect(document.patch({ closeToTray: true, githubScopes: ["org:acme", "acme/webapp"] })).resolves.toEqual(normalised);

    expect(transport.save).toHaveBeenCalledWith({ ...LOADED, closeToTray: true, githubScopes: ["org:acme", "acme/webapp"] });
    expect(document.state()).toEqual({ settings: normalised, loaded: true });
  });

  it("passes a setting it has no type for through a save, as Rust sent it", async () => {
    const fromRust = { ...LOADED, limitsPollMinutes: 30, futureSetting: { level: 2 } } as AppSettings;
    const { transport, document } = rust(fromRust);

    await document.load();
    expect(document.state().settings).toEqual(fromRust);
    await document.patch({ closeToTray: true });

    expect(transport.save).toHaveBeenCalledWith({ ...fromRust, closeToTray: true });
    expect(document.state().settings).toEqual({ ...fromRust, closeToTray: true });
  });

  it("applies a second change to the document the first save returned", async () => {
    const { transport, document } = rust();
    await document.load();
    const first = deferred<AppSettings>();
    const second = deferred<AppSettings>();
    transport.save.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const closing = document.patch({ closeToTray: true, githubScopes: ["org:acme", "acme/webapp"] });
    const updating = document.patch({ automaticUpdates: false });
    await settle();
    expect(transport.save).toHaveBeenCalledTimes(1);

    const savedFirst = { ...LOADED, closeToTray: true, githubScopes: ["org:acme", "repo:acme/webapp"] };
    first.resolve(savedFirst);
    await expect(closing).resolves.toEqual(savedFirst);
    await vi.waitFor(() => expect(transport.save).toHaveBeenCalledTimes(2));
    expect(transport.save.mock.calls[1][0]).toEqual({ ...savedFirst, automaticUpdates: false });

    second.resolve({ ...savedFirst, automaticUpdates: false });
    await expect(updating).resolves.toEqual({ ...savedFirst, automaticUpdates: false });
    expect(document.state().settings).toEqual({ ...savedFirst, automaticUpdates: false });
  });

  it("hands an updater the document the save before it returned", async () => {
    const { transport, document } = rust();
    await document.load();
    const first = deferred<AppSettings>();
    transport.save.mockReturnValueOnce(first.promise);

    void document.patch((latest) => ({ hiddenAgents: [...latest.hiddenAgents, "cursor"] }));
    const hiding = document.patch((latest) => ({ hiddenAgents: [...latest.hiddenAgents, "muse"] }));
    await settle();
    first.resolve({ ...LOADED, hiddenAgents: ["cursor"] });
    await hiding;

    expect(transport.save.mock.calls.map(([sent]) => sent.hiddenAgents)).toEqual([["cursor"], ["cursor", "muse"]]);
  });

  it("reads the document again only once a save under way has answered", async () => {
    const { transport, document } = rust();
    await document.load();
    const saving = deferred<AppSettings>();
    transport.save.mockReturnValueOnce(saving.promise);
    const afterSave = { ...LOADED, closeToTray: true, limitsPollMinutes: 15 } satisfies AppSettings;

    const patched = document.patch({ closeToTray: true });
    const reread = document.load();
    await settle();
    expect(transport.load).toHaveBeenCalledTimes(1);

    transport.load.mockResolvedValueOnce(afterSave);
    saving.resolve({ ...LOADED, closeToTray: true });
    await patched;
    await expect(reread).resolves.toEqual({ settings: afterSave, loaded: true });
    expect(transport.load).toHaveBeenCalledTimes(2);
    expect(document.state().settings).toEqual(afterSave);
  });

  it("applies a change made before the document has loaded to Rust's document, never to the placeholder", async () => {
    const { transport, document } = rust();

    await expect(document.patch({ closeToTray: true })).resolves.toEqual({ ...LOADED, closeToTray: true });

    expect(transport.load).toHaveBeenCalledTimes(1);
    expect(transport.save.mock.calls).toEqual([[{ ...LOADED, closeToTray: true }]]);
  });

  it("waits for a load under way before applying a change", async () => {
    const { transport, document } = rust();
    const reading = deferred<AppSettings>();
    transport.load.mockReturnValueOnce(reading.promise);

    void document.load();
    const patched = document.patch({ closeToTray: true });
    await settle();
    expect(transport.save).not.toHaveBeenCalled();
    reading.resolve(LOADED);
    await patched;

    expect(transport.load).toHaveBeenCalledTimes(1);
    expect(transport.save.mock.calls).toEqual([[{ ...LOADED, closeToTray: true }]]);
  });

  it("reads the document again before a change when the first load failed, and saves nothing when it still cannot", async () => {
    const { transport, document } = rust();
    transport.load.mockRejectedValueOnce(new Error("settings unavailable"));
    await document.load();

    await document.patch({ closeToTray: true });
    expect(transport.save.mock.calls).toEqual([[{ ...LOADED, closeToTray: true }]]);

    const unreadable = rust();
    const unavailable = new Error("settings unavailable");
    unreadable.transport.load.mockRejectedValue(unavailable);
    const failed = vi.fn();
    unreadable.document.onSaveFailed(failed);
    await unreadable.document.load();

    await expect(unreadable.document.patch({ closeToTray: true })).resolves.toBeNull();
    expect(unreadable.transport.save).not.toHaveBeenCalled();
    expect(failed.mock.calls).toEqual([[unavailable]]);
    expect(unreadable.document.state()).toEqual({ settings: DEFAULT_APP_SETTINGS, loaded: false });
  });

  it("hands an updater the document to change", async () => {
    const { transport, document } = rust({ ...LOADED, hiddenAgents: ["cursor"] });
    await document.load();

    await document.patch((latest) => ({ hiddenAgents: [...latest.hiddenAgents, "muse"] }));

    expect(transport.save).toHaveBeenCalledWith({ ...LOADED, hiddenAgents: ["cursor", "muse"] });
  });

  it("reports a failed save once to every listener, answers null and keeps the document", async () => {
    const { transport, document } = rust();
    await document.load();
    const refusal = { kind: "message", message: "Keep at least one provider visible in the agent tabs.", path: null };
    transport.save.mockRejectedValueOnce(refusal);
    const session = vi.fn();
    const gone = vi.fn();
    document.onSaveFailed(session);
    document.onSaveFailed(gone)();

    await expect(document.patch({ hiddenAgents: ["claude", "codex", "antigravity", "cursor", "muse"] })).resolves.toBeNull();

    expect(session.mock.calls).toEqual([[refusal]]);
    expect(gone).not.toHaveBeenCalled();
    expect(document.state()).toEqual({ settings: LOADED, loaded: true });
  });

  it("stops telling a listener that unsubscribed", async () => {
    const { document } = rust();
    const changed = vi.fn();
    document.subscribe(changed)();

    await document.load();

    expect(changed).not.toHaveBeenCalled();
  });
});
