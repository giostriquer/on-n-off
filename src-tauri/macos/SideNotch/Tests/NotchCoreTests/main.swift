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
    _ name: ProviderId = .claude, windows: [Quota], status: String = "ok", current: Bool = true,
    headline: String? = nil, inner: InnerRing? = nil
  ) -> Provider {
    Provider(
      provider: name, status: status, currentAccount: current, message: nil,
      windows: windows, headlineWindowId: headline, innerRing: inner)
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

  /// The ring used to jump to a light amber at 70 %, so a meter that was filling up went paler and
  /// yellower exactly as it ran out. Whatever shape the ramp takes, the invariant is that a fuller
  /// window never sits further from the trip red than a less full one.
  func testTheMeterRampOnlyEverMovesTowardTheTripRed() {
    func distanceToTrip(_ ink: Ink) -> Double {
      let dr = ink.r - tripInk.r, dg = ink.g - tripInk.g, db = ink.b - tripInk.b
      return (dr * dr + dg * dg + db * db).squareRoot()
    }
    // Every accent the meter can be handed, so a new provider is covered the day it lands.
    for base in railProviderOrder.map(providerInk) + [fableInk, creditsInk] {
      var previous = Double.infinity
      for step in 0...100 {
        let ink = meterInk(quota("weekly", Double(step)), base: base, at: now)
        let distance = distanceToTrip(ink)
        // Inside the band every step must move, not merely fail to retreat: a ramp that stopped
        // interpolating would still satisfy a non-increasing check.
        let strict = (71...89).contains(step) && base != tripInk
        if distance > previous + 0.000_001 || (strict && distance >= previous) {
          failures += 1
          print("FAIL meter ramp stalls or backtracks at \(step) %: \(distance) vs \(previous)")
        }
        previous = distance
      }
      expectEqual(meterInk(quota("weekly", 70), base: base, at: now), base)
      expectEqual(meterInk(quota("weekly", 90), base: base, at: now), tripInk)
      // Pins the easing itself: a quarter of the way through the band is half the way to red.
      // A linear blend would put 25 % here, and nothing else in this check would notice.
      expectEqual(
        meterInk(quota("weekly", 75), base: base, at: now), base.mixed(toward: tripInk, 0.5))
    }
    // An unreadable window stays grey rather than joining the ramp.
    expectEqual(meterInk(nil, base: claudeInk, at: now), unreadableInk)
  }

  /// The host decides what each ring shows (`NotchProvider` in `side_notch/model.rs`); the helper
  /// finds those windows by id and lists every window in the order it came.
  /// The popover lists the windows in the order they came, which the host pins
  /// (`side_notch/model/tests/projection.rs`); here they come in another order on purpose, so only
  /// a lookup by id finds the named ones: neither the first window nor the first per-model one.
  func testTheRingsShowTheWindowsTheHostNamed() {
    let fable = quota("model", 58, label: "Weekly · Fable", id: "weekly_scoped:Fable")
    let entry = provider(
      windows: [
        quota("session", 73, label: "5 hour · all models", id: "session"),
        quota("weekly", 41, id: "weekly_all"),
        quota("model", 90, label: "Weekly · Opus", id: "weekly_scoped:Opus"), fable,
      ], headline: "weekly_all", inner: .fable(windowId: "weekly_scoped:Fable"))
    expectEqual(entry.headline?.id, "weekly_all")
    expectEqual(entry.headline?.usedPercent, 41)
    expectEqual(entry.inner, InnerQuota.fable(fable))
    expectEqual(entry.inner?.quota.usedPercent, 58)
    // Nothing named, nothing on the rings.
    let unnamed = provider(windows: [quota("weekly", 41)])
    expectNil(unnamed.headline)
    expectNil(unnamed.inner)
  }

  /// The share as the host sends it: the reader's meter and the amounts already worded.
  func credits(
    _ percent: Double, reset: String? = "2027-02-01T12:00:00Z",
    left: String = "17,000 of 25,000 left", renewed: String = "25,000 of 25,000 left"
  ) -> WorkspaceCredits {
    WorkspaceCredits(usedPercent: percent, resetsAt: reset, left: left, renewed: renewed)
  }
  /// A reset's date as the viewer reads it, built from the same instant in local time.
  func localDate(_ instant: String) -> String {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US")
    formatter.dateFormat = "MMM d"
    return formatter.string(from: parseInstant(instant)!)
  }

  func testCodexCreditsFillTheInnerRingWhileTheWeeklyStaysOutside() {
    let entry = Provider(
      provider: .codex, status: "ok", currentAccount: true, message: nil,
      windows: [quota("weekly", 31, label: "Weekly · all models", id: "primary")],
      headlineWindowId: "primary", innerRing: .workspaceShare, workspaceCredits: credits(32))
    expectEqual(entry.headline?.usedPercent, 31)
    expectEqual(entry.inner, InnerQuota.workspaceShare(credits(32).quota))
    expectEqual(entry.inner?.quota.percent(at: now), 32)
    expectEqual(entry.inner?.quota.label, "Workspace credits")
    // The weekly stays the headline: the share never joins the windows the popover lists.
    expectEqual(entry.windows.map(\.label), ["Weekly · all models"])
  }

  /// The host words the amounts; the helper only picks the renewed wording once the reset has
  /// passed, and formats the reset's date in local time.
  func testACreditSharePicksItsWordingByTheClockAndRenewsAtItsReset() {
    let pending = credits(32)
    expectEqual(pending.amounts(at: now), "17,000 of 25,000 left")
    expectEqual(pending.quota.percent(at: now), 32)
    expectEqual(pending.note(at: now), "Resets \(localDate("2027-02-01T12:00:00Z"))")
    expectEqual(credits(100, left: "limit reached").amounts(at: now), "limit reached")
    expectEqual(credits(32, reset: nil).note(at: now), "")
    expectEqual(credits(32, reset: nil).amounts(at: now), "17,000 of 25,000 left")
    // Past its reset the share has renewed: nothing used, all of it left, and when it reset.
    let renewed = credits(100, reset: "2026-06-15T12:00:00Z", left: "limit reached")
    expectEqual(renewed.quota.percent(at: now), 0)
    expectEqual(renewed.quota.isReached(at: now), false)
    expectEqual(renewed.amounts(at: now), "25,000 of 25,000 left")
    expectEqual(renewed.note(at: now), "Reset \(localDate("2026-06-15T12:00:00Z"))")
  }

  /// A paused account's popover says its values are last observed whenever it shows any: remembered
  /// windows or a remembered share.
  func testAPausedAccountWithOnlyAShareStillHasObservedValues() {
    let share = Provider(
      provider: .codex, status: "failed", currentAccount: true, message: "Paused",
      windows: [], workspaceCredits: credits(32))
    expectEqual(share.hasObservedValues, true)
    expectEqual(provider(.codex, windows: [quota("weekly", 31)], status: "failed").hasObservedValues, true)
    expectEqual(provider(.codex, windows: [], status: "failed").hasObservedValues, false)
  }

  func testWindowsRenewIndependentlyAndUnknownResetRemainsUsable() {
    let entry = provider(windows: [
      quota("weekly", 41),
      quota("model", 58, label: "Weekly · Fable", reset: "2026-01-01T00:00:00Z"),
    ])
    expectEqual(entry.windows[0].percent(at: now), 41)
    // A window past its own reset is back at zero, not unknown: the quota renewed.
    expectEqual(entry.windows[1].percent(at: now), 0)
    expectEqual(entry.windows[1].text(at: now), "0%")
    expectEqual(quota("weekly", 0.4, reset: "invalid").percent(at: now), 0.4)
    expectEqual(quota("weekly", 9, reset: "2027-01-15T08:00:00.000Z").percent(at: now), 0)
    // A figure that is not a usable percentage stays unknown, reset or not.
    expectNil(quota("weekly", .nan).percent(at: now))
    expectNil(quota("weekly", 101, reset: "2026-01-01T00:00:00Z").percent(at: now))
    expectEqual(quota("weekly", .nan).text(at: now), "—")
    expectEqual(quota("weekly", 100).isReached(at: now), true)
    expectEqual(quota("weekly", 99.49).isReached(at: now), false)
    expectEqual(quota("weekly", 41, reset: "2027-02-01T00:00:00Z").note(at: now).hasPrefix("Resets "), true)
    // The renewed note says when, and never recites the spent cycle's figure.
    let renewed = quota("weekly", 97, reset: "2026-01-01T00:00:00Z").note(at: now)
    expectEqual(renewed.hasPrefix("Reset "), true)
    expectEqual(renewed.contains("last seen"), false)
    expectEqual(renewed.contains("97"), false)
    expectEqual(quota("weekly", 41).note(at: now), "")
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
      #"{"version":4,"sequence":1,"snapshot":{"settings":{"enabled":false,"displayId":null,"edge":"right","size":"standard","show":"always","providers":["claude"],"pullRequests":{"enabled":true,"lists":["mine"]}},"displays":[]},"providers":[]}"#
    expectEqual(try HostMessage.decode(Data(valid.utf8)).sequence, 1)
    // An older host's message is refused rather than drawn in part, whichever version it speaks.
    for older in ["1", "2", "3"] {
      expectThrows(
        try HostMessage.decode(
          Data(valid.replacingOccurrences(of: "\"version\":4", with: "\"version\":\(older)").utf8)))
    }
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
    expectNil(decoded.providers[0].workspaceCredits)
    let withCredits = valid.replacingOccurrences(
      of: "\"providers\":[]",
      with:
        #""providers":[{"provider":"codex","status":"ok","currentAccount":true,"windows":[],"innerRing":{"kind":"workspaceShare"},"sessions":[],"workspaceCredits":{"usedPercent":32,"resetsAt":"2027-02-01T12:00:00+00:00","left":"17,000 of 25,000 left","renewed":"25,000 of 25,000 left"}}]"#
    )
    expectEqual(
      try HostMessage.decode(Data(withCredits.utf8)).providers[0].inner?.quota.percent(at: now), 32)
    expectThrows(
      try HostMessage.decode(
        Data(withCredits.replacingOccurrences(of: "\"usedPercent\":32", with: "\"usedPercent\":101").utf8)))
    let emptyName = withSessions.replacingOccurrences(of: "\"name\":\"repo-1a\"", with: "\"name\":\"\"")
    expectThrows(try HostMessage.decode(Data(emptyName.utf8)))
    let oneSession =
      #"{"id":"1","name":"repo-1a","place":"Terminal","project":"repo","status":"idle","lastActiveAt":"2026-09-01T10:00:00Z"}"#
    let tooMany = withSessions.replacingOccurrences(
      of: "\"sessions\":[", with: "\"sessions\":[" + String(repeating: oneSession + ",", count: maxSessions))
    expectThrows(try HostMessage.decode(Data(tooMany.utf8)))
    expectEqual(maxSessions, 12)
  }

  /// The host names the ring's windows by id. A message naming a window it did not send, or putting
  /// a share it did not send on the inner ring, is refused rather than drawn with an empty ring.
  func testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames() throws {
    let valid =
      #"{"version":4,"sequence":1,"snapshot":{"settings":{"enabled":false,"displayId":null,"edge":"right","size":"standard","show":"always","providers":["claude"],"pullRequests":{"enabled":true,"lists":["mine"]}},"displays":[]},"providers":[{"provider":"claude","status":"ok","currentAccount":true,"windows":[{"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":7,"observedAt":""},{"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":13,"observedAt":""}],"headlineWindowId":"weekly_all","innerRing":{"kind":"fable","windowId":"weekly_scoped:Fable"},"sessions":[]}]}"#
    let entry = try HostMessage.decode(Data(valid.utf8)).providers[0]
    expectEqual(entry.headline?.usedPercent, 7)
    expectEqual(entry.inner?.quota.usedPercent, 13)
    for dangling in [
      valid.replacingOccurrences(
        of: "\"headlineWindowId\":\"weekly_all\"", with: "\"headlineWindowId\":\"session\""),
      valid.replacingOccurrences(
        of: "\"windowId\":\"weekly_scoped:Fable\"", with: "\"windowId\":\"weekly_fable\""),
      valid.replacingOccurrences(
        of: "{\"kind\":\"fable\",\"windowId\":\"weekly_scoped:Fable\"}",
        with: "{\"kind\":\"workspaceShare\"}"),
      valid.replacingOccurrences(of: "\"kind\":\"fable\"", with: "\"kind\":\"sparkles\""),
    ] {
      expectThrows(try HostMessage.decode(Data(dangling.utf8)))
    }
  }

  func testPullRequestsValidateLinksListsAndCapsAndCountDistinctRows() throws {
    let valid =
      #"{"version":4,"sequence":1,"snapshot":{"settings":{"enabled":false,"displayId":null,"edge":"right","size":"standard","show":"always","providers":["claude"],"pullRequests":{"enabled":true,"lists":["mine"]}},"displays":[]},"providers":[]}"#
    let pull =
      #"{"id":"node","number":42,"title":"ci: one concurrency group","url":"https://github.com/octo/tools/pull/42","repo":"octo/tools","author":"gio","isDraft":false,"reviewDecision":"APPROVED","ci":"success","mergeKind":"ready","updatedAt":"2026-09-01T10:00:00Z"}"#
    let withPulls = valid.replacingOccurrences(
      of: "\"providers\":[]}",
      with: "\"providers\":[],\"pullRequests\":{\"status\":\"ok\",\"hint\":null,\"stale\":false,\"lists\":[{\"id\":\"mine\",\"total\":1,\"items\":[\(pull)]},{\"id\":\"assigned\",\"total\":1,\"items\":[\(pull)]}]}}")
    let decoded = try HostMessage.decode(Data(withPulls.utf8))
    expectEqual(decoded.pullRequests?.count, 1)  // the same pull request in two lists counts once
    expectEqual(decoded.pullRequests?.ready, 1)
    expectEqual(decoded.pullRequests?.lists.first?.items.first?.link?.host, "github.com")
    expectEqual(decoded.pullRequests?.lists.first?.items.first?.ci, CiState.success)
    expectEqual(decoded.pullRequests?.lists.first?.items.first?.reviewDecision, ReviewDecision.approved)
    // Values a newer host may send decode to `unknown` instead of rejecting the message.
    let newer = withPulls.replacingOccurrences(of: "\"ci\":\"success\"", with: "\"ci\":\"skipped\"")
      .replacingOccurrences(of: "\"mergeKind\":\"ready\"", with: "\"mergeKind\":\"frozen\"")
    let lenient = try HostMessage.decode(Data(newer.utf8))
    expectEqual(lenient.pullRequests?.lists.first?.items.first?.ci, CiState.unknown)
    expectEqual(lenient.pullRequests?.lists.first?.items.first?.mergeKind, MergeKind.unknown)
    expectEqual(lenient.pullRequests?.ready, 0)
    let offsite = withPulls.replacingOccurrences(
      of: "https://github.com/octo/tools/pull/42", with: "http://example.com/pull/42")
    expectThrows(try HostMessage.decode(Data(offsite.utf8)))
    let duplicateLists = withPulls.replacingOccurrences(
      of: "\"lists\":[", with: "\"lists\":[{\"id\":\"mine\",\"total\":0,\"items\":[]},")
    expectThrows(try HostMessage.decode(Data(duplicateLists.utf8)))
    expectEqual(maxPullRequests, 25)
  }

  func testConflictBandRequiresPassingCIAndMergeConflicts() {
    let states: [CiState] = [.none, .pending, .success, .failure, .error, .unknown]
    let kinds: [MergeKind?] = [nil, .conflicts, .queued, .autoMerge, .ready, .behind, .blocked, .unknown]
    for ci in states {
      for kind in kinds {
        let row = PullRequest(id: "fixture", number: 1, title: "Fixture", url: "https://github.com/o/r/pull/1", repo: "o/r", author: "fixture", isDraft: false, reviewDecision: nil, ci: ci, mergeKind: kind, updatedAt: "")
        expectEqual(row.passingWithConflicts, ci == .success && kind == .conflicts)
      }
    }
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
      (.ready, ["version": 4, "type": "ready"]),
      (.ack(sequence: 42), ["version": 4, "type": "ack", "sequence": 42]),
      (.screensChanged, ["version": 4, "type": "screensChanged"]),
      (.refresh, ["version": 4, "type": "refresh"]),
      (.openLimits, ["version": 4, "type": "openLimits"]),
      (.openPullRequests, ["version": 4, "type": "openPullRequests"]),
      (.setShow(.onHover), ["version": 4, "type": "setShow", "show": "onHover"]),
      (.setShow(.always), ["version": 4, "type": "setShow", "show": "always"]),
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
checks.testTheMeterRampOnlyEverMovesTowardTheTripRed()
checks.testTheRingsShowTheWindowsTheHostNamed()
checks.testCodexCreditsFillTheInnerRingWhileTheWeeklyStaysOutside()
checks.testACreditSharePicksItsWordingByTheClockAndRenewsAtItsReset()
checks.testAPausedAccountWithOnlyAShareStillHasObservedValues()
checks.testWindowsRenewIndependentlyAndUnknownResetRemainsUsable()
checks.testSessionAgesReadLikeTheReferenceApp()
checks.testRailFramesFollowTheSelectedUUIDOnEveryEdge()
checks.testTheRailShrinksWithFewerProvidersAndHidesWithoutAnyOrWithoutRoom()
checks.testMissingMirroredAmbiguousAndDisabledDisplaysHideWithoutFallback()
try checks.testSettingsDecodeTheHostDocumentAndRejectUnknownProviders()
checks.testPresetsScaleTheWholeRailOnThePixelGrid()
checks.testPillsHugTheEdgeCentredOnTheRail()
checks.testCellsTileTheRailAndPopoversStayInsideTheWorkArea()
try checks.testProtocolRejectsUnsupportedVersionOversizeInvalidPercentAndBadSessions()
try checks.testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames()
try checks.testPullRequestsValidateLinksListsAndCapsAndCountDistinctRows()
checks.testConflictBandRequiresPassingCIAndMergeConflicts()
checks.testReviewRequestsLinkTheTitleAndEscapeMarkup()
try checks.testClientActionsEncodeACompleteTypedProtocol()
print("20 native check groups; \(failures) failures")
exit(failures == 0 ? 0 : 1)
