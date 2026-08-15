import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import type { UpdaterClient } from "./updaterClient";
import { tauriUpdaterClient } from "./tauriUpdaterClient";
import { UpdateController, type UpdaterSnapshot } from "./updaterController";

const UPDATE_INTERVAL_MS = 24 * 60 * 60 * 1_000;

type UpdateContextValue = UpdaterSnapshot & {
  checkNow: () => Promise<void>;
  dismiss: () => void;
  install: () => Promise<void>;
};

const UpdateContext = createContext<UpdateContextValue | null>(null);

type UpdateProviderProps = {
  children: ReactNode;
  initialProviderReady: boolean;
  automaticUpdates: boolean;
  client?: UpdaterClient;
};

export function UpdateProvider({
  children,
  initialProviderReady,
  automaticUpdates,
  client = tauriUpdaterClient,
}: UpdateProviderProps) {
  const [controller] = useState(() => new UpdateController(client));
  const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot, controller.getSnapshot);

  useEffect(() => {
    if (initialProviderReady) {
      void controller.initialize(automaticUpdates);
    }
  }, [automaticUpdates, controller, initialProviderReady]);

  useEffect(() => {
    if (!initialProviderReady || !automaticUpdates) {
      return;
    }
    const interval = window.setInterval(() => void controller.checkNow(), UPDATE_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [automaticUpdates, controller, initialProviderReady]);

  useEffect(
    () => () => {
      void controller.dispose();
    },
    [controller],
  );

  const value = useMemo<UpdateContextValue>(
    () => ({
      ...snapshot,
      checkNow: () => controller.checkNow(),
      dismiss: () => controller.dismiss(),
      install: () => controller.install(),
    }),
    [controller, snapshot],
  );

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>;
}

export function useUpdater(): UpdateContextValue {
  const context = useContext(UpdateContext);
  if (!context) {
    throw new Error("useUpdater must be used within UpdateProvider");
  }
  return context;
}

export { tauriUpdaterClient };
