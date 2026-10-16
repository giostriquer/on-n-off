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
  const listeners = new Set<() => void>();
  const failureListeners = new Set<(error: unknown) => void>();

  function hold(settings: AppSettings) {
    current = { settings, loaded: true };
    for (const listener of listeners) listener();
  }

  function load(): Promise<AppSettings> {
    reading ??= transport
      .load()
      .then(hold, () => undefined)
      .then(() => current.settings)
      .finally(() => {
        reading = null;
      });
    return reading;
  }

  async function patch(change: AppSettingsChange): Promise<AppSettings | null> {
    const base = current.settings;
    try {
      hold(await transport.save({ ...base, ...(typeof change === "function" ? change(base) : change) }));
      return current.settings;
    } catch (error) {
      for (const listener of failureListeners) listener(error);
      return null;
    }
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
