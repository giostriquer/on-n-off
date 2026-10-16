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
  func reading(
    windows: [Quota], status: String = "ok", headline: String? = nil, inner: InnerRing? = nil
  ) -> Reading {
    Reading(
      status: status, message: nil, windows: windows, headlineWindowId: headline, innerRing: inner)
  }
  func rail(
    _ displayId: String, edge: Edge = .right, size: NotchSize = .standard,
    providers: [ProviderId] = ProviderId.allCases, pullRequests: Bool = false
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

  func testTheMeterRampOnlyEverMovesTowardTheTripRed() {
    func distanceToTrip(_ ink: Ink) -> Double {
      let dr = ink.r - tripInk.r, dg = ink.g - tripInk.g, db = ink.b - tripInk.b
      return (dr * dr + dg * dg + db * db).squareRoot()
    }
    for base in ProviderId.allCases.map(providerInk) + [fableInk, creditsInk] {
      var previous = Double.infinity
      for step in 0...100 {
        let ink = meterInk(quota("weekly", Double(step)), base: base, at: now)
        let distance = distanceToTrip(ink)
        let strict = (71...89).contains(step) && base != tripInk
        if distance > previous + 0.000_001 || (strict && distance >= previous) {
          failures += 1
          print("FAIL meter ramp stalls or backtracks at \(step) %: \(distance) vs \(previous)")
        }
        previous = distance
      }
      expectEqual(meterInk(quota("weekly", 70), base: base, at: now), base)
      expectEqual(meterInk(quota("weekly", 90), base: base, at: now), tripInk)
      expectEqual(
        meterInk(quota("weekly", 75), base: base, at: now), base.mixed(toward: tripInk, 0.5))
    }
    expectEqual(meterInk(nil, base: claudeInk, at: now), unreadableInk)
  }

  func testTheRingsShowTheWindowsTheHostNamed() {
    let fable = quota("model", 58, label: "Weekly · Fable", id: "weekly_scoped:Fable")
    let entry = reading(
      windows: [
        quota("session", 73, label: "5 hour · all models", id: "session"),
        quota("weekly", 41, id: "weekly_all"),
        quota("model", 90, label: "Weekly · Opus", id: "weekly_scoped:Opus"), fable,
      ], headline: "weekly_all", inner: .fable(windowId: "weekly_scoped:Fable"))
    expectEqual(entry.headline?.id, "weekly_all")
    expectEqual(entry.headline?.usedPercent, 41)
    expectEqual(entry.inner, InnerQuota.fable(fable))
    expectEqual(entry.inner?.quota.usedPercent, 58)
    let unnamed = reading(windows: [quota("weekly", 41)])
    expectNil(unnamed.headline)
    expectNil(unnamed.inner)
  }

  func credits(
    _ percent: Double, reset: String? = "2027-02-01T12:00:00Z",
    left: String = "17,000 of 25,000 left", renewed: String = "25,000 of 25,000 left"
  ) -> WorkspaceCredits {
    WorkspaceCredits(usedPercent: percent, resetsAt: reset, left: left, renewed: renewed)
  }
  func localDate(_ instant: String) -> String {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US")
    formatter.dateFormat = "MMM d"
    return formatter.string(from: parseInstant(instant)!)
  }

  func testCodexCreditsFillTheInnerRingWhileTheWeeklyStaysOutside() {
    let entry = Reading(
      status: "ok", message: nil,
      windows: [quota("weekly", 31, label: "Weekly · all models", id: "primary")],
      headlineWindowId: "primary", innerRing: .workspaceShare, workspaceCredits: credits(32))
    expectEqual(entry.headline?.usedPercent, 31)
    expectEqual(entry.inner, InnerQuota.workspaceShare(credits(32).quota))
    expectEqual(entry.inner?.quota.percent(at: now), 32)
    expectEqual(entry.inner?.quota.label, "Workspace credits")
    expectEqual(entry.windows.map(\.label), ["Weekly · all models"])
  }

  func testACreditSharePicksItsWordingByTheClockAndRenewsAtItsReset() {
    let pending = credits(32)
    expectEqual(pending.amounts(at: now), "17,000 of 25,000 left")
    expectEqual(pending.quota.percent(at: now), 32)
    expectEqual(pending.note(at: now), "Resets \(localDate("2027-02-01T12:00:00Z"))")
    expectEqual(credits(100, left: "limit reached").amounts(at: now), "limit reached")
    expectEqual(credits(32, reset: nil).note(at: now), "")
    expectEqual(credits(32, reset: nil).amounts(at: now), "17,000 of 25,000 left")
    let renewed = credits(100, reset: "2026-06-15T12:00:00Z", left: "limit reached")
    expectEqual(renewed.quota.percent(at: now), 0)
    expectEqual(renewed.quota.isReached(at: now), false)
    expectEqual(renewed.amounts(at: now), "25,000 of 25,000 left")
    expectEqual(renewed.note(at: now), "Reset \(localDate("2026-06-15T12:00:00Z"))")
  }

  func testAPausedRingSaysItsReadingIsTheLastObserved() {
    let weekly = [quota("weekly", 46)]
    for status in ["failed", "signedOut", "unauthenticated", "unsupported"] {
      expectEqual(reading(windows: weekly, status: status, headline: "weekly").ringIsLastObserved, true)
    }
    expectEqual(reading(windows: weekly, status: "ok", headline: "weekly").ringIsLastObserved, false)
    expectEqual(reading(windows: weekly, status: "failed").ringIsLastObserved, false)
  }

  func testWindowsRenewIndependentlyAndUnknownResetRemainsUsable() {
    let entry = reading(windows: [
      quota("weekly", 41),
      quota("model", 58, label: "Weekly · Fable", reset: "2026-01-01T00:00:00Z"),
    ])
    expectEqual(entry.windows[0].percent(at: now), 41)
    expectEqual(entry.windows[1].percent(at: now), 0)
    expectEqual(entry.windows[1].text(at: now), "0%")
    expectEqual(quota("weekly", 0.4, reset: "invalid").percent(at: now), 0.4)
    expectEqual(quota("weekly", 9, reset: "2027-01-15T08:00:00.000Z").percent(at: now), 0)
    expectNil(quota("weekly", .nan).percent(at: now))
    expectNil(quota("weekly", 101, reset: "2026-01-01T00:00:00Z").percent(at: now))
    expectEqual(quota("weekly", .nan).text(at: now), "—")
    expectEqual(quota("weekly", 100).isReached(at: now), true)
    expectEqual(quota("weekly", 99.49).isReached(at: now), false)
    expectEqual(quota("weekly", 41, reset: "2027-02-01T00:00:00Z").note(at: now).hasPrefix("Resets "), true)
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

  func testTheRailCountsEachProviderOnceAndThePullRequestCell() {
    expectEqual(rail("main", providers: [.cursor, .claude, .claude]).cellCount, 2)
    let defaults = Settings(enabled: true, displayId: "main")
    expectEqual(defaults.providers, ProviderId.allCases)
    expectEqual(defaults.pullRequests.enabled, true)
    expectEqual(defaults.cellCount, 6)
  }

  func testRailFramesAndPillsMatchTheSharedCases() throws {
    let fixture = try RailFixture.load()
    expectEqual(fixture.rails.isEmpty, false)
    let main = RailFixture.Screen(
      id: "main", x: nil, width: nil, workY: nil, workHeight: nil, scale: nil, mirrored: nil)
    for (index, entry) in fixture.rails.enumerated() {
      let area = fixture.workArea
      let displays = (entry.displays ?? [main]).map { screen in
        Display(
          id: screen.id, name: screen.id, x: screen.x ?? 0, y: 0, width: screen.width ?? area.width,
          height: area.height, workY: screen.workY ?? area.workY,
          workHeight: screen.workHeight ?? area.workHeight, scale: screen.scale ?? entry.scale,
          mirrored: screen.mirrored ?? false)
      }
      let settings = Settings(
        enabled: entry.enabled ?? true, displayId: "main", edge: entry.edge, size: entry.size,
        providers: Array(ProviderId.allCases.prefix(entry.cells ?? ProviderId.allCases.count)),
        pullRequests: PullRequestSettings(enabled: false, lists: [.mine]))
      expectPixels(
        notchRailFrame(settings: settings, displays: displays).map(pixels), entry.rail,
        scale: entry.scale, "rail case \(index)")
      expectPixels(
        notchPillFrame(settings: settings, displays: displays).map(pixels), entry.pill,
        scale: entry.scale, "pill case \(index)")
    }
  }

  func testRailMetricsMatchTheSharedCases() throws {
    let fixture = try RailFixture.load()
    expectEqual(fixture.metrics.isEmpty, false)
    for entry in fixture.metrics {
      let layout = railLayout(size: entry.size, displayScale: entry.scale, edge: .right)
      expectPixels(
        [
          layout.thickness, layout.cellLength, layout.cellSpacing, layout.inset, layout.cellPadding,
          layout.contentSpacing, layout.iconSlot, layout.labelHeight, layout.ringStroke,
          layout.innerRingStroke, layout.innerRingInset, layout.glyphSize,
        ],
        [
          entry.thickness, entry.cellLength, entry.cellSpacing, entry.inset, entry.cellPadding,
          entry.contentSpacing, entry.iconSlot, entry.labelHeight, entry.ringStroke,
          entry.innerRingStroke, entry.innerRingInset, entry.glyphSize,
        ], scale: entry.scale, "\(entry.size) at \(entry.scale)x")
      let across = railLayout(size: entry.size, displayScale: entry.scale, edge: .top)
      expectEqual([across.thickness, across.cellLength], [layout.cellLength, layout.thickness])
    }
  }

  func testSettingsDecodeTheHostDocumentAndSkipUnknownProviders() throws {
    let document = try JSONDecoder().decode(
      Settings.self,
      from: Data(
        #"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["codex","codex"],"pullRequests":{"enabled":true,"lists":["mine","assigned"]},"popoverStyle":"solid"}"#
          .utf8))
    expectEqual(document.pullRequests.lists, [.mine, .assigned])
    expectEqual(document.show, ShowMode.onHover)
    expectEqual(document.edge, Edge.top)
    expectEqual(document.size, NotchSize.compact)
    expectEqual(document.cellCount, 2)
    expectEqual(document.popoverStyle, NotchStyle.solid)
    let newer = try JSONDecoder().decode(
      Settings.self,
      from: Data(
        #"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["gemini","claude"],"pullRequests":{"enabled":false,"lists":[]},"popoverStyle":"glass"}"#
          .utf8))
    expectEqual(newer.providers, [.claude])
    expectEqual(newer.cellCount, 1)
    expectThrows(
      try JSONDecoder().decode(
        Settings.self, from: Data(#"{"enabled":true,"displayId":"main","edge":"right"}"#.utf8)))
    for style in [#""#, #","popoverStyle":"frosted""#] {
      expectThrows(
        try JSONDecoder().decode(
          Settings.self,
          from: Data(
            (#"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["codex"],"pullRequests":{"enabled":true,"lists":["mine"]}"#
              + style + "}").utf8)))
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
    expectEqual(bar.thickness, layout.cellLength)
    expectEqual(
      railCellFrames(edge: .top, layout: bar, count: 2)[1].minX,
      bar.inset + bar.cellLength + bar.cellSpacing)
    expectEqual(cells[0].minY, layout.length(count: 4) - cells[3].maxY)

    let railFrame = notchRailFrame(settings: rail("main"), displays: [main])!
    let cell = cells[0].offsetBy(dx: railFrame.minX, dy: railFrame.minY)
    let size = CGSize(width: 300, height: 420)
    let frame = popoverFrame(cell: cell, edge: .right, size: size, display: main, scale: 1)
    expectEqual(frame.maxX, cell.minX - 2)
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

  let message =
    #"{"version":7,"sequence":1,"snapshot":{"settings":{"enabled":false,"displayId":null,"edge":"right","size":"standard","show":"always","providers":["claude"],"pullRequests":{"enabled":true,"lists":["mine"]},"popoverStyle":"glass"},"displays":[]},"cells":[]}"#

  func withCells(_ cells: String) -> String {
    message.replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(cells)]")
  }

  func providerCell(_ message: HostMessage, _ index: Int = 0) -> ProviderCell? {
    guard message.cells.indices.contains(index), case .provider(let cell) = message.cells[index]
    else { return nil }
    return cell
  }

  func pullRequestCell(_ message: HostMessage) -> PullRequestCell? {
    for cell in message.cells { if case .pullRequests(let pulls) = cell { return pulls } }
    return nil
  }

  func testProtocolRejectsUnsupportedVersionOversizeInvalidPercentAndBadSessions() throws {
    expectEqual(try HostMessage.decode(Data(message.utf8)).sequence, 1)
    for older in ["1", "2", "3", "4", "5", "6"] {
      expectThrows(
        try HostMessage.decode(
          Data(message.replacingOccurrences(of: "\"version\":7", with: "\"version\":\(older)").utf8)))
    }
    expectThrows(
      try HostMessage.decode(Data((message + String(repeating: " ", count: 262_144)).utf8)))
    let invalid = withCells(
      #"{"kind":"provider","provider":"claude","reading":{"status":"ok","windows":[{"id":"w","label":"Weekly","kind":"weekly","usedPercent":101,"observedAt":""}]},"sessions":[]}"#
    )
    expectThrows(try HostMessage.decode(Data(invalid.utf8)))
    let withSessions = withCells(
      #"{"kind":"provider","provider":"cursor","reading":{"status":"unsupported","windows":[]},"sessions":[{"id":"1","name":"repo-1a","place":"Terminal","project":"repo","status":"working","lastActiveAt":"2026-09-01T10:00:00Z"}]}"#
    )
    let decoded = try HostMessage.decode(Data(withSessions.utf8))
    expectEqual(providerCell(decoded)?.sessions.first?.isWorking, true)
    expectNil(providerCell(decoded)?.reading?.workspaceCredits)
    let withCredits = withCells(
      #"{"kind":"provider","provider":"codex","reading":{"status":"ok","windows":[],"innerRing":{"kind":"workspaceShare"},"workspaceCredits":{"usedPercent":32,"resetsAt":"2027-02-01T12:00:00+00:00","left":"17,000 of 25,000 left","renewed":"25,000 of 25,000 left"}},"sessions":[]}"#
    )
    expectEqual(
      providerCell(try HostMessage.decode(Data(withCredits.utf8)))?.reading?.inner?.quota.percent(
        at: now), 32)
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
    expectThrows(try HostMessage.decode(Data(withCells(#"{"kind":"weather"}"#).utf8)))
  }

  func testACellForAProviderTheHelperDoesNotKnowIsSkippedNotFatal() throws {
    let rail =
      message.replacingOccurrences(of: "\"enabled\":false", with: "\"enabled\":true")
      .replacingOccurrences(of: "\"providers\":[\"claude\"]", with: "\"providers\":[\"gemini\",\"claude\"]")
      .replacingOccurrences(of: "\"pullRequests\":{\"enabled\":true", with: "\"pullRequests\":{\"enabled\":false")
    let claude = #"{"kind":"provider","provider":"claude","notice":{"text":"Checking usage…","tone":"muted"},"sessions":[]}"#
    let gemini = #"{"kind":"provider","provider":"gemini","reading":{"status":"ok","windows":[]},"sessions":[]}"#
    let decoded = try HostMessage.decode(
      Data(rail.replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(gemini),\(claude)]").utf8))
    expectEqual(decoded.cells.map(\.id), [RailCell.provider(.claude)])
    expectEqual(providerCell(decoded)?.notice, Notice(text: "Checking usage…", tone: .muted))
    expectEqual(decoded.snapshot.settings.cellCount, 1)
    let muse = claude.replacingOccurrences(of: "\"provider\":\"claude\"", with: "\"provider\":\"muse\"")
    let reordered = try HostMessage.decode(
      Data(
        rail.replacingOccurrences(of: "[\"gemini\",\"claude\"]", with: "[\"claude\",\"muse\"]")
          .replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(muse),\(claude)]").utf8))
    expectEqual(reordered.cells.map(\.id), [RailCell.provider(.muse), .provider(.claude)])
    expectThrows(
      try HostMessage.decode(
        Data(rail.replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(claude),\(claude)]").utf8)))
    expectThrows(
      try HostMessage.decode(
        Data(
          rail.replacingOccurrences(of: "[\"gemini\",\"claude\"]", with: "[\"codex\",\"claude\"]")
            .replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(claude)]").utf8)))
  }

  func testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames() throws {
    let valid = withCells(
      #"{"kind":"provider","provider":"claude","reading":{"status":"ok","windows":[{"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":7,"observedAt":""},{"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":13,"observedAt":""}],"headlineWindowId":"weekly_all","innerRing":{"kind":"fable","windowId":"weekly_scoped:Fable"}},"sessions":[]}"#
    )
    let entry = providerCell(try HostMessage.decode(Data(valid.utf8)))?.reading
    expectEqual(entry?.headline?.usedPercent, 7)
    expectEqual(entry?.inner?.quota.usedPercent, 13)
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

  func testPullRequestsValidateLinksListsSegmentsAndCaps() throws {
    let pull =
      #"{"id":"node","number":42,"title":"ci: one concurrency group","url":"https://github.com/octo/tools/pull/42","repo":"octo/tools","author":"octocat","ci":"success","updatedAt":"2026-09-01T10:00:00Z","badges":[{"label":"Ready to merge","tone":"green"}]}"#
    let segment = #"{"ci":"success","passingWithConflicts":false}"#
    let withPulls = withCells(
      #"{"kind":"pullRequests","label":"1","reading":{"readable":true,"count":1,"segments":["# + segment
        + #"],"attention":{"conflicts":0,"failing":0,"changesRequested":0,"ready":1},"lists":[{"id":"mine","title":"Mine","total":1,"items":["#
        + pull + #"]},{"id":"assigned","title":"Assigned","total":1,"items":["# + pull + "]}]}}")
    let decoded = try HostMessage.decode(Data(withPulls.utf8))
    let pulls = pullRequestCell(decoded)?.reading
    expectEqual(pulls?.count, 1)
    expectEqual(pulls?.attention.ready, 1)
    expectEqual(pulls?.segments, [Segment(ci: .success, passingWithConflicts: false)])
    expectEqual(pulls?.lists.map(\.title), ["Mine", "Assigned"])
    expectEqual(pulls?.lists.first?.items.first?.link?.host, "github.com")
    expectEqual(pulls?.lists.first?.items.first?.ci, CiState.success)
    let newer = withPulls.replacingOccurrences(of: "\"ci\":\"success\",\"updatedAt\"", with: "\"ci\":\"skipped\",\"updatedAt\"")
    expectEqual(
      pullRequestCell(try HostMessage.decode(Data(newer.utf8)))?.reading?.lists.first?.items.first?.ci,
      CiState.unknown)
    let offsite = withPulls.replacingOccurrences(
      of: "https://github.com/octo/tools/pull/42", with: "http://example.com/pull/42")
    expectThrows(try HostMessage.decode(Data(offsite.utf8)))
    let duplicateLists = withPulls.replacingOccurrences(
      of: "\"lists\":[", with: "\"lists\":[{\"id\":\"mine\",\"title\":\"Mine\",\"total\":0,\"items\":[]},")
    expectThrows(try HostMessage.decode(Data(duplicateLists.utf8)))
    let tooManySegments = withPulls.replacingOccurrences(
      of: "\"segments\":[", with: "\"segments\":[" + String(repeating: segment + ",", count: maxRingSegments))
    expectThrows(try HostMessage.decode(Data(tooManySegments.utf8)))
    expectEqual(maxPullRequests, 25)
    expectEqual(maxRingSegments, 24)
  }

  func testRowsCarryTheirTagAndBadgesInTheTonesTheHostChose() throws {
    let pull: (Int, String) -> String = { number, extra in
      #"{"id":"node-\#(number)","number":\#(number),"title":"Fix","url":"https://github.com/acme/webapp/pull/\#(number)","repo":"acme/webapp","author":"octocat","ci":"success","updatedAt":"","badges":[]\#(extra)}"#
    }
    let rows = pullRequestCell(
      try HostMessage.decode(
        Data(
          withCells(
            #"{"kind":"pullRequests","label":"4","reading":{"readable":true,"count":4,"segments":[],"attention":{"conflicts":0,"failing":0,"changesRequested":0,"ready":0},"lists":[{"id":"mine","title":"Mine","total":4,"items":["#
              + [
                pull(1, #","tag":{"label":"Work","tone":"work"}"#),
                pull(2, #","tag":{"label":"Personal","tone":"personal"}"#),
                pull(3, #","tag":{"label":"Client","tone":"client"}"#), pull(4, ""),
              ].joined(separator: ",") + "]}]}}"
          ).utf8))
    )?.reading?.lists.first?.items
    expectEqual(
      rows?.map(\.tag),
      [
        Badge(label: "Work", tone: .work), Badge(label: "Personal", tone: .personal),
        Badge(label: "Client", tone: .muted), nil,
      ])
    expectEqual(rows?.allSatisfy(\.badges.isEmpty), true)
  }

  func goldenMessages() throws -> [HostMessage] {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .appendingPathComponent("host-message.json")
    let messages = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [Any] ?? []
    return try messages.map { try HostMessage.decode(JSONSerialization.data(withJSONObject: $0)) }
  }

  func testTheGoldenMessagesDecodeToTheCellsTheHostDecided() throws {
    let messages = try goldenMessages()
    expectEqual(messages.map(\.sequence), [1, 2])
    expectEqual(messages.map(\.version), [protocolVersion, protocolVersion])
    let first = messages[0]
    expectEqual(
      first.cells.map(\.id),
      [.provider(.claude), .provider(.codex), .provider(.muse), .pullRequests])
    for index in 0..<3 {
      expectNil(providerCell(first, index)?.reading)
      expectEqual(providerCell(first, index)?.notice, Notice(text: "Checking usage…", tone: .muted))
    }
    expectNil(pullRequestCell(first)?.reading)
    expectEqual(pullRequestCell(first)?.label, "—")
    expectEqual(
      pullRequestCell(first)?.notice, Notice(text: "Checking pull requests…", tone: .muted))
    expectNil(first.actionError)

    let golden = messages[1]
    expectEqual(golden.actionError, "Cannot open the Limits window.")
    expectEqual(
      golden.cells.map(\.id),
      [.provider(.claude), .provider(.codex), .provider(.muse), .pullRequests])
    expectEqual(golden.snapshot.settings.cellCount, golden.cells.count)

    let claude = providerCell(golden, 0)
    expectEqual(claude?.reading?.headline?.usedPercent, 41.5)
    expectEqual(claude?.reading?.inner, InnerQuota.fable(claude!.reading!.windows[2]))
    expectNil(claude?.notice)
    expectEqual(claude?.sessions.map(\.isWorking), [true, false])

    let codex = providerCell(golden, 1)
    expectEqual(codex?.reading?.status, "failed")
    expectEqual(codex?.notice, Notice(text: "Codex did not answer.", tone: .muted))
    expectEqual(
      codex?.paused, Notice(text: "Refresh paused. Last observed values below.", tone: .amber))
    expectEqual(codex?.reading?.workspaceCredits?.left, "16,999.5 of 25,000 left")
    expectEqual(codex?.reading?.inner?.quota.usedPercent, 32)

    let muse = providerCell(golden, 2)
    expectNil(muse?.reading)
    expectEqual(muse?.notice, Notice(text: "Checking usage…", tone: .muted))
    expectEqual(muse?.sessions.count, 1)

    let pulls = pullRequestCell(golden)
    expectEqual(pulls?.label, "3")
    expectEqual(
      pulls?.notice, Notice(text: "Showing the pull requests read at 10:00.", tone: .amber))
    let reading = pulls?.reading
    expectEqual(reading?.readable, true)
    expectEqual(reading?.count, 3)
    expectEqual(
      reading?.segments,
      [
        Segment(ci: .success, passingWithConflicts: false),
        Segment(ci: .failure, passingWithConflicts: false),
        Segment(ci: .success, passingWithConflicts: true),
      ])
    expectEqual(reading?.attention, Attention(conflicts: 1, failing: 1, changesRequested: 1, ready: 1))
    expectEqual(reading?.lists.map(\.title), ["Mine", "Review requested"])
    let mine = reading?.lists.first?.items
    expectEqual(
      mine?.first?.badges,
      [Badge(label: "Approved", tone: .green), Badge(label: "Ready to merge", tone: .green)])
    expectEqual(mine?.map(\.tag), [Badge(label: "Work", tone: .work), Badge(label: "Personal", tone: .personal)])
    expectEqual(
      mine?.last?.badges,
      [Badge(label: "Draft", tone: .muted), Badge(label: "Changes requested", tone: .red)])
    let review = reading?.lists.last?.items
    expectEqual(review?.map(\.id), ["node-4", "node-1"])
    expectEqual(review?.first?.badges, [Badge(label: "Conflicts", tone: .red)])
    expectNil(review?.first?.tag)
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
      (.ready, ["version": 7, "type": "ready"]),
      (.ack(sequence: 42), ["version": 7, "type": "ack", "sequence": 42]),
      (.screensChanged, ["version": 7, "type": "screensChanged"]),
      (.refresh, ["version": 7, "type": "refresh"]),
      (.openLimits, ["version": 7, "type": "openLimits"]),
      (.openPullRequests, ["version": 7, "type": "openPullRequests"]),
      (.setShow(.onHover), ["version": 7, "type": "setShow", "show": "onHover"]),
      (.setShow(.always), ["version": 7, "type": "setShow", "show": "always"]),
    ]
    for (action, expected) in cases {
      let encoded = try JSONEncoder().encode(action)
      let object = try JSONSerialization.jsonObject(with: encoded) as? NSDictionary
      expectEqual(object, expected as NSDictionary)
    }
  }
}

var failures = 0

struct RailFixture: Decodable {
  struct WorkArea: Decodable {
    let width, height, workY, workHeight: Double
  }
  struct Screen: Decodable {
    let id: String
    let x, width, workY, workHeight, scale: Double?
    let mirrored: Bool?
  }
  struct Rail: Decodable {
    let displays: [Screen]?
    let scale: Double
    let size: NotchSize
    let edge: Edge
    let cells: Int?
    let enabled: Bool?
    let rail: [Double]?
    let pill: [Double]?
  }
  struct Metrics: Decodable {
    let scale: Double
    let size: NotchSize
    let thickness, cellLength, cellSpacing, inset, cellPadding, contentSpacing, iconSlot: Double
    let labelHeight, ringStroke, innerRingStroke, innerRingInset, glyphSize: Double
  }
  let workArea: WorkArea
  let rails: [Rail]
  let metrics: [Metrics]

  static func load() throws -> RailFixture {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .appendingPathComponent("rail-cases.json")
    return try JSONDecoder().decode(RailFixture.self, from: Data(contentsOf: url))
  }
}

func pixels(_ frame: CGRect) -> [Double] {
  [Double(frame.minX), Double(frame.minY), Double(frame.width), Double(frame.height)]
}

@MainActor
func expectPixels(
  _ points: [Double]?, _ expected: [Double]?, scale: Double, _ label: String, line: Int = #line
) {
  let actual = points.map { $0.map { $0 * scale } }
  let matches: Bool
  switch (actual, expected) {
  case (nil, nil): matches = true
  case let (actual?, expected?):
    matches =
      actual.count == expected.count
      && zip(actual, expected).allSatisfy { abs($0 - $1) < 0.000_001 }
  default: matches = false
  }
  if !matches {
    failures += 1
    let shown = (String(describing: actual), String(describing: expected))
    print("FAIL line \(line): \(label) \(shown.0) != \(shown.1)")
  }
}
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
checks.testAPausedRingSaysItsReadingIsTheLastObserved()
checks.testWindowsRenewIndependentlyAndUnknownResetRemainsUsable()
checks.testSessionAgesReadLikeTheReferenceApp()
checks.testTheRailCountsEachProviderOnceAndThePullRequestCell()
try checks.testRailFramesAndPillsMatchTheSharedCases()
try checks.testRailMetricsMatchTheSharedCases()
try checks.testSettingsDecodeTheHostDocumentAndSkipUnknownProviders()
checks.testCellsTileTheRailAndPopoversStayInsideTheWorkArea()
try checks.testProtocolRejectsUnsupportedVersionOversizeInvalidPercentAndBadSessions()
try checks.testACellForAProviderTheHelperDoesNotKnowIsSkippedNotFatal()
try checks.testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames()
try checks.testPullRequestsValidateLinksListsSegmentsAndCaps()
try checks.testRowsCarryTheirTagAndBadgesInTheTonesTheHostChose()
try checks.testTheGoldenMessagesDecodeToTheCellsTheHostDecided()
checks.testReviewRequestsLinkTheTitleAndEscapeMarkup()
try checks.testClientActionsEncodeACompleteTypedProtocol()
print("20 native check groups; \(failures) failures")
exit(failures == 0 ? 0 : 1)
