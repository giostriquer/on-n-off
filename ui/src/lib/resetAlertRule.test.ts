import { describe, expect, it } from "vitest";
import rule from "../../../src-tauri/src/reset_alert/fixtures/rule.json";
import {
  CODEX_RESET_MAX_LEFT_PERCENT,
  RESET_ALERT_MAX_HOURS,
  RESET_ALERT_MIN_HOURS,
  RESET_ALERT_MIN_LEFT_PERCENT,
  RESET_AUTO_SPEND_DELAY_MINUTES,
  defaultResetAlert,
  resetSpendLimit,
} from "./appSettings";

describe("the banked reset alert rule Rust keeps", () => {
  it("caps a spend, delays an automatic one and starts a new alert as the UI says", () => {
    expect(CODEX_RESET_MAX_LEFT_PERCENT).toBe(rule.spendCapPercent);
    expect(RESET_AUTO_SPEND_DELAY_MINUTES).toBe(rule.autoSpendDelayMinutes);
    expect(defaultResetAlert(null)).toEqual(rule.defaultAlert);
  });

  it("bounds an alert where the form does", () => {
    expect({
      maxLeftPercent: { min: RESET_ALERT_MIN_LEFT_PERCENT, max: CODEX_RESET_MAX_LEFT_PERCENT },
      minHoursToRenewal: { min: RESET_ALERT_MIN_HOURS, max: RESET_ALERT_MAX_HOURS },
    }).toEqual(rule.bounds);
  });

  it("holds a card's warning to the share an automatic spend is held to", () => {
    for (const [maxLeftPercent, share] of rule.spendShares) {
      const alerts = { acct: { ...defaultResetAlert(null), maxLeftPercent } };
      expect(resetSpendLimit(alerts, "acct")).toBe(share);
    }
    expect(resetSpendLimit({}, "acct")).toBe(rule.spendCapPercent);
  });
});
