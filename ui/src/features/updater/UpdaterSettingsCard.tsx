import { RefreshCw } from "lucide-react";
import { Rocker } from "@/features/agents/Rocker";
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
    <section
      aria-label="Application updates"
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
    >
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        <div className="min-w-0 flex-1">
          <h3 className="m-0 text-[15px] font-semibold">Application updates</h3>
          <p className="mt-1 mb-0 font-mono text-[11.5px] text-[var(--mute)]">
            Installed <span>{updater.currentVersion ?? "loading…"}</span> · <span>Stable</span>
            {updater.buildInfo?.installerKind ? ` · ${updater.buildInfo.installerKind.toUpperCase()}` : ""}
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <span className="text-[9.5px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase">
            Auto-download
          </span>
          <Rocker
            size="skill"
            on={automaticUpdates}
            ariaLabel="Automatically download updates"
            onToggle={() => onAutomaticUpdatesChange(!automaticUpdates)}
          />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2.5">
        <div className="min-w-0 flex-1 text-[12px] text-[var(--mute)]" aria-live="polite">
          <UpdateStatus />
        </div>
        {updater.state.status === "error" ? (
          <button
            type="button"
            className="h-8 rounded-md border border-[var(--hair)] px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase"
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
            className="h-8 rounded-md border border-[var(--fill)] bg-[var(--fill)] px-2.5 text-[10px] font-semibold tracking-[0.04em] text-[var(--fill-ink)] uppercase"
            onClick={() => void updater.install()}
          >
            Install and restart
          </button>
        ) : null}
        {updater.state.status !== "error" && updater.state.status !== "ready" ? (
          <button
            type="button"
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[var(--hair)] px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase disabled:opacity-45"
            disabled={!canCheck || busy}
            onClick={() => void updater.checkNow()}
          >
            <RefreshCw className={`size-3 ${busy ? "animate-spin" : ""}`} aria-hidden="true" />
            Check now
          </button>
        ) : null}
      </div>
    </section>
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
