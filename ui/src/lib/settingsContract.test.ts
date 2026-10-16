import { describe, expect, expectTypeOf, it } from "vitest";
import rustDefaultSettings from "../../../src-tauri/src/settings/fixtures/app-settings.default.json";
import rustSettings from "../../../src-tauri/src/settings/fixtures/app-settings.json";
import rustNotchSettings from "../../../src-tauri/src/side_notch/model/fixtures/notch-settings.json";
import { DEFAULT_APP_SETTINGS, defaultResetAlert } from "./appSettings";
import { defaultNotchSettings, type NotchSettings } from "./notchTypes";
import type { AppSettings, ResetAlert } from "./types";

function keys(document: object): string[] {
  return Object.keys(document).sort();
}

describe("the settings documents Rust writes", () => {
  it("have the keys the UI's types and defaults have, and no other", () => {
    expectTypeOf<keyof AppSettings>().toEqualTypeOf<keyof typeof rustSettings>();
    expectTypeOf<keyof ResetAlert>().toEqualTypeOf<keyof (typeof rustSettings.resetAlerts)["acct-a"]>();
    expectTypeOf<keyof NotchSettings>().toEqualTypeOf<keyof typeof rustNotchSettings>();
    expectTypeOf<keyof NotchSettings["pullRequests"]>().toEqualTypeOf<keyof typeof rustNotchSettings.pullRequests>();
    expect(keys(DEFAULT_APP_SETTINGS)).toEqual(keys(rustSettings));
    expect(keys(defaultResetAlert(null))).toEqual(keys(rustSettings.resetAlerts["acct-a"]));
    expect(keys(defaultNotchSettings())).toEqual(keys(rustNotchSettings));
    expect(keys(defaultNotchSettings().pullRequests)).toEqual(keys(rustNotchSettings.pullRequests));
  });

  it("start the UI's app settings from Rust's own defaults", () => {
    expect(DEFAULT_APP_SETTINGS).toEqual(rustDefaultSettings);
  });
});
