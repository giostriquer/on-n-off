import { GitPullRequest, RefreshCw } from "lucide-react";
import { ALL_AGENTS } from "$lib/appSettings";
import { displayError, parseInvokeError } from "$lib/error";
import { GITHUB_LIST_IDS, type GithubListId } from "$lib/githubTypes";
import type { NotchDisplay, NotchEdge, NotchSettings } from "$lib/notchTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId, LimitsPollMinutes } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { useNotchState } from "./useNotchState";
import "./side-notch.css";

type DisplayLayout = NotchDisplay & {
  order: number;
  left: number;
  top: number;
  layoutWidth: number;
  layoutHeight: number;
};

export function layoutDisplays(displays: NotchDisplay[]): DisplayLayout[] {
  if (displays.length === 0) return [];
  const minX = Math.min(...displays.map((display) => display.x));
  const minY = Math.min(...displays.map((display) => display.y));
  const maxX = Math.max(...displays.map((display) => display.x + display.width));
  const maxY = Math.max(...displays.map((display) => display.y + display.height));
  const desktopWidth = Math.max(maxX - minX, 1);
  const desktopHeight = Math.max(maxY - minY, 1);
  const ordered = [...displays].sort(
    (a, b) => a.x - b.x || a.y - b.y || a.id.localeCompare(b.id),
  );
  return ordered.map((display, index) => ({
    ...display,
    order: index + 1,
    left: ((display.x - minX) / desktopWidth) * 100,
    top: ((display.y - minY) / desktopHeight) * 100,
    layoutWidth: (display.width / desktopWidth) * 100,
    layoutHeight: (display.height / desktopHeight) * 100,
  }));
}

/** The three-way "Show" control folds `enabled` and `show` into one choice. */
export type NotchShowChoice = "always" | "hover" | "hide";

export function showChoice(settings: Pick<NotchSettings, "enabled" | "show">): NotchShowChoice {
  if (!settings.enabled) return "hide";
  return settings.show === "onHover" ? "hover" : "always";
}

export function showPatch(choice: NotchShowChoice): Partial<Pick<NotchSettings, "enabled" | "show">> {
  if (choice === "hide") return { enabled: false };
  return { enabled: true, show: choice === "hover" ? "onHover" : "always" };
}

/** Toggles one provider's cell, keeping rail order and refusing to remove the last one. */
export function toggleNotchProvider(
  providers: readonly AgentId[],
  id: AgentId,
  shown: boolean,
): AgentId[] {
  const next = new Set(providers);
  if (shown) next.add(id);
  else if (next.size > 1) next.delete(id);
  return ALL_AGENTS.filter((agent) => next.has(agent));
}

/** Toggles one pull-request list, keeping screen order and refusing to remove the last one. */
export function toggleNotchList(
  lists: readonly GithubListId[],
  id: GithubListId,
  shown: boolean,
): GithubListId[] {
  const next = new Set(lists);
  if (shown) next.add(id);
  else if (next.size > 1) next.delete(id);
  return GITHUB_LIST_IDS.filter((list) => next.has(list));
}

const LIST_LABEL: Record<GithubListId, string> = {
  mine: "Mine",
  reviewRequested: "Review requested",
  assigned: "Assigned",
};

const SHOW_OPTIONS: [NotchShowChoice, string, string][] = [
  ["always", "Always show", "The rail stays open with every reading visible."],
  ["hover", "Show on hover", "A small pill at the screen edge opens the rail when you reach it."],
  ["hide", "Hide", "Nothing is shown. Your choices are kept for next time."],
];

const EDGE_OPTIONS: [NotchEdge, string, string][] = [
  ["right", "Right", "Down the right-hand edge, clear of a Dock on that side."],
  ["left", "Left", "Down the left-hand edge, clear of a Dock on that side."],
  ["top", "Top", "A wide bar below the menu bar, readings side by side."],
  ["bottom", "Bottom", "A wide bar resting above the Dock, readings side by side."],
];

const PROVIDER_NOTE: Record<AgentId, string> = {
  claude: "Usage rings, quota windows, and live Claude Code sessions.",
  codex: "Usage rings, quota windows, and live Codex sessions.",
  antigravity: "A cell without a meter until Antigravity exposes subscription limits.",
  cursor: "A cell without a meter until Cursor exposes subscription limits.",
};

