import { RefreshCw } from "lucide-react";
import { displayError, parseInvokeError } from "$lib/error";
import type { NotchDisplay, NotchSettings } from "$lib/notchTypes";
import type { LimitsPollMinutes } from "$lib/types";
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
          <p>Usage at the edge of one display. macOS only.</p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={settings?.enabled ?? false}
          aria-label="Enable side notch"
          className="notch-switch"
          disabled={
            !settings ||
            state.saving ||
            (!settings.enabled && (!selected || selected.mirrored))
          }
          onClick={() => change({ enabled: !settings?.enabled })}
        >
          <span />
        </button>
      </header>
      <div className="notch-settings-body">
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
          disabled={!settings || state.saving}
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
        <div className="notch-size-row">
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
                disabled={!settings || state.saving}
                aria-pressed={settings?.size === size}
                onClick={() => change({ size })}
              >
                {size[0].toUpperCase() + size.slice(1)}
              </button>
            ))}
          </div>
        </div>
        <div className="notch-edge-row">
          <span id="notch-edge-label">Edge</span>
          <div
            role="group"
            aria-labelledby="notch-edge-label"
            className="notch-segment"
          >
            {(["left", "right"] as const).map((edge) => (
              <button
                key={edge}
                type="button"
                disabled={!settings || state.saving}
                aria-pressed={settings?.edge === edge}
                onClick={() => change({ edge })}
              >
                {edge === "left" ? "Left" : "Right"}
              </button>
            ))}
          </div>
        </div>
        <p className="notch-help">
          Only on this display. Hidden while disconnected or mirrored.
        </p>
        <p className="notch-help">
          Refreshes with all usage surfaces every {pollMinutes} minutes. Overlays
          the screen; choose the edge opposite your Dock.
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
