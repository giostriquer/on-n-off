import CoreGraphics
import Foundation
import NotchCore

@MainActor
final class NotchTests {
  let now = Date(timeIntervalSince1970: 1_800_000_000)
  func quota(
    _ kind: String, _ percent: Double, label: String = "Weekly", id: String = "weekly",
    reset: String? = nil
  ) -> Quota {
    Quota(
      id: id, label: label, kind: kind, usedPercent: percent, resetsAt: reset,
      observedAt: "2026-08-31T12:00:00Z")
  }
  func provider(
    _ name: String = "claude", windows: [Quota], status: String = "ok", current: Bool = true
  ) -> Provider {
    Provider(
      provider: name, status: status, currentAccount: current, plan: "max", message: nil,
      windows: windows)
  }
  func display(_ id: String, x: Double = 0, mirrored: Bool = false) -> Display {
    Display(
      id: id, name: "Same name", x: x, y: 0, width: 1728, height: 1117, workY: 33,
      workHeight: 1084, scale: 2, mirrored: mirrored)
  }

  func testClaudeUsesWeeklyAndBothFableAdapterFormsWithoutLosingSessionDetails() {
    for id in ["weekly_fable", "weekly_scoped:Fable"] {
      let entry = provider(windows: [
        quota("session", 73), quota("weekly", 41),
        quota("model", 58, label: "Weekly · Fable", id: id),
      ])
      expectEqual(entry.primary?.usedPercent, 41)
      expectEqual(entry.fable?.usedPercent, 58)
      expectEqual(entry.visibleWindows.first?.usedPercent, 73)
    }
  }

  func testNoWeeklyOrFableSubstitutionFromOtherQuotas() {
    let entry = provider(windows: [
      quota("session", 73), quota("model", 90, label: "Weekly · Opus"),
    ])
    expectNil(entry.primary)
    expectNil(entry.fable)
  }

  func testUnavailableAndRememberedAccountsNeverPopulateCompactRings() {
    for entry in [
      provider(windows: [quota("weekly", 99)], status: "failed"),
      provider(windows: [quota("weekly", 99)], current: false),
    ] {
      expectNil(entry.primary)
      expectEqual(entry.visibleWindows.count, 1)
    }
  }

  func testFableExpiresIndependentlyAndUnknownResetRemainsUsable() {
    let entry = provider(windows: [
      quota("weekly", 41),
      quota("model", 58, label: "Weekly · Fable", reset: "2026-01-01T00:00:00Z"),
    ])
    expectEqual(entry.primary?.percent(at: now), 41)
    expectNil(entry.fable?.percent(at: now))
    expectEqual(quota("weekly", 0.4, reset: "invalid").percent(at: now), 0.4)
    expectNil(quota("weekly", 9, reset: "2027-01-15T08:00:00.000Z").percent(at: now))
  }

  func testCodexPrefersSessionAndHidesInternalBuckets() {
    let entry = provider(
      "codex",
      windows: [
        quota("model", 99, label: "Weekly · GPT-Reserve"),
        quota("model", 80, id: "extra:codex_bengalfox:weekly"), quota("weekly", 10),
        quota("session", 20),
      ])
    expectEqual(entry.primary?.usedPercent, 20)
    expectEqual(entry.visibleWindows.count, 2)
    expectEqual(provider("codex", windows: [quota("weekly", 10)]).primary?.usedPercent, 10)
  }

  func testStableUUIDAndNegativeCoordinatesSurviveDisplayReordering() {
    let settings = Settings(enabled: true, displayId: "retina", edge: .right)
    let displays = [display("external"), display("retina", x: -1728)]
    expectEqual(
      notchFrame(settings: settings, displays: displays, expanded: false),
      CGRect(x: -76, y: 405, width: 76, height: 340))
    expectEqual(
      notchFrame(settings: settings, displays: displays.reversed(), expanded: true),
      CGRect(x: -388, y: 405, width: 388, height: 340))
    expectEqual(
      notchFrame(
        settings: Settings(enabled: true, displayId: "retina", edge: .left), displays: displays,
        expanded: true)?.minX, -1728)
  }

  func testMissingMirroredAmbiguousAndDisabledDisplaysHideWithoutFallback() {
    let selected = Settings(enabled: true, displayId: "chosen")
    for displays in [
      [display("other")], [display("chosen", mirrored: true)],
      [display("chosen"), display("chosen")],
    ] {
      expectNil(notchFrame(settings: selected, displays: displays, expanded: false))
    }
    expectNil(notchFrame(settings: Settings(), displays: [display("chosen")], expanded: false))
  }

