import CoreGraphics
import Foundation

/// Host ↔ helper message version. Bumped with every shape change so a stale helper fails loudly.
public let protocolVersion = 2
/// Rows per provider in a message; mirrors `MAX_SESSIONS` on the host.
public let maxSessions = 12

/// The providers the rail knows, in rail order. Matches the host's `AgentId` wire names.
public enum ProviderId: String, Codable, CaseIterable, Sendable {
  case claude, codex, antigravity, cursor
}
public let railProviderOrder = ProviderId.allCases

public struct Quota: Codable, Equatable, Identifiable, Sendable {
  public let id: String
  public let label: String
  public let kind: String
  public let usedPercent: Double
  public let resetsAt: String?
  public let observedAt: String

  public init(
    id: String, label: String, kind: String, usedPercent: Double, resetsAt: String?,
    observedAt: String
  ) {
    self.id = id
    self.label = label
    self.kind = kind
    self.usedPercent = usedPercent
    self.resetsAt = resetsAt
    self.observedAt = observedAt
  }

  public func percent(at now: Date) -> Double? {
    guard usedPercent.isFinite, (0...100).contains(usedPercent) else { return nil }
    if let reset = parseInstant(resetsAt), reset <= now { return nil }
    return usedPercent
  }

  public func isReached(at now: Date) -> Bool {
    guard let percent = percent(at: now) else { return false }
    return roundedPercent(percent) == 100
  }

  public func text(at now: Date) -> String {
    percent(at: now).map(formatPercent) ?? "—"
  }

  /// "Resets Tue 8:00 PM" while the window is pending; "Reset Tue 8:00 PM · last seen 97%"
  /// afterwards; empty when the provider reported no reset.
  public func note(at now: Date) -> String {
    guard let reset = parseInstant(resetsAt) else { return "" }
    let formatter = DateFormatter()
    formatter.dateFormat = "EEE h:mm a"
    let clock = formatter.string(from: reset)
    if reset <= now { return "Reset \(clock) · last seen \(formatPercent(usedPercent))" }
    return "Resets \(clock)"
  }
}

public struct Session: Codable, Equatable, Identifiable, Sendable {
  public let id: String
  public let name: String
  public let place: String
  public let project: String
  public let status: String
  public let lastActiveAt: String

  public init(
    id: String, name: String, place: String, project: String, status: String,
    lastActiveAt: String
  ) {
    self.id = id
    self.name = name
    self.place = place
    self.project = project
    self.status = status
    self.lastActiveAt = lastActiveAt
  }

  public var isWorking: Bool { status == "working" }

  /// "just now", "4 min", "2 h", "3 d" since the last activity.
  public func age(at now: Date) -> String {
    guard let instant = parseInstant(lastActiveAt) else { return "" }
    let minutes = Int(now.timeIntervalSince(instant) / 60)
    if minutes < 1 { return "just now" }
    if minutes < 60 { return "\(minutes) min" }
    if minutes < 1440 { return "\(minutes / 60) h" }
    return "\(minutes / 1440) d"
  }
}

public struct Provider: Codable, Equatable, Identifiable, Sendable {
  public var id: ProviderId { provider }
  public let provider: ProviderId
  public let status: String
  public let currentAccount: Bool
  public let plan: String?
  public let message: String?
  public let windows: [Quota]
  public let sessions: [Session]

  public init(
    provider: ProviderId, status: String, currentAccount: Bool, plan: String?, message: String?,
    windows: [Quota], sessions: [Session] = []
  ) {
    self.provider = provider
    self.status = status
    self.currentAccount = currentAccount
    self.plan = plan
    self.message = message
    self.windows = windows
    self.sessions = sessions
  }

  public var visibleWindows: [Quota] {
    guard provider == .codex else { return windows }
    return windows.filter { window in
      let label = window.label.split(separator: "·").last?.trimmingCharacters(in: .whitespaces)
        .lowercased()
      return !["gpt-reserve", "gpt-5.3-codex-spark"].contains(label ?? "")
        && !["base_model_inference", "codex_bengalfox"].contains { bucket in
          window.id == "extra:\(bucket)" || window.id.hasPrefix("extra:\(bucket):")
        }
    }
  }

  /// Windows in popover order: the current session first, then weekly, then per-model.
  public var orderedWindows: [Quota] {
    let priority = ["session": 0, "weekly": 1, "model": 2]
    return visibleWindows.sorted { priority[$0.kind, default: 3] < priority[$1.kind, default: 3] }
  }

