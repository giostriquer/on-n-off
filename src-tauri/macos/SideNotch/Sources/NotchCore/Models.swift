import CoreGraphics
import Foundation

public let protocolVersion = 7
public let maxSessions = 12

public enum ProviderId: String, Codable, CaseIterable, Sendable {
  case claude, codex, muse, antigravity, cursor
}

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
    if let reset = parseInstant(resetsAt), reset <= now { return 0 }
    return usedPercent
  }

  public func isReached(at now: Date) -> Bool {
    guard let percent = percent(at: now) else { return false }
    return roundedPercent(percent) == 100
  }

  public func text(at now: Date) -> String {
    percent(at: now).map(formatPercent) ?? "—"
  }

  public func note(at now: Date) -> String {
    guard let reset = parseInstant(resetsAt) else { return "" }
    let formatter = DateFormatter()
    formatter.dateFormat = "EEE h:mm a"
    let clock = formatter.string(from: reset)
    if reset <= now { return "Reset \(clock)" }
    return "Resets \(clock)"
  }
}

public struct WorkspaceCredits: Codable, Equatable, Sendable {
  public let usedPercent: Double
  public let resetsAt: String?
  public let left: String
  public let renewed: String

  public init(usedPercent: Double, resetsAt: String?, left: String, renewed: String) {
    self.usedPercent = usedPercent
    self.resetsAt = resetsAt
    self.left = left
    self.renewed = renewed
  }

  public var quota: Quota {
    Quota(
      id: "workspace-credits", label: "Workspace credits", kind: "credits",
      usedPercent: usedPercent, resetsAt: resetsAt, observedAt: "")
  }

  public func amounts(at now: Date) -> String {
    (parseInstant(resetsAt).map { $0 <= now } ?? false) ? renewed : left
  }

  public func note(at now: Date) -> String {
    guard let reset = parseInstant(resetsAt) else { return "" }
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US")
    formatter.dateFormat = "MMM d"
    let date = formatter.string(from: reset)
    return reset <= now ? "Reset \(date)" : "Resets \(date)"
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

  public func age(at now: Date) -> String {
    guard let instant = parseInstant(lastActiveAt) else { return "" }
    let minutes = Int(now.timeIntervalSince(instant) / 60)
    if minutes < 1 { return "just now" }
    if minutes < 60 { return "\(minutes) min" }
    if minutes < 1440 { return "\(minutes / 60) h" }
    return "\(minutes / 1440) d"
  }
}

public enum InnerRing: Codable, Equatable, Sendable {
  case fable(windowId: String)
  case workspaceShare

  private enum CodingKeys: String, CodingKey { case kind, windowId }

  public init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    switch try values.decode(String.self, forKey: .kind) {
    case "fable": self = .fable(windowId: try values.decode(String.self, forKey: .windowId))
    case "workspaceShare": self = .workspaceShare
    default:
      throw DecodingError.dataCorruptedError(
        forKey: .kind, in: values, debugDescription: "Unknown inner ring")
    }
  }

  public func encode(to encoder: Encoder) throws {
    var values = encoder.container(keyedBy: CodingKeys.self)
    switch self {
    case .fable(let windowId):
      try values.encode("fable", forKey: .kind)
      try values.encode(windowId, forKey: .windowId)
    case .workspaceShare:
      try values.encode("workspaceShare", forKey: .kind)
    }
  }
}

public enum InnerQuota: Equatable, Sendable {
  case fable(Quota)
  case workspaceShare(Quota)

  public var quota: Quota {
    switch self {
    case .fable(let quota), .workspaceShare(let quota): return quota
    }
  }
}

public struct Reading: Codable, Equatable, Sendable {
  public let status: String
  public let message: String?
  public let windows: [Quota]
  public let headlineWindowId: String?
  public let innerRing: InnerRing?
  public let workspaceCredits: WorkspaceCredits?

  public init(
    status: String, message: String?, windows: [Quota], headlineWindowId: String? = nil,
    innerRing: InnerRing? = nil, workspaceCredits: WorkspaceCredits? = nil
  ) {
    self.status = status
    self.message = message
    self.windows = windows
    self.headlineWindowId = headlineWindowId
    self.innerRing = innerRing
    self.workspaceCredits = workspaceCredits
  }

  public var headline: Quota? {
    headlineWindowId.flatMap { id in windows.first { $0.id == id } }
  }

  public var inner: InnerQuota? {
    switch innerRing {
    case .fable(let id)?: return windows.first { $0.id == id }.map(InnerQuota.fable)
    case .workspaceShare?: return workspaceCredits.map { .workspaceShare($0.quota) }
    case nil: return nil
    }
  }

  public var referencesAreSent: Bool {
    (headlineWindowId == nil || headline != nil) && (innerRing == nil || inner != nil)
  }

  public var ringIsLastObserved: Bool { status != "ok" && headline != nil }
}

