import { Download } from "lucide-react";
import { useUpdater } from "./UpdateProvider";

export function UpdateStrip() {
  const updater = useUpdater();
  if (updater.state.status !== "ready" || updater.state.dismissed) {
    return null;
  }

  const { update } = updater.state;
  return (
    <section
      role="region"
      aria-label="Update available"
      className="flex shrink-0 flex-wrap items-center gap-3 border-b border-[var(--fill)] bg-[color-mix(in_srgb,var(--fill)_13%,var(--plate))] px-4 py-2.5"
    >
      <Download className="size-4 shrink-0 text-[var(--fill)]" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="m-0 text-[13px] font-semibold">on-n-off {update.version} is ready</p>
        {update.body ? (
          <details className="mt-0.5 text-[11.5px] text-[var(--mute)]">
            <summary className="cursor-pointer">Release notes</summary>
            <p className="mb-0 whitespace-pre-wrap">{update.body}</p>
          </details>
        ) : null}
      </div>
      <button
        type="button"
        className="h-8 rounded-lg border border-[var(--hair)] px-3 text-[11px] font-semibold"
        onClick={updater.dismiss}
      >
        Later
      </button>
      <button
        type="button"
        className="h-8 rounded-lg border border-[var(--fill)] bg-[var(--fill)] px-3 text-[11px] font-semibold text-[var(--fill-ink)]"
        onClick={() => void updater.install()}
      >
        Install and restart
      </button>
    </section>
  );
}