  /// The outer ring's window: Claude's weekly limit (its 5-hour window is in the popover),
  /// otherwise the current session, falling back to weekly. Only for a readable current account.
  public var primary: Quota? {
    guard currentAccount, status == "ok" else { return nil }
    if provider == .claude { return visibleWindows.first { $0.kind == "weekly" } }
    return visibleWindows.first { $0.kind == "session" }
      ?? visibleWindows.first { $0.kind == "weekly" }
  }

  /// Claude's Fable weekly window, shown on an inner ring with its own label.
  public var fable: Quota? {
    guard provider == .claude, currentAccount, status == "ok" else { return nil }
    return visibleWindows.first {
      $0.kind == "model"
        && $0.label.trimmingCharacters(in: .whitespaces).lowercased() == "weekly · fable"
    }
  }
}

public enum Edge: String, Codable, Sendable {
  case left, right, top, bottom
  public var isVertical: Bool { self == .left || self == .right }
}

public enum ShowMode: String, Codable, Sendable { case always, onHover }

/// The Pull requests screen's three lists, in the order the popover shows them.
public enum GithubList: String, Codable, CaseIterable, Sendable {
  case mine, reviewRequested, assigned
  public var title: String {
    switch self {
    case .mine: return "Mine"
    case .reviewRequested: return "Review requested"
    case .assigned: return "Assigned"
    }
  }
}

public struct PullRequestSettings: Codable, Equatable, Sendable {
  public var enabled: Bool
  public var lists: [GithubList]
  public init(enabled: Bool = true, lists: [GithubList] = [.mine]) {
    self.enabled = enabled
    self.lists = lists
  }
  /// The selected lists in screen order, without duplicates.
  public var selectedLists: [GithubList] { GithubList.allCases.filter(lists.contains) }
}

/// One cell on the rail.
public enum RailCell: Hashable, Sendable {
  case provider(ProviderId)
  case pullRequests
}

public enum NotchSize: String, Codable, Sendable {
  case compact, standard, large

  public var scale: Double {
    switch self {
    case .compact: 0.875
    case .standard: 1.0
    case .large: 1.125
    }
  }
}

/// The host's settings document. Every field is present on the wire; legacy documents are
/// upgraded on the host before they reach the helper.
public struct Settings: Codable, Equatable, Sendable {
  public var enabled: Bool
  public var displayId: String?
  public var edge: Edge
  public var size: NotchSize
  public var show: ShowMode
  public var providers: [ProviderId]
  public var pullRequests: PullRequestSettings
  public init(
    enabled: Bool = false, displayId: String? = nil, edge: Edge = .right,
    size: NotchSize = .standard, show: ShowMode = .always,
    providers: [ProviderId] = railProviderOrder,
    pullRequests: PullRequestSettings = PullRequestSettings()
  ) {
    self.enabled = enabled
    self.displayId = displayId
    self.edge = edge
    self.size = size
    self.show = show
    self.providers = providers
    self.pullRequests = pullRequests
  }

  /// The selected providers in rail order, without duplicates.
  public var railProviders: [ProviderId] { railProviderOrder.filter(providers.contains) }

  /// Cells on the rail: one per selected provider, then the pull-request cell when it is on.
  public var railCells: [RailCell] {
    railProviders.map(RailCell.provider) + (pullRequests.enabled ? [.pullRequests] : [])
  }
}

public struct Display: Codable, Equatable, Identifiable, Sendable {
  public let id: String
  public let name: String
  public let x, y, width, height, workY, workHeight, scale: Double
  public let mirrored: Bool
  public init(
    id: String, name: String, x: Double, y: Double, width: Double, height: Double,
    workY: Double, workHeight: Double, scale: Double, mirrored: Bool
  ) {
    self.id = id
    self.name = name
    self.x = x
    self.y = y
    self.width = width
    self.height = height
    self.workY = workY
    self.workHeight = workHeight
    self.scale = scale
    self.mirrored = mirrored
  }
}

public enum ClientAction: Encodable, Sendable {
  case ready
  case ack(sequence: UInt64)
  case screensChanged
  case refresh
  case openLimits
  case openPullRequests
  /// The rail's pin control: always show the rail, or show it on hover.
  case setShow(ShowMode)

  private enum CodingKeys: String, CodingKey {
    case version, type, sequence, show
  }