public enum Tone: String, Codable, Sendable {
  case muted, green, red, amber, work, personal
  public init(from decoder: Decoder) throws {
    self = try decodeLenient(decoder, unknown: .muted)
  }
}

public struct Notice: Codable, Equatable, Sendable {
  public let text: String
  public let tone: Tone
  public init(text: String, tone: Tone) {
    self.text = text
    self.tone = tone
  }
}

public struct Badge: Codable, Equatable, Sendable {
  public let label: String
  public let tone: Tone
  public init(label: String, tone: Tone) {
    self.label = label
    self.tone = tone
  }
}

public struct ProviderCell: Codable, Equatable, Sendable {
  public let provider: ProviderId
  public let reading: Reading?
  public let notice: Notice?
  public let paused: Notice?
  public let sessions: [Session]

  public init(
    provider: ProviderId, reading: Reading?, notice: Notice? = nil, paused: Notice? = nil,
    sessions: [Session] = []
  ) {
    self.provider = provider
    self.reading = reading
    self.notice = notice
    self.paused = paused
    self.sessions = sessions
  }
}

public enum Edge: String, Codable, Sendable {
  case left, right, top, bottom
  public var isVertical: Bool { self == .left || self == .right }
}

public enum ShowMode: String, Codable, Sendable { case always, onHover }

public enum GithubList: String, Codable, CaseIterable, Sendable {
  case mine, reviewRequested, assigned
}

public enum RailCell: Hashable, Sendable {
  case provider(ProviderId)
  case pullRequests
}

public enum NotchStyle: String, Codable, Sendable { case glass, solid }

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

public struct Settings: Codable, Equatable, Sendable {
  public var enabled: Bool
  public var displayId: String?
  public var edge: Edge
  public var size: NotchSize
  public var show: ShowMode
  public var popoverStyle: NotchStyle
  public init(
    enabled: Bool = false, displayId: String? = nil, edge: Edge = .right,
    size: NotchSize = .standard, show: ShowMode = .always, popoverStyle: NotchStyle = .glass
  ) {
    self.enabled = enabled
    self.displayId = displayId
    self.edge = edge
    self.size = size
    self.show = show
    self.popoverStyle = popoverStyle
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

public let maxPullRequests = 25
public let maxRingSegments = 24

private func decodeLenient<Value: RawRepresentable>(
  _ decoder: Decoder, unknown: Value
) throws -> Value where Value.RawValue == String {
  let raw = try decoder.singleValueContainer().decode(String.self)
  return Value(rawValue: raw) ?? unknown
}

public enum CiState: String, Codable, Sendable {
  case none, pending, success, failure, error, unknown
  public init(from decoder: Decoder) throws {
    self = try decodeLenient(decoder, unknown: .unknown)
  }
}

public struct PullRequest: Codable, Equatable, Identifiable, Sendable {
  public let id: String
  public let number: UInt64
  public let title: String
  public let url: String
  public let repo: String
  public let author: String
  public let ci: CiState
  public let updatedAt: String
  public let badges: [Badge]
  public let tag: Badge?

  public init(
    id: String, number: UInt64, title: String, url: String, repo: String, author: String,
    ci: CiState, updatedAt: String, badges: [Badge] = [], tag: Badge? = nil
  ) {
    self.id = id
    self.number = number
    self.title = title
    self.url = url
    self.repo = repo
    self.author = author
    self.ci = ci
    self.updatedAt = updatedAt
    self.badges = badges
    self.tag = tag
  }

  public var link: URL? {
    guard let url = URL(string: url), url.scheme == "https", url.host == "github.com" else {
      return nil
    }
    return url
  }
}

public struct PullRequestList: Codable, Equatable, Sendable {
  public let id: GithubList
  public let title: String
  public let total: UInt64
  public let items: [PullRequest]
  public init(id: GithubList, title: String, total: UInt64, items: [PullRequest]) {
    self.id = id
    self.title = title
    self.total = total
    self.items = items
  }
}

public struct Segment: Codable, Equatable, Sendable {
  public let ci: CiState
  public let passingWithConflicts: Bool
  public init(ci: CiState, passingWithConflicts: Bool) {
    self.ci = ci
    self.passingWithConflicts = passingWithConflicts
  }
}

public struct Attention: Codable, Equatable, Sendable {
  public let conflicts: Int
  public let failing: Int
  public let changesRequested: Int
  public let ready: Int
  public init(conflicts: Int, failing: Int, changesRequested: Int, ready: Int) {
    self.conflicts = conflicts
    self.failing = failing
    self.changesRequested = changesRequested
    self.ready = ready
  }
}

public struct PullRequestReading: Codable, Equatable, Sendable {
  public let readable: Bool
  public let count: Int
  public let segments: [Segment]
  public let attention: Attention
  public let lists: [PullRequestList]
  public init(
    readable: Bool, count: Int, segments: [Segment], attention: Attention,
    lists: [PullRequestList]
  ) {
    self.readable = readable
    self.count = count
    self.segments = segments
    self.attention = attention
    self.lists = lists
  }
}

public struct PullRequestCell: Codable, Equatable, Sendable {
  public let label: String
  public let reading: PullRequestReading?
  public let notice: Notice?
  public init(label: String, reading: PullRequestReading?, notice: Notice? = nil) {
    self.label = label
    self.reading = reading
    self.notice = notice
  }
  public var readable: Bool { reading?.readable == true }
}

public enum Cell: Equatable, Sendable {
  case provider(ProviderCell)
  case pullRequests(PullRequestCell)

  public var id: RailCell {
    switch self {
    case .provider(let cell): return .provider(cell.provider)
    case .pullRequests: return .pullRequests
    }
  }
}

private struct CellSlot: Decodable {
  let cell: Cell?
  private enum CodingKeys: String, CodingKey { case kind, provider }

  init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    switch try values.decode(String.self, forKey: .kind) {
    case "provider":
      let known = ProviderId(rawValue: try values.decode(String.self, forKey: .provider))
      cell = try known.map { _ in .provider(try ProviderCell(from: decoder)) }
    case "pullRequests":
      cell = .pullRequests(try PullRequestCell(from: decoder))
    default:
      throw DecodingError.dataCorruptedError(
        forKey: .kind, in: values, debugDescription: "Unknown cell")
    }
  }
}

public struct HostMessage: Decodable, Sendable {
  public let version: Int
  public let sequence: UInt64
  public let snapshot: Snapshot
  public let cells: [Cell]
  public let actionError: String?

