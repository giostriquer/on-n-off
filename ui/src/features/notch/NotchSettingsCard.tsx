import { Monitor, RefreshCw } from "lucide-react";
import { displayError, parseInvokeError } from "$lib/error";
import type { NotchSettings } from "$lib/notchTypes";
import { useNotchState } from "./useNotchState";
import "./side-notch.css";

export function NotchSettingsCard() {
  const state = useNotchState();
  if (state.data?.supported === false) return null;
  const settings = state.data?.settings;
  const displays = state.data?.displays ?? [];
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
          {displays.map((display, index) => (
            <option
              key={display.id}
              value={display.id}
              disabled={display.mirrored}
            >
              {index + 1}. {display.name} · {Math.round(display.width)} ×{" "}
              {Math.round(display.height)}
              {display.mirrored ? " · mirrored" : ""}
            </option>
          ))}
        </select>
        <div className="notch-display-map" aria-hidden="true">
          {displays.map((display, index) => (
            <span
              key={display.id}
              className={display.id === settings?.displayId ? "selected" : ""}
            >
              <Monitor size={38} strokeWidth={1.4} />
              <small>{index + 1}</small>
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
            Refreshes every minute while visible. Overlays the screen; choose
            the edge opposite your Dock.
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