  public func encode(to encoder: Encoder) throws {
    var values = encoder.container(keyedBy: CodingKeys.self)
    try values.encode(protocolVersion, forKey: .version)
    switch self {
    case .ready:
      try values.encode("ready", forKey: .type)
    case .ack(let sequence):
      try values.encode("ack", forKey: .type)
      try values.encode(sequence, forKey: .sequence)
    case .screensChanged:
      try values.encode("screensChanged", forKey: .type)
    case .refresh:
      try values.encode("refresh", forKey: .type)
    case .openLimits:
      try values.encode("openLimits", forKey: .type)
    case .openPullRequests:
      try values.encode("openPullRequests", forKey: .type)
    case .setShow(let show):
      try values.encode("setShow", forKey: .type)
      try values.encode(show, forKey: .show)
    }
  }
}

public struct Snapshot: Codable, Equatable, Sendable {
  public let settings: Settings
  public let displays: [Display]
  public let error: String?
}

/// Rows per list on the wire; mirrors `MAX_PULL_REQUESTS` on the host.
public let maxPullRequests = 25

public struct PullRequest: Codable, Equatable, Identifiable, Sendable {
  public let id: String
  public let number: UInt64
  public let title: String
  public let url: String
  public let repo: String
  public let author: String
  public let isDraft: Bool
  public let reviewDecision: String?
  public let ci: String
  public let mergeKind: String?
  public let updatedAt: String

  public init(
    id: String, number: UInt64, title: String, url: String, repo: String, author: String,
    isDraft: Bool, reviewDecision: String?, ci: String, mergeKind: String?, updatedAt: String
  ) {
    self.id = id
    self.number = number
    self.title = title
    self.url = url
    self.repo = repo
    self.author = author
    self.isDraft = isDraft
    self.reviewDecision = reviewDecision
    self.ci = ci
    self.mergeKind = mergeKind
    self.updatedAt = updatedAt
  }

  /// Only GitHub pages ever open from the notch.
  public var link: URL? {
    guard let url = URL(string: url), url.scheme == "https", url.host == "github.com" else {
      return nil
    }
    return url
  }
}

public struct PullRequestList: Codable, Equatable, Sendable {
  public let id: GithubList
  public let total: UInt64
  public let items: [PullRequest]
  public init(id: GithubList, total: UInt64, items: [PullRequest]) {
    self.id = id
    self.total = total
    self.items = items
  }
}

public struct PullRequests: Codable, Equatable, Sendable {
  public let status: String
  public let hint: String?
  public let stale: Bool
  public let lists: [PullRequestList]
  public init(status: String, hint: String?, stale: Bool, lists: [PullRequestList]) {
    self.status = status
    self.hint = hint
    self.stale = stale
    self.lists = lists
  }
  public var readable: Bool { status == "ok" || (stale && !lists.isEmpty) }
  public var count: Int { lists.reduce(0) { $0 + $1.items.count } }
  public var ready: Int {
    lists.reduce(0) { $0 + $1.items.filter { $0.mergeKind == "ready" }.count }
  }
  public var failing: Int {
    lists.reduce(0) { $0 + $1.items.filter { $0.ci == "failure" || $0.ci == "error" }.count }
  }
  public var changesRequested: Int {
    lists.reduce(0) { $0 + $1.items.filter { $0.reviewDecision == "CHANGES_REQUESTED" }.count }
  }
}

public struct HostMessage: Decodable, Sendable {
  public let version: Int
  public let sequence: UInt64
  public let snapshot: Snapshot
  public let providers: [Provider]
  public let pullRequests: PullRequests?
  public let actionError: String?

  public static func decode(_ data: Data) throws -> HostMessage {
    guard data.count <= 262_144 else { throw ProtocolError.invalidMessage }
    let value = try JSONDecoder().decode(Self.self, from: data)
    guard value.version == protocolVersion, value.providers.count <= railProviderOrder.count,
      Set(value.providers.map(\.provider)).count == value.providers.count,
      value.providers.allSatisfy({ entry in
        entry.currentAccount && entry.windows.count <= 100
          && entry.windows.allSatisfy {
            (0...100).contains($0.usedPercent) && $0.usedPercent.isFinite
          }
          && entry.sessions.count <= maxSessions
          && entry.sessions.allSatisfy { !$0.id.isEmpty && !$0.name.isEmpty }
      }),
      value.pullRequests.map({ pulls in
        pulls.lists.count <= GithubList.allCases.count
          && Set(pulls.lists.map(\.id)).count == pulls.lists.count
          && pulls.lists.allSatisfy { list in
            list.items.count <= maxPullRequests
              && list.items.allSatisfy { !$0.id.isEmpty && !$0.title.isEmpty && $0.link != nil }
          }
      }) ?? true, value.snapshot.displays.count <= 64,
      value.snapshot.displays.allSatisfy({ display in
        !display.id.isEmpty && display.scale > 0 && display.width > 0 && display.height > 0
          && [
            display.x, display.y, display.width, display.height, display.workY, display.workHeight,
            display.scale,
          ].allSatisfy(\.isFinite)
      })
    else { throw ProtocolError.invalidMessage }
    return value
  }
}

