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
  func reading(windows: [Quota], headline: String? = nil, inner: InnerRing? = nil) -> Reading {
    Reading(windows: windows, headlineWindowId: headline, innerRing: inner)
  }
  func rail(_ displayId: String, edge: Edge = .right, size: NotchSize = .standard) -> Settings {
    Settings(enabled: true, displayId: displayId, edge: edge, size: size)
  }
  func display(_ id: String, x: Double = 0, mirrored: Bool = false, scale: Double = 2) -> Display
  {
    Display(
      id: id, name: "Same name", x: x, y: 0, width: 1728, height: 1117, workY: 33,
      workHeight: 1084, scale: scale, mirrored: mirrored)
  }

  func testEachMeterLevelDrawsInOneInk() {
    for base in ProviderId.allCases.map(providerInk) + [fableInk, creditsInk] {
      expectEqual(meterInk(.unreadable, base: base), unreadableInk)
      expectEqual(meterInk(.base, base: base), base)
      expectEqual(meterInk(.blend(0.5), base: base), base.mixed(toward: tripInk, 0.5))
      expectEqual(meterInk(.trip, base: base), tripInk)
    }
    expectEqual(meterInk(nil, base: claudeInk, at: now), unreadableInk)
  }

  func testTheSharedWindowCasesReadTheSameAtThePinnedInstant() throws {
    let cases = try WindowCases.load()
    NSTimeZone.default = TimeZone(identifier: cases.timeZone)!
    defer { NSTimeZone.resetSystemTimeZone(); NSTimeZone.default = .current }
    let at = parseInstant(cases.now)!
    func level(_ meter: WindowCases.Meter) -> MeterLevel? {
      switch (meter.level, meter.fraction) {
      case ("unreadable", nil): return .unreadable
      case ("base", nil): return .base
      case ("blend", let fraction?): return .blend(fraction)
      case ("trip", nil): return .trip
      default: return nil
      }
    }
    func sameLevel(_ shown: MeterLevel, _ expected: MeterLevel?) -> Bool {
      if case .blend(let a) = shown, case .blend(let b)? = expected { return abs(a - b) < 1e-9 }
      return shown == expected
    }
    func kind(_ cycle: ResetCycle) -> String {
      switch cycle {
      case .pending: return "pending"
      case .renewed: return "renewed"
      case .idle: return "idle"
      case .unknown: return "unknown"
      }
    }
    func check(_ entry: WindowCases.Case, _ quota: Quota, note: String) {
      let shown = (
        quota.percent(at: at), quota.isReached(at: at), kind(quota.cycle(at: at)),
        quota.text(at: at), note)
      let expected = (
        entry.usedNow, entry.reached, entry.note.kind, entry.usedText, entry.note.notch)
      if shown != expected || !sameLevel(meterLevel(quota, at: at), level(entry.meter)) {
        failures += 1
        print("FAIL window case \(entry.name): \(shown) != \(expected)")
      }
    }
    expectEqual(cases.windows.isEmpty || cases.shares.isEmpty, false)
    for entry in cases.windows {
      let input = entry.window!
      let quota = Quota(
        id: "w", label: "Weekly · all models", kind: "weekly",
        usedPercent: input.usedPercent ?? .nan, resetsAt: input.resetsAt, observedAt: "",
        startsOnFirstMessage: input.startsOnFirstMessage ?? false)
      check(entry, quota, note: quota.note(at: at))
    }
    for entry in cases.shares {
      let input = entry.share!
      let credits = WorkspaceCredits(
        usedPercent: input.usedPercent ?? .nan, resetsAt: input.resetsAt, left: "", renewed: "")
      check(entry, credits.quota, note: credits.note(at: at))
    }
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
  func testCodexCreditsFillTheInnerRingWhileTheWeeklyStaysOutside() {
    let entry = Reading(
      windows: [quota("weekly", 31, label: "Weekly · all models", id: "primary")],
      headlineWindowId: "primary", innerRing: .workspaceShare, workspaceCredits: credits(32))
    expectEqual(entry.headline?.usedPercent, 31)
    expectEqual(entry.inner, InnerQuota.workspaceShare(credits(32).quota))
    expectEqual(entry.inner?.quota.percent(at: now), 32)
    expectEqual(entry.inner?.quota.label, "Workspace credits")
    expectEqual(entry.windows.map(\.label), ["Weekly · all models"])
  }

  func testACreditSharePicksItsWordingByTheClock() {
    expectEqual(credits(32).amounts(at: now), "17,000 of 25,000 left")
    expectEqual(credits(100, left: "limit reached").amounts(at: now), "limit reached")
    expectEqual(credits(32, reset: nil).amounts(at: now), "17,000 of 25,000 left")
    expectEqual(
      credits(100, reset: "2026-06-15T12:00:00Z", left: "limit reached").amounts(at: now),
      "25,000 of 25,000 left")
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
        enabled: entry.enabled ?? true, displayId: "main", edge: entry.edge, size: entry.size)
      let count = entry.cells ?? ProviderId.allCases.count
      expectPixels(
        notchRailFrame(settings: settings, displays: displays, count: count).map(pixels),
        entry.rail, scale: entry.scale, "rail case \(index)")
      expectPixels(
        notchPillFrame(settings: settings, displays: displays, count: count).map(pixels),
        entry.pill,
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
      for (edge, cells) in [(Edge.right, entry.rightCells), (.top, entry.topCells)] {
        expectEqual(cells.isEmpty, false)
        let frames = railCellFrames(
          edge: edge, layout: railLayout(size: entry.size, displayScale: entry.scale, edge: edge),
          count: cells.count)
        expectEqual(frames.count, cells.count)
        for (index, (frame, cell)) in zip(frames, cells).enumerated() {
          expectPixels(
            pixels(frame), cell, scale: entry.scale,
            "\(edge) cell \(index) of \(entry.size) at \(entry.scale)x")
        }
      }
    }
  }

  func testSettingsDecodeWhatTheHelperDrawsAndIgnoreWhatOnlyTheHostReads() throws {
    let document = try JSONDecoder().decode(
      Settings.self,
      from: Data(
        #"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover","providers":["codex","gemini"],"pullRequests":{"enabled":true,"lists":["mine","starred"]},"popoverStyle":"solid"}"#
          .utf8))
    expectEqual(document.show, ShowMode.onHover)
    expectEqual(document.edge, Edge.top)
    expectEqual(document.size, NotchSize.compact)
    expectEqual(document.popoverStyle, NotchStyle.solid)
    expectThrows(
      try JSONDecoder().decode(
        Settings.self, from: Data(#"{"enabled":true,"displayId":"main","edge":"right"}"#.utf8)))
    for style in [#""#, #","popoverStyle":"frosted""#] {
      expectThrows(
        try JSONDecoder().decode(
          Settings.self,
          from: Data(
            (#"{"enabled":true,"displayId":"main","edge":"top","size":"compact","show":"onHover""#
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

    let railFrame = notchRailFrame(settings: rail("main"), displays: [main], count: 4)!
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

  func testProtocolRejectsUnsupportedVersionOversizeAndBadSessionsButNotAnUnreadableMeter() throws {
    expectEqual(try HostMessage.decode(Data(message.utf8)).sequence, 1)
    for older in ["1", "2", "3", "4", "5", "6"] {
      expectThrows(
        try HostMessage.decode(
          Data(message.replacingOccurrences(of: "\"version\":7", with: "\"version\":\(older)").utf8)))
    }
    expectThrows(
      try HostMessage.decode(Data((message + String(repeating: " ", count: 262_144)).utf8)))
    let window = #"{"id":"w","label":"Weekly","kind":"weekly","usedPercent":101,"observedAt":""}"#
    let pending = #"{"kind":"provider","provider":"claude","notice":{"text":"Checking limits…","tone":"muted"},"sessions":[]}"#
    for unreadable in [window, window.replacingOccurrences(of: "101", with: "null")] {
      let message = withCells(
        #"{"kind":"provider","provider":"codex","reading":{"windows":["# + unreadable
          + #"],"headlineWindowId":"w"},"sessions":[]},"# + pending)
      let decoded = try HostMessage.decode(Data(message.utf8))
      expectEqual(decoded.cells.map(\.id), [.provider(.codex), .provider(.claude)])
      expectNil(providerCell(decoded)?.reading?.headline?.percent(at: now))
      expectEqual(providerCell(decoded)?.reading?.headline?.text(at: now), "—")
    }
    let withSessions = withCells(
      #"{"kind":"provider","provider":"cursor","reading":{"windows":[]},"sessions":[{"id":"1","name":"repo-1a","place":"Terminal","project":"repo","status":"working","lastActiveAt":"2026-09-01T10:00:00Z"}]}"#
    )
    let decoded = try HostMessage.decode(Data(withSessions.utf8))
    expectEqual(providerCell(decoded)?.sessions.first?.isWorking, true)
    expectNil(providerCell(decoded)?.reading?.workspaceCredits)
    let withCredits = withCells(
      #"{"kind":"provider","provider":"codex","reading":{"windows":[],"innerRing":{"kind":"workspaceShare"},"workspaceCredits":{"usedPercent":32,"resetsAt":"2027-02-01T12:00:00+00:00","left":"17,000 of 25,000 left","renewed":"25,000 of 25,000 left"}},"sessions":[]}"#
    )
    expectEqual(
      providerCell(try HostMessage.decode(Data(withCredits.utf8)))?.reading?.inner?.quota.percent(
        at: now), 32)
    for unreadable in ["101", "null"] {
      let share = withCredits.replacingOccurrences(
        of: "\"usedPercent\":32", with: "\"usedPercent\":\(unreadable)")
      let decoded = providerCell(try HostMessage.decode(Data(share.utf8)))
      expectNil(decoded?.reading?.inner?.quota.percent(at: now))
      expectEqual(decoded?.reading?.workspaceCredits?.left, "17,000 of 25,000 left")
    }
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
    let claude = #"{"kind":"provider","provider":"claude","notice":{"text":"Checking limits…","tone":"muted"},"sessions":[]}"#
    let gemini = #"{"kind":"provider","provider":"gemini","reading":{"windows":[]},"sessions":[]}"#
    let decoded = try HostMessage.decode(
      Data(rail.replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(gemini),\(claude)]").utf8))
    expectEqual(decoded.cells.map(\.id), [RailCell.provider(.claude)])
    expectEqual(providerCell(decoded)?.notice, Notice(text: "Checking limits…", tone: .muted))
    let muse = claude.replacingOccurrences(of: "\"provider\":\"claude\"", with: "\"provider\":\"muse\"")
    let reordered = try HostMessage.decode(
      Data(
        rail.replacingOccurrences(of: "[\"gemini\",\"claude\"]", with: "[\"claude\",\"muse\"]")
          .replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(muse),\(claude)]").utf8))
    expectEqual(reordered.cells.map(\.id), [RailCell.provider(.muse), .provider(.claude)])
    expectThrows(
      try HostMessage.decode(
        Data(rail.replacingOccurrences(of: "\"cells\":[]", with: "\"cells\":[\(claude),\(claude)]").utf8)))
  }

  func testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames() throws {
    let valid = withCells(
      #"{"kind":"provider","provider":"claude","reading":{"windows":[{"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":7,"observedAt":""},{"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":13,"observedAt":""}],"headlineWindowId":"weekly_all","innerRing":{"kind":"fable","windowId":"weekly_scoped:Fable"}},"sessions":[]}"#
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
      expectEqual(providerCell(first, index)?.notice, Notice(text: "Checking limits…", tone: .muted))
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

    let claude = providerCell(golden, 0)
    expectEqual(claude?.reading?.headline?.usedPercent, 41.5)
    let idle = claude?.reading?.windows.first { $0.id == "session" }
    expectEqual(idle?.cycle(at: now), .idle)
    expectEqual(idle?.note(at: now), "Starts with first message")
    expectEqual(claude?.reading?.inner, InnerQuota.fable(claude!.reading!.windows[2]))
    expectNil(claude?.notice)
    expectEqual(claude?.sessions.map(\.isWorking), [true, false])

    let codex = providerCell(golden, 1)
    expectEqual(codex?.notice, Notice(text: "Codex did not answer.", tone: .red))
    expectEqual(
      codex?.paused, Notice(text: "Refresh paused. Last observed values below.", tone: .amber))
    expectEqual(codex?.reading?.workspaceCredits?.left, "16,999.5 of 25,000 left")
    expectEqual(codex?.reading?.inner?.quota.usedPercent, 32)

    let muse = providerCell(golden, 2)
    expectNil(muse?.reading)
    expectEqual(muse?.notice, Notice(text: "Checking limits…", tone: .muted))
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

  func testVoiceOverSpeaksEachCellFromWhatTheHostDecided() {
    let weekly = quota("weekly", 46, id: "weekly")
    let fable = quota("model", 100, label: "Weekly · Fable", id: "fable")
    expectEqual(ProviderCell(provider: .claude, reading: nil).spoken(name: "Claude", at: now), "Claude, updating")
    expectEqual(
      ProviderCell(
        provider: .claude,
        reading: reading(windows: [weekly, fable], headline: "weekly", inner: .fable(windowId: "fable"))
      ).spoken(name: "Claude", at: now),
      "Claude, weekly, 46% used, Fable weekly, 100% used, limit reached")
    expectEqual(
      ProviderCell(
        provider: .codex, reading: reading(windows: [quota("weekly", 100)], headline: "weekly")
      ).spoken(name: "Codex", at: now),
      "Codex, weekly, 100% used, limit reached")
    expectEqual(
      ProviderCell(
        provider: .claude,
        reading: reading(
          windows: [quota("weekly", 101), quota("model", .nan, label: "Weekly · Fable", id: "fable")],
          headline: "weekly", inner: .fable(windowId: "fable"))
      ).spoken(name: "Claude", at: now),
      "Claude, weekly, usage unavailable, Fable weekly, usage unavailable")
    expectEqual(
      ProviderCell(
        provider: .claude, reading: reading(windows: [weekly], headline: "weekly"),
        notice: Notice(text: "Claude Code reported no usage.", tone: .red),
        paused: Notice(text: "Refresh paused. Last observed values below.", tone: .amber)
      ).spoken(name: "Claude", at: now),
      "Claude, weekly, 46% used, refresh paused, last observed")
    expectEqual(
      ProviderCell(
        provider: .cursor,
        reading: Reading(windows: []),
        notice: Notice(text: "Cursor has no subscription limits to show.", tone: .muted)
      ).spoken(name: "Cursor", at: now),
      "Cursor, Cursor has no subscription limits to show.")
    expectEqual(
      ProviderCell(
        provider: .codex, reading: reading(windows: []),
        notice: Notice(text: "Codex limits are unavailable.", tone: .red)
      ).spoken(name: "Codex", at: now),
      "Codex, Codex limits are unavailable.")
    expectEqual(
      ProviderCell(
        provider: .muse,
        reading: Reading(windows: [])
      ).spoken(name: "Muse", at: now),
      "Muse, unavailable")

    let attention = Attention(conflicts: 1, failing: 2, changesRequested: 0, ready: 1)
    expectEqual(PullRequestCell(label: "—", reading: nil).spoken, "Pull requests, updating")
    expectEqual(
      PullRequestCell(
        label: "—",
        reading: PullRequestReading(
          readable: false, count: 0, segments: [],
          attention: Attention(conflicts: 0, failing: 0, changesRequested: 0, ready: 0), lists: []),
        notice: Notice(text: "Sign in with the GitHub CLI.", tone: .muted)
      ).spoken, "Pull requests, Sign in with the GitHub CLI.")
    expectEqual(
      PullRequestCell(
        label: "3",
        reading: PullRequestReading(readable: true, count: 3, segments: [], attention: attention, lists: [])
      ).spoken, "Pull requests, 3 open, 1 with merge conflicts, 2 failing CI, 1 ready to merge")
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

struct WindowCases: Decodable {
  struct Input: Decodable {
    let usedPercent: Double?
    let resetsAt: String?
    let startsOnFirstMessage: Bool?
  }
  struct Meter: Decodable {
    let level: String
    let fraction: Double?
  }
  struct Note: Decodable {
    let kind: String
    let notch: String
  }
  struct Case: Decodable {
    let name: String
    let window: Input?
    let share: Input?
    let usedNow: Double?
    let reached: Bool
    let usedText: String
    let meter: Meter
    let note: Note
  }
  let now: String
  let timeZone: String
  let windows: [Case]
  let shares: [Case]

  static func load() throws -> WindowCases {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .appendingPathComponent("../../../../src/limits/window_at/fixtures/window-cases.json")
      .standardized
    return try JSONDecoder().decode(WindowCases.self, from: Data(contentsOf: url))
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
    let rightCells, topCells: [[Double]]
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
checks.testEachMeterLevelDrawsInOneInk()
try checks.testTheSharedWindowCasesReadTheSameAtThePinnedInstant()
checks.testTheRingsShowTheWindowsTheHostNamed()
checks.testCodexCreditsFillTheInnerRingWhileTheWeeklyStaysOutside()
checks.testACreditSharePicksItsWordingByTheClock()
checks.testSessionAgesReadLikeTheReferenceApp()
try checks.testRailFramesAndPillsMatchTheSharedCases()
try checks.testRailMetricsMatchTheSharedCases()
try checks.testSettingsDecodeWhatTheHelperDrawsAndIgnoreWhatOnlyTheHostReads()
checks.testCellsTileTheRailAndPopoversStayInsideTheWorkArea()
try checks.testProtocolRejectsUnsupportedVersionOversizeAndBadSessionsButNotAnUnreadableMeter()
try checks.testACellForAProviderTheHelperDoesNotKnowIsSkippedNotFatal()
try checks.testProtocolCarriesTheRingsByWindowIdAndRefusesDanglingNames()
try checks.testPullRequestsValidateLinksListsSegmentsAndCaps()
try checks.testRowsCarryTheirTagAndBadgesInTheTonesTheHostChose()
try checks.testTheGoldenMessagesDecodeToTheCellsTheHostDecided()
checks.testVoiceOverSpeaksEachCellFromWhatTheHostDecided()
checks.testReviewRequestsLinkTheTitleAndEscapeMarkup()
try checks.testClientActionsEncodeACompleteTypedProtocol()
print("19 native check groups; \(failures) failures")
exit(failures == 0 ? 0 : 1)
