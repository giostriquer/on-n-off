import { GitPullRequest, RefreshCw } from "lucide-react";
import { ALL_AGENTS } from "$lib/appSettings";
import { displayError, parseInvokeError } from "$lib/error";
import { GITHUB_LIST_IDS, type GithubListId } from "$lib/githubTypes";
import type { NotchDisplay, NotchEdge, NotchSettings } from "$lib/notchTypes";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { AgentId } from "$lib/types";
import { providerLabel } from "$lib/usageMerge";
import { Rocker } from "@/components/Rocker";
import { Segmented } from "@/components/Segmented";
import { SettingRow, SettingsCard, caption, cardIconButton, cardSelect, rowLabel, rowName } from "@/components/SettingsCard";
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

/** Adds or removes one entry, keeping `order`'s sequence and refusing to remove the last one. */
function toggleOrdered<T>(order: readonly T[], selected: readonly T[], id: T, shown: boolean): T[] {
  const next = new Set(selected);
  if (shown) next.add(id);
  else if (next.size > 1) next.delete(id);
  return order.filter((entry) => next.has(entry));
}

/** Toggles one provider's cell, keeping rail order and refusing to remove the last one. */
export function toggleNotchProvider(
  providers: readonly AgentId[],
  id: AgentId,
  shown: boolean,
): AgentId[] {
  return toggleOrdered(ALL_AGENTS, providers, id, shown);
}

/** Toggles one pull-request list, keeping screen order and refusing to remove the last one. */
export function toggleNotchList(
  lists: readonly GithubListId[],
  id: GithubListId,
  shown: boolean,
): GithubListId[] {
  return toggleOrdered(GITHUB_LIST_IDS, lists, id, shown);
}

const LIST_LABEL: Record<GithubListId, string> = {
  mine: "Mine",
  reviewRequested: "Review requested",
  assigned: "Assigned",
};

const SHOW_OPTIONS: [NotchShowChoice, string][] = [
  ["always", "Always show"],
  ["hover", "Show on hover"],
  ["hide", "Hide"],
];

const EDGE_OPTIONS: [NotchEdge, string][] = [
  ["right", "Right"],
  ["left", "Left"],
  ["top", "Top"],
  ["bottom", "Bottom"],
];

export function NotchSettingsCard() {
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
    <SettingsCard
      label="Side notch settings"
      title="Side notch"
      description="Usage rings at the edge of one display, with details on hover. macOS and Windows 11."
    >
      <SettingRow>
        <span id="notch-show-label" className={rowLabel}>Show</span>
        <Segmented
          ariaLabelledBy="notch-show-label"
          options={SHOW_OPTIONS.map(([value, label]) => ({ value, label, disabled: value !== "hide" && !canShow }))}
          pressed={(value) => choice === value}
          onPress={(value) => change(showPatch(value))}
          disabled={busy}
        />
      </SettingRow>
      <SettingRow>
        <span id="notch-edge-label" className={rowLabel}>Edge</span>
        <Segmented
          ariaLabelledBy="notch-edge-label"
          options={EDGE_OPTIONS.map(([value, label]) => ({ value, label }))}
          pressed={(value) => settings?.edge === value}
          onPress={(edge) => change({ edge })}
          disabled={busy}
        />
      </SettingRow>
      <SettingRow stack>
        <div className="flex items-center gap-3">
          <label htmlFor="notch-display" className={rowLabel}>Display</label>
          <button
            type="button"
            className={cardIconButton}
            aria-label="Refresh displays"
            disabled={state.isFetching}
            onClick={() => void state.refetch()}
          >
            <RefreshCw className="size-3.5" aria-hidden="true" />
          </button>
        </div>
        <select
          id="notch-display"
          className={`${cardSelect} w-full`}
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
      </SettingRow>
      <SettingRow>
        <span id="notch-size-label" className={rowLabel}>Size</span>
        <Segmented
          ariaLabelledBy="notch-size-label"
          options={(["compact", "standard", "large"] as const).map((size) => ({
            value: size,
            label: size[0].toUpperCase() + size.slice(1),
          }))}
          pressed={(size) => settings?.size === size}
          onPress={(size) => change({ size })}
          disabled={busy}
        />
      </SettingRow>
      <SettingRow>
        <h4 id="notch-providers-label" className={`m-0 ${caption}`}>Integrations</h4>
      </SettingRow>
      <ul className="m-0 list-none p-0" aria-labelledby="notch-providers-label">
        {ALL_AGENTS.map((id) => {
          const shown = settings?.providers.includes(id) ?? false;
          const last = shown && cells === 1;
          return (
            <li key={id}>
              <SettingRow>
                <ProviderIcon provider={id} className="size-4 shrink-0" title="" />
                <span className={rowName}>{providerLabel(id)}</span>
                <Rocker
                  size="skill"
                  on={shown}
                  disabled={busy || last}
                  ariaLabel={`Show ${providerLabel(id)} in the notch`}
                  onToggle={() =>
                    settings &&
                    change({ providers: toggleNotchProvider(settings.providers, id, !shown) })
                  }
                />
              </SettingRow>
            </li>
          );
        })}
        <li>
          <SettingRow>
            <GitPullRequest className="size-4 shrink-0" aria-hidden="true" />
            <span className={rowName}>Pull requests</span>
            {settings?.pullRequests.enabled && (
              <Segmented
                ariaLabel="Pull request lists"
                options={GITHUB_LIST_IDS.map((list) => ({
                  value: list,
                  label: LIST_LABEL[list],
                  disabled: settings.pullRequests.lists.includes(list) && settings.pullRequests.lists.length === 1,
                }))}
                pressed={(list) => settings.pullRequests.lists.includes(list)}
                onPress={(list) =>
                  change({
                    pullRequests: {
                      ...settings.pullRequests,
                      lists: toggleNotchList(
                        settings.pullRequests.lists,
                        list,
                        !settings.pullRequests.lists.includes(list),
                      ),
                    },
                  })
                }
                disabled={busy}
              />
            )}
            <Rocker
              size="skill"
              on={settings?.pullRequests.enabled ?? false}
              disabled={busy || (settings?.pullRequests.enabled === true && cells === 1)}
              ariaLabel="Show pull requests in the notch"
              onToggle={() =>
                settings &&
                change({
                  pullRequests: { ...settings.pullRequests, enabled: !settings.pullRequests.enabled },
                })
              }
            />
          </SettingRow>
        </li>
      </ul>
      {message && (
        <SettingRow>
          <p role="alert" className="m-0 text-[12px] text-[var(--trip)]">
            {message}
          </p>
        </SettingRow>
      )}
    </SettingsCard>
  );
}