public enum ProtocolError: Error { case invalidMessage }

// MARK: - Geometry

/// Every native metric snaps to the target display's pixel grid so text and strokes never land
/// between pixels on 1x monitors.
public func pixelAligned(_ value: Double, displayScale: Double) -> Double {
  let scale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
  return (value * scale).rounded() / scale
}

/// Rail metrics in points for one size preset, one display, and one edge. Cells are 76 wide and
/// 73 tall on screen whatever the edge (icon slot with the rings, then the percent label): a
/// vertical rail is 76 thick with cells stacked along it, a horizontal bar is 73 thick with
/// cells side by side. Each end flares into the screen edge over `ear`
/// points and the first cell starts after that ear, so `inset == ear`.
public struct RailLayout: Equatable, Sendable {
  public let thickness: Double
  /// A cell's extent along the rail's axis.
  public let cellLength: Double
  public let cellSpacing: Double
  /// Distance from either end to the first cell; also the length of the ear curve. The cells
  /// are centred as a block, so both ends of the rail read the same.
  public let inset: Double
  public var ear: Double { inset }
  public let cellPadding: Double
  public let contentSpacing: Double
  public let iconSlot: Double
  public let ringStroke: Double
  public let innerRingStroke: Double
  public let innerRingInset: Double
  public let glyphSize: Double
  public let labelHeight: Double

  public func length(count: Int) -> Double {
    let count = Double(max(count, 0))
    return count * cellLength + max(count - 1, 0) * cellSpacing + 2 * inset
  }
}

public func railLayout(size: NotchSize, displayScale: Double, edge: Edge) -> RailLayout {
  func value(_ points: Double) -> Double {
    pixelAligned(points * size.scale, displayScale: displayScale)
  }
  let cellPadding = value(1)
  let contentSpacing = value(3)
  let iconSlot = value(46)
  // Tall enough for the 17 pt label at every preset, so the figure never scales to fit.
  let labelHeight = value(22)
  let cellWidth = value(76)
  let cellHeight = iconSlot + labelHeight + contentSpacing + 2 * cellPadding
  let vertical = edge.isVertical
  return RailLayout(
    thickness: vertical ? cellWidth : cellHeight,
    cellLength: vertical ? cellHeight : cellWidth,
    cellSpacing: value(8), inset: value(40), cellPadding: cellPadding, contentSpacing: contentSpacing, iconSlot: iconSlot,
    ringStroke: value(4), innerRingStroke: value(3), innerRingInset: value(6),
    glyphSize: value(24), labelHeight: labelHeight)
}

/// The rail's frame in top-left display coordinates, or `nil` while the notch must stay hidden:
/// disabled, no provider, the selected display missing, ambiguous, or mirrored, or a rail that
/// does not fit the work area. Mirrors `side_notch::model::layout` on the host.
public func notchRailFrame(settings: Settings, displays: [Display]) -> CGRect? {
  guard settings.enabled, let display = selectedDisplay(settings: settings, displays: displays)
  else { return nil }
  let count = settings.railCells.count
  guard count > 0 else { return nil }
  let layout = railLayout(size: settings.size, displayScale: display.scale, edge: settings.edge)
  let length = layout.length(count: count)
  let thickness = layout.thickness
  func aligned(_ value: Double) -> Double { pixelAligned(value, displayScale: display.scale) }
  if settings.edge.isVertical {
    guard length <= display.workHeight else { return nil }
    let x = settings.edge == .left ? display.x : display.x + display.width - thickness
    let y = display.workY + (display.workHeight - length) / 2
    return CGRect(x: aligned(x), y: aligned(y), width: thickness, height: length)
  }
  guard length <= display.width, thickness <= display.workHeight else { return nil }
  let x = display.x + (display.width - length) / 2
  let y = settings.edge == .top ? display.workY : display.workY + display.workHeight - thickness
  return CGRect(x: aligned(x), y: aligned(y), width: length, height: thickness)
}