  func testLegacySettingsDefaultToStandardAndPresetsScaleTheWholePanel() throws {
    let legacy = try JSONDecoder().decode(
      Settings.self,
      from: Data(#"{"enabled":true,"displayId":"main","edge":"right"}"#.utf8))
    expectEqual(legacy.size, NotchSize.standard)
    let displays = [display("main")]
    for (size, scale) in [
      (NotchSize.compact, 0.875), (NotchSize.standard, 1.0), (NotchSize.large, 1.125),
    ] {
      let settings = Settings(enabled: true, displayId: "main", edge: .right, size: size)
      let compact = notchFrame(settings: settings, displays: displays, expanded: false)
      let expanded = notchFrame(settings: settings, displays: displays, expanded: true)
      expectEqual(compact?.width, 76 * scale)
      expectEqual(expanded?.width, 388 * scale)
      expectEqual(compact?.height, 340 * scale)
      expectEqual(expanded?.height, 340 * scale)
    }
  }

  func testProtocolRejectsUnsupportedVersionOversizeAndInvalidPercent() throws {
    let valid =
      #"{"version":1,"sequence":1,"snapshot":{"settings":{"enabled":false,"edge":"right"},"displays":[]},"providers":[]}"#
    expectEqual(try HostMessage.decode(Data(valid.utf8)).sequence, 1)
    expectThrows(
      try HostMessage.decode(
        Data(valid.replacingOccurrences(of: "\"version\":1", with: "\"version\":2").utf8)))
    expectThrows(
      try HostMessage.decode(Data((valid + String(repeating: " ", count: 262_144)).utf8)))
    let invalid = valid.replacingOccurrences(
      of: "\"providers\":[]",
      with:
        #""providers":[{"provider":"claude","status":"ok","currentAccount":true,"windows":[{"id":"w","label":"Weekly","kind":"weekly","usedPercent":101,"observedAt":""}]}]"#
    )
    expectThrows(try HostMessage.decode(Data(invalid.utf8)))
  }

  func testClientActionsEncodeACompleteTypedProtocol() throws {
    let settings = Settings(enabled: true, displayId: "external", edge: .left, size: .large)
    let cases: [(ClientAction, [String: Any])] = [
      (.ready, ["version": 1, "type": "ready"]),
      (.ack(sequence: 42), ["version": 1, "type": "ack", "sequence": 42]),
      (.screensChanged, ["version": 1, "type": "screensChanged"]),
      (.refresh, ["version": 1, "type": "refresh"]),
      (.openLimits, ["version": 1, "type": "openLimits"]),
      (
        .save(settings: settings, revision: 7, request: 9),
        [
          "version": 1, "type": "save", "revision": 7, "request": 9,
          "settings": [
            "enabled": true, "displayId": "external", "edge": "left", "size": "large",
          ],
        ]
      ),
    ]
    for (action, expected) in cases {
      let encoded = try JSONEncoder().encode(action)
      let object = try JSONSerialization.jsonObject(with: encoded) as? NSDictionary
      expectEqual(object, expected as NSDictionary)
    }
  }
}

var failures = 0
@MainActor
func expectEqual<T: Equatable>(_ actual: T, _ expected: T, line: Int = #line) {
  if actual != expected {
    failures += 1
    print("FAIL line \(line): \(actual) != \(expected)")
  }
}
@MainActor
func expectNil<T>(_ value: T?, line: Int = #line) {
  if value != nil {
    failures += 1
    print("FAIL line \(line): expected nil")
  }
}
@MainActor
func expectThrows<T>(_ value: @autoclosure () throws -> T, line: Int = #line) {
  do {
    _ = try value()
    failures += 1
    print("FAIL line \(line): expected rejection")
  } catch {}
}
let checks = NotchTests()
checks.testClaudeUsesWeeklyAndBothFableAdapterFormsWithoutLosingSessionDetails()
checks.testNoWeeklyOrFableSubstitutionFromOtherQuotas()
checks.testUnavailableAndRememberedAccountsNeverPopulateCompactRings()
checks.testFableExpiresIndependentlyAndUnknownResetRemainsUsable()
checks.testCodexPrefersSessionAndHidesInternalBuckets()
checks.testStableUUIDAndNegativeCoordinatesSurviveDisplayReordering()
checks.testMissingMirroredAmbiguousAndDisabledDisplaysHideWithoutFallback()
try checks.testLegacySettingsDefaultToStandardAndPresetsScaleTheWholePanel()
try checks.testProtocolRejectsUnsupportedVersionOversizeAndInvalidPercent()
try checks.testClientActionsEncodeACompleteTypedProtocol()
print("10 native check groups; \(failures) failures")
exit(failures == 0 ? 0 : 1)