export function NotchSettingsCard({ pollMinutes = 5 }: { pollMinutes?: LimitsPollMinutes }) {
  const state = useNotchState();
  if (state.data?.supported === false) return null;
  const settings = state.data?.settings;
  const displays = state.data?.displays ?? [];
  const displayLayout = layoutDisplays(displays);
  const selected = displays.find(
    (display) => display.id === settings?.displayId,
  );
  const error = state.saveError ?? state.error;
  const message = error
    ? displayError(parseInvokeError(error), "Side notch")
    : state.data?.error;
  const choice = settings ? showChoice(settings) : "hide";
  const cells = (settings?.providers.length ?? 0) + (settings?.pullRequests.enabled ? 1 : 0);
  const canShow = Boolean(selected && !selected.mirrored && settings && cells > 0);
  const busy = !settings || state.saving;
  function change(patch: Partial<NotchSettings>) {
    if (settings)
      void state.save({ ...settings, ...patch }).catch(() => undefined);
  }
  return (
    <section
      aria-label="Side notch settings"
      className="notch-settings is-page"
    >
      <header>
        <div>
          <h3>Side notch</h3>
          <p>Usage rings at the edge of one display, with details on hover. macOS only.</p>
        </div>
      </header>
      <div className="notch-settings-body">
        <div className="notch-row">
          <span id="notch-show-label">Show</span>
          <div role="group" aria-labelledby="notch-show-label" className="notch-segment">
            {SHOW_OPTIONS.map(([value, label]) => (
              <button
                key={value}
                type="button"
                disabled={busy || (value !== "hide" && !canShow)}
                aria-pressed={choice === value}
                onClick={() => change(showPatch(value))}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
        <p className="notch-help">{SHOW_OPTIONS.find(([value]) => value === choice)?.[2]}</p>
        <div className="notch-row">
          <span id="notch-edge-label">Edge</span>
          <div role="group" aria-labelledby="notch-edge-label" className="notch-segment">
            {EDGE_OPTIONS.map(([value, label]) => (
              <button
                key={value}
                type="button"
                disabled={busy}
                aria-pressed={settings?.edge === value}
                onClick={() => change({ edge: value })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
        <p className="notch-help">
          {EDGE_OPTIONS.find(([value]) => value === settings?.edge)?.[2]}
        </p>
        <div className="notch-label-row">
          <label htmlFor="notch-display">Display</label>
          <button
            type="button"
            className="notch-icon-button"
            aria-label="Refresh displays"
            disabled={state.isFetching}
            onClick={() => void state.refetch()}
          >
            <RefreshCw size={13} />
          </button>
        </div>
        <select
          id="notch-display"
          value={settings?.displayId ?? ""}
          disabled={busy}
          onChange={(event) =>
            change({ displayId: event.target.value || null })
          }
        >
          <option value="" disabled>
            Choose a display
          </option>
          {settings?.displayId && !selected && (
            <option value={settings.displayId}>
              Saved display · disconnected
            </option>
          )}
          {displayLayout.map((display) => (
            <option
              key={display.id}
              value={display.id}
              disabled={display.mirrored}
            >
              {display.order}. {display.name} · {Math.round(display.width)} ×{" "}
              {Math.round(display.height)}
              {display.mirrored ? " · mirrored" : ""}
            </option>
          ))}
        </select>
        <div className="notch-display-map" aria-hidden="true">
          {displayLayout.map((display) => (
            <span
              key={display.id}
              data-display-id={display.id}
              className={display.id === settings?.displayId ? "selected" : ""}
              style={{
                left: `${display.left}%`,
                top: `${display.top}%`,
                width: `${display.layoutWidth}%`,
                height: `${display.layoutHeight}%`,
              }}
            >
              <small>{display.order}</small>
              {display.id === settings?.displayId && (
                <i data-edge={settings.edge} />
              )}
            </span>
          ))}
        </div>
        <div className="notch-row">
          <span id="notch-size-label">Size</span>
          <div
            role="group"
            aria-labelledby="notch-size-label"
            className="notch-segment"
          >
            {(["compact", "standard", "large"] as const).map((size) => (
              <button
                key={size}
                type="button"
                disabled={busy}
                aria-pressed={settings?.size === size}
                onClick={() => change({ size })}
              >
                {size[0].toUpperCase() + size.slice(1)}
              </button>
            ))}
          </div>
        </div>
        <h4 id="notch-providers-label">Integrations</h4>
        <ul className="notch-providers" aria-labelledby="notch-providers-label">
          {ALL_AGENTS.map((id) => {
            const shown = settings?.providers.includes(id) ?? false;
            const last = shown && cells === 1;
            return (
              <li key={id}>
                <ProviderIcon provider={id} className="size-4 shrink-0" title="" />
                <div>
                  <span>{providerLabel(id)}</span>
                  <small>{PROVIDER_NOTE[id]}</small>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={shown}
                  aria-label={`Show ${providerLabel(id)} in the notch`}
                  className="notch-switch"
                  disabled={busy || last}
                  onClick={() =>
                    settings &&
                    change({ providers: toggleNotchProvider(settings.providers, id, !shown) })
                  }
                >
                  <span />
                </button>
              </li>
            );
          })}
          <li>
            <GitPullRequest className="size-4 shrink-0" aria-hidden="true" />
            <div>
              <span>Pull requests</span>
              <small>Open pull requests from the Pull requests screen, with one-click review requests.</small>
              {settings?.pullRequests.enabled && (
                <div
                  role="group"
                  aria-label="Pull request lists"
                  className="notch-segment notch-lists"
                >
                  {GITHUB_LIST_IDS.map((list) => {
                    const shown = settings.pullRequests.lists.includes(list);
                    return (
                      <button
                        key={list}
                        type="button"
                        disabled={busy || (shown && settings.pullRequests.lists.length === 1)}
                        aria-pressed={shown}
                        onClick={() =>
                          change({
                            pullRequests: {
                              ...settings.pullRequests,
                              lists: toggleNotchList(settings.pullRequests.lists, list, !shown),
                            },
                          })
                        }
                      >
                        {LIST_LABEL[list]}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={settings?.pullRequests.enabled ?? false}
              aria-label="Show pull requests in the notch"
              className="notch-switch"
              disabled={busy || (settings?.pullRequests.enabled === true && cells === 1)}
              onClick={() =>
                settings &&
                change({
                  pullRequests: { ...settings.pullRequests, enabled: !settings.pullRequests.enabled },
                })
              }
            >
              <span />
            </button>
          </li>
        </ul>
        <p className="notch-help">
          Only on this display. Hidden while disconnected or mirrored. Refreshes with all usage
          surfaces every {pollMinutes} minutes; live sessions every few seconds. Overlays the
          screen without reserving space.
        </p>
        {message && (
          <p role="alert" className="notch-error">
            {message}
          </p>
        )}
      </div>
    </section>
  );
}