/// The collapsed "show on hover" pill: a thin strip centred on the rail's span, flush with the
/// edge, whose hit area is what opens the rail.
public func notchPillFrame(settings: Settings, displays: [Display]) -> CGRect? {
  guard let rail = notchRailFrame(settings: settings, displays: displays),
    let display = selectedDisplay(settings: settings, displays: displays)
  else { return nil }
  func value(_ points: Double) -> Double {
    pixelAligned(points * settings.size.scale, displayScale: display.scale)
  }
  let thickness = value(6)
  let length = min(value(120), settings.edge.isVertical ? rail.height : rail.width)
  switch settings.edge {
  case .left:
    return CGRect(x: rail.minX, y: rail.midY - length / 2, width: thickness, height: length)
  case .right:
    return CGRect(
      x: rail.maxX - thickness, y: rail.midY - length / 2, width: thickness, height: length)
  case .top:
    return CGRect(x: rail.midX - length / 2, y: rail.minY, width: length, height: thickness)
  case .bottom:
    return CGRect(
      x: rail.midX - length / 2, y: rail.maxY - thickness, width: length, height: thickness)
  }
}

/// Cell frames inside the rail (top-left origin), one per selected provider in rail order.
public func railCellFrames(edge: Edge, layout: RailLayout, count: Int) -> [CGRect] {
  (0..<max(count, 0)).map { index in
    let offset = layout.inset + Double(index) * (layout.cellLength + layout.cellSpacing)
    return edge.isVertical
      ? CGRect(x: 0, y: offset, width: layout.thickness, height: layout.cellLength)
      : CGRect(x: offset, y: 0, width: layout.cellLength, height: layout.thickness)
  }
}

public let popoverWidth = 272.0
public let popoverGap = 2.0
public let popoverMargin = 8.0

/// Where a popover of `size` sits next to `cell` (a rail cell in top-left display coordinates):
/// inward from the edge, centred on the cell, and clamped to the display's work area.
public func popoverFrame(
  cell: CGRect, edge: Edge, size: CGSize, display: Display, scale: Double
) -> CGRect {
  let gap = pixelAligned(popoverGap * scale, displayScale: display.scale)
  let margin = pixelAligned(popoverMargin * scale, displayScale: display.scale)
  var origin: CGPoint
  switch edge {
  case .right: origin = CGPoint(x: cell.minX - gap - size.width, y: cell.midY - size.height / 2)
  case .left: origin = CGPoint(x: cell.maxX + gap, y: cell.midY - size.height / 2)
  case .top: origin = CGPoint(x: cell.midX - size.width / 2, y: cell.maxY + gap)
  case .bottom: origin = CGPoint(x: cell.midX - size.width / 2, y: cell.minY - gap - size.height)
  }
  let minX = display.x + margin
  let maxX = display.x + display.width - margin - size.width
  let minY = display.workY + margin
  let maxY = display.workY + display.workHeight - margin - size.height
  origin.x = min(max(origin.x, minX), max(minX, maxX))
  origin.y = min(max(origin.y, minY), max(minY, maxY))
  return CGRect(
    x: pixelAligned(origin.x, displayScale: display.scale),
    y: pixelAligned(origin.y, displayScale: display.scale), width: size.width,
    height: size.height)
}

public func selectedDisplay(settings: Settings, displays: [Display]) -> Display? {
  guard let id = settings.displayId else { return nil }
  let matches = displays.filter { $0.id == id }
  guard matches.count == 1, let display = matches.first, !display.mirrored else { return nil }
  return display
}

public func parseInstant(_ text: String?) -> Date? {
  guard let text = text else { return nil }
  let formatter = ISO8601DateFormatter()
  formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
  if let date = formatter.date(from: text) { return date }
  formatter.formatOptions = [.withInternetDateTime]
  return formatter.date(from: text)
}

public func formatPercent(_ percent: Double) -> String {
  percent > 0 && percent < 1 ? "<1%" : "\(roundedPercent(percent))%"
}

private func roundedPercent(_ percent: Double) -> Int {
  Int(percent.rounded())
}

/// The clipboard's rich-text form of a review request: “review please: <title>” with the title
/// linked, which Slack and other chat apps paste as a link.
public func reviewRequestHtml(title: String, url: URL) -> String {
  "review please: <a href=\"\(escapeHtml(url.absoluteString))\">\(escapeHtml(title))</a>"
}

/// The plain-text fallback for targets that drop rich text.
public func reviewRequestText(title: String, url: URL) -> String {
  "review please: \(title) \(url.absoluteString)"
}

private func escapeHtml(_ text: String) -> String {
  text.replacingOccurrences(of: "&", with: "&amp;")
    .replacingOccurrences(of: "<", with: "&lt;")
    .replacingOccurrences(of: ">", with: "&gt;")
    .replacingOccurrences(of: "\"", with: "&quot;")
}
