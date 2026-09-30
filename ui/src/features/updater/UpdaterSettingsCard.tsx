import { RefreshCw } from "lucide-react";
import { CardToggle, SettingRow, SettingsCard, cardButton, cardButtonPrimary, rowLabel } from "@/components/SettingsCard";
import { useUpdater } from "./UpdateProvider";

type UpdaterSettingsCardProps = {
  automaticUpdates: boolean;
  onAutomaticUpdatesChange: (enabled: boolean) => void;
};

export function UpdaterSettingsCard({
  automaticUpdates,
  onAutomaticUpdatesChange,
}: UpdaterSettingsCardProps) {
  const updater = useUpdater();
  const busy =
    updater.state.status === "checking" ||
    updater.state.status === "downloading" ||
    updater.state.status === "installing";
  const canCheck = updater.buildInfo?.enabled !== false;

  return (
    <SettingsCard
      title="Application updates"
      meta={
        <>
          Installed <span>{updater.currentVersion ?? "loading…"}</span> · <span>Stable</span>
          {updater.buildInfo?.installerKind ? ` · ${updater.buildInfo.installerKind.toUpperCase()}` : ""}
        </>
      }
      control={
        <CardToggle
          caption="Auto-download"
          on={automaticUpdates}
          ariaLabel="Automatically download updates"
          onToggle={() => onAutomaticUpdatesChange(!automaticUpdates)}
        />
      }
    >
      <SettingRow>
        <div className={rowLabel} aria-live="polite">
          <UpdateStatus />
        </div>
        {updater.state.status === "error" ? (
          <button
            type="button"
            className={cardButton}
            aria-label="Retry update check"
            disabled={!canCheck || busy}
            onClick={() => void updater.checkNow()}
          >
            Retry
          </button>
        ) : null}
        {updater.state.status === "ready" ? (
          <button
            type="button"
            className={cardButtonPrimary}
            onClick={() => void updater.install()}
          >
            Install and restart
          </button>
        ) : null}
        {updater.state.status !== "error" && updater.state.status !== "ready" ? (
          <button
            type="button"
            className={cardButton}
            disabled={!canCheck || busy}
            onClick={() => void updater.checkNow()}
          >
            <RefreshCw className={`size-3 ${busy ? "animate-spin" : ""}`} aria-hidden="true" />
            Check now
          </button>
        ) : null}
      </SettingRow>
    </SettingsCard>
  );
}

function UpdateStatus() {
  const updater = useUpdater();
  const state = updater.state;
  if (state.status === "error") {
    return <span className="text-[var(--trip)]">{state.message}</span>;
  }
  if (!updater.buildInfo) {
    return <>Loading update configuration…</>;
  }
  if (!updater.buildInfo.enabled) {
    return <>Update checks are available in installed release builds.</>;
  }
  switch (state.status) {
    case "idle":
      return <>Ready to check for updates.</>;
    case "checking":
      return <>Checking for updates…</>;
    case "upToDate":
      return <>Up to date</>;
    case "downloading": {
      const progress =
        state.contentLength && state.contentLength > 0
          ? ` · ${Math.round((state.downloaded / state.contentLength) * 100)}%`
          : "";
      return (
        <>
          Downloading {state.update.version}
          {progress}
        </>
      );
    }
    case "ready":
      return (
        <>
          Version {state.update.version} is downloaded and verified.
          {state.update.body ? (
            <details className="mt-1">
              <summary className="cursor-pointer">Release notes</summary>
              <p className="mb-0 whitespace-pre-wrap">{state.update.body}</p>
            </details>
          ) : null}
        </>
      );
    case "installing":
      return <>Starting installer…</>;
  }
}
