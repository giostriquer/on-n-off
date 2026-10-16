import { DEFAULT_APP_SETTINGS } from "./appSettings";
import type { AppSettings } from "./types";

export type AppSettingsChange = Partial<AppSettings> | ((latest: AppSettings) => Partial<AppSettings>);

export type AppSettingsTransport = {
  load: () => Promise<AppSettings>;
  save: (settings: AppSettings) => Promise<AppSettings>;
};

export type AppSettingsState = {
  settings: AppSettings;
  loaded: boolean;
};

export type AppSettingsDocument = {
  state: () => AppSettingsState;
  subscribe: (listener: () => void) => () => void;
  load: () => Promise<AppSettings>;
  patch: (change: AppSettingsChange) => Promise<AppSettings | null>;
  onSaveFailed: (listener: (error: unknown) => void) => () => void;
};

export function createAppSettingsDocument(transport: AppSettingsTransport): AppSettingsDocument {
  let current: AppSettingsState = { settings: DEFAULT_APP_SETTINGS, loaded: false };
  let reading: Promise<AppSettings> | null = null;
  let tail: Promise<unknown> = Promise.resolve();
  const listeners = new Set<() => void>();
  const failureListeners = new Set<(error: unknown) => void>();

  function hold(settings: AppSettings) {
    current = { settings, loaded: true };
    for (const listener of listeners) listener();
  }

  function inTurn<T>(work: () => Promise<T>): Promise<T> {
    const turn = tail.then(work);
    tail = turn.catch(() => undefined);
    return turn;
  }

  function load(): Promise<AppSettings> {
    reading ??= inTurn(() =>
      transport
        .load()
        .then(hold, () => undefined)
        .then(() => current.settings),
    ).finally(() => {
      reading = null;
    });
    return reading;
  }

  function patch(change: AppSettingsChange): Promise<AppSettings | null> {
    return inTurn(async () => {
      const latest = current.settings;
      const saved = await transport.save({ ...latest, ...(typeof change === "function" ? change(latest) : change) });
      hold(saved);
      return saved;
    }).catch((error: unknown) => {
      for (const listener of failureListeners) listener(error);
      return null;
    });
  }

  return {
    state: () => current,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    load,
    patch,
    onSaveFailed: (listener) => {
      failureListeners.add(listener);
      return () => {
        failureListeners.delete(listener);
      };
    },
  };
}