  private enum CodingKeys: String, CodingKey {
    case version, sequence, snapshot, cells, actionError
  }

  public init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    version = try values.decode(Int.self, forKey: .version)
    sequence = try values.decode(UInt64.self, forKey: .sequence)
    snapshot = try values.decode(Snapshot.self, forKey: .snapshot)
    cells = try values.decode([CellSlot].self, forKey: .cells).compactMap(\.cell)
    actionError = try values.decodeIfPresent(String.self, forKey: .actionError)
  }

  public static func decode(_ data: Data) throws -> HostMessage {
    guard data.count <= 262_144 else { throw ProtocolError.invalidMessage }
    let value = try JSONDecoder().decode(Self.self, from: data)
    guard value.version == protocolVersion,
      Set(value.cells.map(\.id)).count == value.cells.count,
      value.cells.allSatisfy(\.isValid),
      value.snapshot.displays.count <= 64,
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

extension Cell {
  fileprivate var isValid: Bool {
    switch self {
    case .provider(let cell):
      return cell.sessions.count <= maxSessions
        && cell.sessions.allSatisfy { !$0.id.isEmpty && !$0.name.isEmpty }
        && (cell.reading.map { reading in
          reading.windows.count <= 100 && reading.referencesAreSent
            && reading.windows.allSatisfy {
              (0...100).contains($0.usedPercent) && $0.usedPercent.isFinite
            }
            && (reading.workspaceCredits.map {
              (0...100).contains($0.usedPercent) && $0.usedPercent.isFinite
            } ?? true)
        } ?? true)
    case .pullRequests(let cell):
      return cell.reading.map { pulls in
        pulls.segments.count <= maxRingSegments
          && pulls.lists.count <= GithubList.allCases.count
          && Set(pulls.lists.map(\.id)).count == pulls.lists.count
          && pulls.lists.allSatisfy { list in
            list.items.count <= maxPullRequests
              && list.items.allSatisfy { !$0.id.isEmpty && !$0.title.isEmpty && $0.link != nil }
          }
      } ?? true
    }
  }
}

public enum ProtocolError: Error { case invalidMessage }

public func pixelAligned(_ value: Double, displayScale: Double) -> Double {
  let scale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
  return (value * scale).rounded() / scale
}

public struct RailLayout: Equatable, Sendable {
  public let thickness: Double
  public let cellLength: Double
  public let cellSpacing: Double
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

public func notchRailFrame(settings: Settings, displays: [Display], count: Int) -> CGRect? {
  guard settings.enabled, count > 0,
    let display = selectedDisplay(settings: settings, displays: displays)
  else { return nil }
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

public func notchPillFrame(settings: Settings, displays: [Display], count: Int) -> CGRect? {
  guard let rail = notchRailFrame(settings: settings, displays: displays, count: count),
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

public func reviewRequestHtml(title: String, url: URL) -> String {
  "review please: <a href=\"\(escapeHtml(url.absoluteString))\">\(escapeHtml(title))</a>"
}

public func reviewRequestText(title: String, url: URL) -> String {
  "review please: \(title) \(url.absoluteString)"
}

private func escapeHtml(_ text: String) -> String {
  text.replacingOccurrences(of: "&", with: "&amp;")
    .replacingOccurrences(of: "<", with: "&lt;")
    .replacingOccurrences(of: ">", with: "&gt;")
    .replacingOccurrences(of: "\"", with: "&quot;")
}
