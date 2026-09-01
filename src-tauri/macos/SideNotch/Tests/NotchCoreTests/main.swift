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
    _ name: ProviderId = .claude, windows: [Quota], status: String = "ok", current: Bool = true
  ) -> Provider {
    Provider(
      provider: name, status: status, currentAccount: current, plan: "max", message: nil,
      windows: windows)
  }
  func rail(
    _ displayId: String, edge: Edge = .right, size: NotchSize = .standard,
    providers: [ProviderId] = railProviderOrder, pullRequests: Bool = false
  ) -> Settings {
    Settings(
      enabled: true, displayId: displayId, edge: edge, size: size, providers: providers,
      pullRequests: PullRequestSettings(enabled: pullRequests, lists: [.mine]))
  }
  func display(_ id: String, x: Double = 0, mirrored: Bool = false, scale: Double = 2) -> Display
  {
    Display(
      id: id, name: "Same name", x: x, y: 0, width: 1728, height: 1117, workY: 33,
      workHeight: 1084, scale: scale, mirrored: mirrored)
  }

  func testClaudeRingsShowWeeklyAndFableWhileThePopoverListsTheSessionFirst() {
    for id in ["weekly_fable", "weekly_scoped:Fable"] {
      let entry = provider(windows: [
        quota("session", 73, label: "5 hour · all models", id: "session"), quota("weekly", 41),
        quota("model", 58, label: "Weekly · Fable", id: id),
      ])
      expectEqual(entry.primary?.usedPercent, 41)
      expectEqual(entry.fable?.usedPercent, 58)
      expectEqual(entry.orderedWindows.map(\.usedPercent), [73, 41, 58])
    }
    let noWeekly = provider(windows: [
      quota("session", 73), quota("model", 90, label: "Weekly · Opus"),
    ])
    expectNil(noWeekly.primary)
    expectNil(noWeekly.fable)
    expectNil(provider(windows: []).primary)
  }

  func testUnavailableAndRememberedAccountsNeverPopulateRings() {
    for entry in [
      provider(windows: [quota("weekly", 99)], status: "failed"),
      provider(windows: [quota("weekly", 99)], current: false),
      provider(.cursor, windows: [], status: "unsupported"),
    ] {
      expectNil(entry.primary)
      expectNil(entry.fable)
    }
  }

  func testWindowsExpireIndependentlyAndUnknownResetRemainsUsable() {
    let entry = provider(windows: [
      quota("weekly", 41),
      quota("model", 58, label: "Weekly · Fable", reset: "2026-01-01T00:00:00Z"),
    ])
    expectEqual(entry.orderedWindows[0].percent(at: now), 41)
    expectNil(entry.orderedWindows[1].percent(at: now))
    expectEqual(quota("weekly", 0.4, reset: "invalid").percent(at: now), 0.4)
    expectNil(quota("weekly", 9, reset: "2027-01-15T08:00:00.000Z").percent(at: now))
    expectEqual(quota("weekly", 100).isReached(at: now), true)
    expectEqual(quota("weekly", 99.49).isReached(at: now), false)
    expectEqual(quota("weekly", 41, reset: "2027-02-01T00:00:00Z").note(at: now).hasPrefix("Resets "), true)
    expectEqual(quota("weekly", 41).note(at: now), "")
  }

  func testCodexPrefersSessionAndHidesInternalBuckets() {
    let entry = provider(
      .codex,
      windows: [
        quota("model", 99, label: "Weekly · GPT-Reserve"),
        quota("model", 80, id: "extra:codex_bengalfox:weekly"), quota("weekly", 10),
        quota("session", 20),
      ])
    expectEqual(entry.primary?.usedPercent, 20)
    expectEqual(entry.visibleWindows.count, 2)
    expectEqual(provider(.codex, windows: [quota("weekly", 10)]).primary?.usedPercent, 10)
  }

  func testSessionAgesReadLikeTheReferenceApp() {
    func session(_ seconds: Double, status: String = "idle") -> Session {
      let formatter = ISO8601DateFormatter()
      return Session(
        id: "s", name: "repo-1a", place: "Terminal", project: "repo", status: status,
        lastActiveAt: formatter.string(from: now.addingTimeInterval(-seconds)))
    }
    expectEqual(session(20).age(at: now), "just now")
    expectEqual(session(4 * 60).age(at: now), "4 min")
    expectEqual(session(3 * 3600).age(at: now), "3 h")
    expectEqual(session(2 * 86400).age(at: now), "2 d")
    expectEqual(session(0, status: "working").isWorking, true)
    expectEqual(session(0).isWorking, false)
  }

  func testRailFramesFollowTheSelectedUUIDOnEveryEdge() {
    let displays = [display("external"), display("retina", x: -1728)]
    // Four cells: 4 × 73 + 3 × 8 + 2 × 40 = 396.
    expectEqual(
      notchRailFrame(settings: rail("retina"), displays: displays),
      CGRect(x: -76, y: 377, width: 76, height: 396))
    expectEqual(
      notchRailFrame(settings: rail("retina", edge: .left), displays: displays.reversed()),
      CGRect(x: -1728, y: 377, width: 76, height: 396))
    expectEqual(
      notchRailFrame(settings: rail("external", edge: .top), displays: displays),
      CGRect(x: 660, y: 33, width: 408, height: 73))
    expectEqual(
      notchRailFrame(settings: rail("external", edge: .bottom), displays: displays),
      CGRect(x: 660, y: 33 + 1084 - 73, width: 408, height: 73))
  }

  func testTheRailShrinksWithFewerProvidersAndHidesWithoutAnyOrWithoutRoom() {
    let displays = [display("main")]
    let two = rail("main", providers: [.cursor, .claude, .claude])
    expectEqual(two.railProviders, [.claude, .cursor])
    expectEqual(notchRailFrame(settings: two, displays: displays)?.height, 234)
    expectNil(notchRailFrame(settings: rail("main", providers: []), displays: displays))
    // The pull-request cell counts like a provider and is on by default, listing only "Mine".
    let defaults = Settings(enabled: true, displayId: "main")
    expectEqual(defaults.pullRequests.enabled, true)
    expectEqual(defaults.railCells.count, 5)
    expectEqual(defaults.railCells.last, RailCell.pullRequests)
    expectEqual(
      PullRequestSettings(enabled: true, lists: [.assigned, .mine, .assigned]).selectedLists,
      [.mine, .assigned])
    expectEqual(
      notchRailFrame(settings: rail("main", providers: [], pullRequests: true), displays: displays)?
        .height, 73 + 80)
    let short = Display(
      id: "short", name: "LG", x: 0, y: 0, width: 1920, height: 300, workY: 30, workHeight: 270,
      scale: 1, mirrored: false)
    expectNil(notchRailFrame(settings: rail("short"), displays: [short]))
  }

  func testMissingMirroredAmbiguousAndDisabledDisplaysHideWithoutFallback() {
    let selected = rail("chosen")
    for displays in [
      [display("other")], [display("chosen", mirrored: true)],
      [display("chosen"), display("chosen")],
    ] {
      expectNil(notchRailFrame(settings: selected, displays: displays))
      expectNil(notchPillFrame(settings: selected, displays: displays))
    }
    expectNil(notchRailFrame(settings: Settings(), displays: [display("chosen")]))
  }

  func testSettingsDecodeTheHostDocumentAndRejectUnknownProviders() throws {
    let document = try JSONDecoder().decode(
      Settings.self,
      from: Data(
        #"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["codex","codex"],"pullRequests":{"enabled":true,"lists":["mine","assigned"]}}"#
          .utf8))
    expectEqual(document.pullRequests.selectedLists, [.mine, .assigned])
    expectEqual(document.show, ShowMode.onHover)
    expectEqual(document.edge, Edge.top)
    expectEqual(document.size, NotchSize.compact)
    expectEqual(document.railProviders, [.codex])
    expectThrows(
      try JSONDecoder().decode(
        Settings.self,
        from: Data(
          #"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["gemini"],"pullRequests":{"enabled":false,"lists":[]}}"#
            .utf8)))
    expectThrows(
      try JSONDecoder().decode(
        Settings.self, from: Data(#"{"enabled":true,"displayId":"main","edge":"right"}"#.utf8)))
  }

  func testPresetsScaleTheWholeRailOnThePixelGrid() {
    let displays = [display("main", scale: 1)]
    for (size, scale) in [
      (NotchSize.compact, 0.875), (NotchSize.standard, 1.0), (NotchSize.large, 1.125),
    ] {
      let settings = rail("main", size: size)
      let frame = notchRailFrame(settings: settings, displays: displays)
      expectEqual(frame.map { Double($0.width) }, (76 * scale).rounded())
      let layout = railLayout(size: size, displayScale: 1, edge: .right)
      expectEqual(frame.map { Double($0.height) }, layout.length(count: 4))
      for value in [frame?.minX, frame?.minY, frame?.width, frame?.height] {
        expectEqual(value, value?.rounded())
      }
    }
  }

  func testPillsHugTheEdgeCentredOnTheRail() {
    let displays = [display("main", scale: 1)]
    for edge in [Edge.left, .right, .top, .bottom] {
      let settings = rail("main", edge: edge)
      let rail = notchRailFrame(settings: settings, displays: displays)!
      let pill = notchPillFrame(settings: settings, displays: displays)!
      expectEqual(rail.contains(pill), true)
      switch edge {
      case .left: expectEqual(pill.minX, rail.minX)
      case .right: expectEqual(pill.maxX, rail.maxX)
      case .top: expectEqual(pill.minY, rail.minY)
      case .bottom: expectEqual(pill.maxY, rail.maxY)
      }
      expectEqual(edge.isVertical ? pill.midY : pill.midX, edge.isVertical ? rail.midY : rail.midX)
    }
  }

  func testCellsTileTheRailAndPopoversStayInsideTheWorkArea() {
    let main = display("main", scale: 1)
    let layout = railLayout(size: .standard, displayScale: 1, edge: .right)
    let cells = railCellFrames(edge: .right, layout: layout, count: 4)
    expectEqual(cells.count, 4)
    expectEqual(cells[0].minY, layout.ear)
    expectEqual(cells[3].maxY, layout.length(count: 4) - layout.ear)
    let bar = railLayout(size: .standard, displayScale: 1, edge: .top)
    expectEqual(bar.thickness, 73)
    expectEqual(railCellFrames(edge: .top, layout: bar, count: 2)[1].minX, 40 + 76 + 8)
    // The block of cells is centred, so both ends of the rail read the same.
    expectEqual(cells[0].minY, layout.length(count: 4) - cells[3].maxY)

    let railFrame = notchRailFrame(settings: rail("main"), displays: [main])!
    let cell = cells[0].offsetBy(dx: railFrame.minX, dy: railFrame.minY)
    let size = CGSize(width: 300, height: 420)
    let frame = popoverFrame(cell: cell, edge: .right, size: size, display: main, scale: 1)
    expectEqual(frame.maxX, cell.minX - 2)
    // An odd cell height puts the cell's centre on a half point; the popover snaps to the pixel grid.
    expectEqual(abs(frame.midY - cell.midY) <= 0.5, true)
    let topCell = CGRect(x: 690, y: 33, width: 76, height: 73)
    let below = popoverFrame(cell: topCell, edge: .top, size: size, display: main, scale: 1)
    expectEqual(below.minY, topCell.maxY + 2)
    expectEqual(below.midX, topCell.midX)
    let cornerCell = CGRect(x: 1652, y: 40, width: 76, height: 73)
    let clamped = popoverFrame(cell: cornerCell, edge: .right, size: size, display: main, scale: 1)
    expectEqual(clamped.minY, 33 + 8)
    let bottomCell = CGRect(x: 1652, y: 1100, width: 76, height: 73)
    let clampedBottom = popoverFrame(cell: bottomCell, edge: .right, size: size, display: main, scale: 1)
    expectEqual(clampedBottom.maxY, 33 + 1084 - 8)
  }

  func testProtocolRejectsUnsupportedVersionOversizeInvalidPercentAndBadSessions() throws {
    let valid =
      #"{"version":2,"sequence":1,"snapshot":{"settings":{"enabled":false,"displayId":null,"edge":"right","size":"standard","show":"always","providers":["claude"],"pullRequests":{"enabled":true,"lists":["mine"]}},"displays":[]},"providers":[]}"#
    expectEqual(try HostMessage.decode(Data(valid.utf8)).sequence, 1)
    expectThrows(
      try HostMessage.decode(
        Data(valid.replacingOccurrences(of: "\"version\":2", with: "\"version\":1").utf8)))
    expectThrows(
      try HostMessage.decode(Data((valid + String(repeating: " ", count: 262_144)).utf8)))
    let invalid = valid.replacingOccurrences(
      of: "\"providers\":[]",
      with:
        #""providers":[{"provider":"claude","status":"ok","currentAccount":true,"windows":[{"id":"w","label":"Weekly","kind":"weekly","usedPercent":101,"observedAt":""}],"sessions":[]}]"#
    )
    expectThrows(try HostMessage.decode(Data(invalid.utf8)))
    let unknownProvider = valid.replacingOccurrences(
      of: "\"providers\":[]",
      with: #""providers":[{"provider":"gemini","status":"ok","currentAccount":true,"windows":[],"sessions":[]}]"#)
    expectThrows(try HostMessage.decode(Data(unknownProvider.utf8)))
    let withSessions = valid.replacingOccurrences(
      of: "\"providers\":[]",
      with:
        #""providers":[{"provider":"cursor","status":"unsupported","currentAccount":true,"windows":[],"sessions":[{"id":"1","name":"repo-1a","place":"Terminal","project":"repo","status":"working","lastActiveAt":"2026-09-01T10:00:00Z"}]}]"#
    )
    let decoded = try HostMessage.decode(Data(withSessions.utf8))
    expectEqual(decoded.providers[0].sessions.first?.isWorking, true)
    let emptyName = withSessions.replacingOccurrences(of: "\"name\":\"repo-1a\"", with: "\"name\":\"\"")
    expectThrows(try HostMessage.decode(Data(emptyName.utf8)))
    let oneSession =
      #"{"id":"1","name":"repo-1a","place":"Terminal","project":"repo","status":"idle","lastActiveAt":"2026-09-01T10:00:00Z"}"#
    let tooMany = withSessions.replacingOccurrences(
      of: "\"sessions\":[", with: "\"sessions\":[" + String(repeating: oneSession + ",", count: maxSessions))
    expectThrows(try HostMessage.decode(Data(tooMany.utf8)))
    expectEqual(maxSessions, 12)

    let pull =
      #"{"id":"node","number":42,"title":"ci: one concurrency group","url":"https://github.com/octo/tools/pull/42","repo":"octo/tools","author":"gio","isDraft":false,"reviewDecision":"APPROVED","ci":"success","mergeKind":"ready","updatedAt":"2026-09-01T10:00:00Z"}"#
    let withPulls = valid.replacingOccurrences(
      of: "\"providers\":[]}",
      with: "\"providers\":[],\"pullRequests\":{\"status\":\"ok\",\"hint\":null,\"stale\":false,\"lists\":[{\"id\":\"mine\",\"total\":1,\"items\":[\(pull)]}]}}")
    let decoded2 = try HostMessage.decode(Data(withPulls.utf8))
    expectEqual(decoded2.pullRequests?.count, 1)
    expectEqual(decoded2.pullRequests?.ready, 1)
    expectEqual(decoded2.pullRequests?.lists.first?.items.first?.link?.host, "github.com")
    let offsite = withPulls.replacingOccurrences(
      of: "https://github.com/octo/tools/pull/42", with: "http://example.com/pull/42")
    expectThrows(try HostMessage.decode(Data(offsite.utf8)))
    let duplicateLists = withPulls.replacingOccurrences(
      of: "\"lists\":[", with: "\"lists\":[{\"id\":\"mine\",\"total\":0,\"items\":[]},")
    expectThrows(try HostMessage.decode(Data(duplicateLists.utf8)))
    expectEqual(maxPullRequests, 25)
  }

  func testReviewRequestsLinkTheTitleAndEscapeMarkup() {
    let url = URL(string: "https://github.com/octo/tools/pull/42?x=1&y=2")!
    expectEqual(
      reviewRequestHtml(title: "ci: <run> \"dev\" & main", url: url),
      "review please: <a href=\"https://github.com/octo/tools/pull/42?x=1&amp;y=2\">ci: &lt;run&gt; &quot;dev&quot; &amp; main</a>"
    )
    expectEqual(
      reviewRequestText(title: "ci: run", url: url),
      "review please: ci: run https://github.com/octo/tools/pull/42?x=1&y=2")
  }

  func testClientActionsEncodeACompleteTypedProtocol() throws {
    let cases: [(ClientAction, [String: Any])] = [
      (.ready, ["version": 2, "type": "ready"]),
      (.ack(sequence: 42), ["version": 2, "type": "ack", "sequence": 42]),
      (.screensChanged, ["version": 2, "type": "screensChanged"]),
      (.refresh, ["version": 2, "type": "refresh"]),
      (.openLimits, ["version": 2, "type": "openLimits"]),
      (.openPullRequests, ["version": 2, "type": "openPullRequests"]),
      (.setShow(.onHover), ["version": 2, "type": "setShow", "show": "onHover"]),
      (.setShow(.always), ["version": 2, "type": "setShow", "show": "always"]),
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
checks.testClaudeRingsShowWeeklyAndFableWhileThePopoverListsTheSessionFirst()
checks.testUnavailableAndRememberedAccountsNeverPopulateRings()
checks.testWindowsExpireIndependentlyAndUnknownResetRemainsUsable()
checks.testCodexPrefersSessionAndHidesInternalBuckets()
checks.testSessionAgesReadLikeTheReferenceApp()
checks.testRailFramesFollowTheSelectedUUIDOnEveryEdge()
checks.testTheRailShrinksWithFewerProvidersAndHidesWithoutAnyOrWithoutRoom()
checks.testMissingMirroredAmbiguousAndDisabledDisplaysHideWithoutFallback()
try checks.testSettingsDecodeTheHostDocumentAndRejectUnknownProviders()
checks.testPresetsScaleTheWholeRailOnThePixelGrid()
checks.testPillsHugTheEdgeCentredOnTheRail()
checks.testCellsTileTheRailAndPopoversStayInsideTheWorkArea()
try checks.testProtocolRejectsUnsupportedVersionOversizeInvalidPercentAndBadSessions()
checks.testReviewRequestsLinkTheTitleAndEscapeMarkup()
try checks.testClientActionsEncodeACompleteTypedProtocol()
print("15 native check groups; \(failures) failures")
exit(failures == 0 ? 0 : 1)
