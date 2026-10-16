import { createContext, useContext, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import * as api from "./api";
import { createAppSettingsDocument, type AppSettingsDocument } from "./appSettingsDocument";

const AppSettingsContext = createContext<AppSettingsDocument | null>(null);

export function AppSettingsProvider({ children }: { children: ReactNode }) {
  const [appSettings] = useState(() =>
    createAppSettingsDocument({
      load: () => api.loadAppSettings(),
      save: (settings) => api.saveAppSettings(settings),
    }),
  );
  useEffect(() => {
    void appSettings.load();
  }, [appSettings]);
  return <AppSettingsContext.Provider value={appSettings}>{children}</AppSettingsContext.Provider>;
}

export function useAppSettings() {
  const appSettings = useContext(AppSettingsContext);
  if (!appSettings) {
    throw new Error("useAppSettings must be used within AppSettingsProvider");
  }
  const { settings, loaded } = useSyncExternalStore(appSettings.subscribe, appSettings.state, appSettings.state);
  return {
    settings,
    loaded,
    patch: appSettings.patch,
    load: appSettings.load,
    onSaveFailed: appSettings.onSaveFailed,
  };
}
