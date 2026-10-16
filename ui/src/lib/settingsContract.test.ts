import { describe, expect, it } from "vitest";
import rustDefaultSettings from "../../../src-tauri/src/settings/fixtures/app-settings.default.json";
import rustSettings from "../../../src-tauri/src/settings/fixtures/app-settings.json";
import rustNotchSettings from "../../../src-tauri/src/side_notch/model/fixtures/notch-settings.json";
import { DEFAULT_APP_SETTINGS, defaultResetAlert } from "./appSettings";
import { defaultNotchSettings, type NotchSettings } from "./notchTypes";
import type { AppSettings, ResetAlert } from "./types";

type SameKeys<A, B> = [keyof A] extends [keyof B] ? ([keyof B] extends [keyof A] ? true : false) : false;

const typedKeys: [
  SameKeys<AppSettings, typeof rustSettings>,
  SameKeys<ResetAlert, (typeof rustSettings.resetAlerts)["acct-a"]>,
  SameKeys<NotchSettings, typeof rustNotchSettings>,
  SameKeys<NotchSettings["pullRequests"], typeof rustNotchSettings.pullRequests>,
] = [true, true, true, true];

function keys(document: object): string[] {
  return Object.keys(document).sort();
}

describe("the settings documents Rust writes", () => {
  it("have the keys the UI's types and defaults have, and no other", () => {
    expect(typedKeys).toEqual([true, true, true, true]);
    expect(keys(DEFAULT_APP_SETTINGS)).toEqual(keys(rustSettings));
    expect(keys(defaultResetAlert(null))).toEqual(keys(rustSettings.resetAlerts["acct-a"]));
    expect(keys(defaultNotchSettings())).toEqual(keys(rustNotchSettings));
    expect(keys(defaultNotchSettings().pullRequests)).toEqual(keys(rustNotchSettings.pullRequests));
  });

  it("start the UI's app settings from Rust's own defaults", () => {
    expect(DEFAULT_APP_SETTINGS).toEqual(rustDefaultSettings);
  });
});
