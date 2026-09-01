import CoreGraphics
import Foundation

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

  public func note(at now: Date) -> String {
    guard let reset = parseInstant(resetsAt) else { return "" }
    let formatter = DateFormatter()
    formatter.dateFormat = "EEE HH:mm"
    let clock = formatter.string(from: reset)
    if reset <= now { return "Reset \(clock) · last seen \(formatPercent(usedPercent))" }
    let minutes = Int(reset.timeIntervalSince(now) / 60)
    let remaining =
      minutes >= 1440
      ? "\(minutes / 1440)d \((minutes % 1440) / 60)h"
      : minutes >= 60 ? "\(minutes / 60)h \(minutes % 60)m" : minutes > 0 ? "\(minutes)m" : "<1m"
    return "Resets in \(remaining) · \(clock)"
  }
}

public struct Provider: Codable, Equatable, Identifiable, Sendable {
  public var id: String { provider }
  public let provider: String
  public let status: String
  public let currentAccount: Bool
  public let plan: String?
  public let message: String?
  public let windows: [Quota]

  public init(
    provider: String, status: String, currentAccount: Bool, plan: String?, message: String?,
    windows: [Quota]
  ) {
    self.provider = provider
    self.status = status
    self.currentAccount = currentAccount
    self.plan = plan
    self.message = message
    self.windows = windows
  }

  public var visibleWindows: [Quota] {
    guard provider == "codex" else { return windows }
    return windows.filter { window in
      let label = window.label.split(separator: "·").last?.trimmingCharacters(in: .whitespaces)
        .lowercased()
      return !["gpt-reserve", "gpt-5.3-codex-spark"].contains(label ?? "")
        && !["base_model_inference", "codex_bengalfox"].contains { bucket in
          window.id == "extra:\(bucket)" || window.id.hasPrefix("extra:\(bucket):")
        }
    }
  }
  public var primary: Quota? {
    guard currentAccount, status == "ok" else { return nil }
    if provider == "claude" { return visibleWindows.first { $0.kind == "weekly" } }
    return visibleWindows.first { $0.kind == "session" }
      ?? visibleWindows.first { $0.kind == "weekly" }
  }
  public var fable: Quota? {
    guard provider == "claude", currentAccount, status == "ok" else { return nil }
    return visibleWindows.first {
      $0.kind == "model"
        && $0.label.trimmingCharacters(in: .whitespaces).lowercased() == "weekly · fable"
    }
  }
}

public enum Edge: String, Codable, Sendable { case left, right }
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

public struct MeterRailLayout: Equatable, Sendable {
  public let railWidth: Double
  public let railHeight: Double
  public let cellWidth: Double
  public let cellHeight: Double
  public let cellSpacing: Double
  public let cellPadding: Double
  public let contentSpacing: Double
  public let iconSlotSize: Double
  public let primarySlotHeight: Double
  public let auxiliarySlotHeight: Double
  public let primarySlotWidth: Double
  public let auxiliarySlotWidth: Double
  public let ringInset: Double
  public let columnOffsetX: Double
  public let primaryOffsetX: Double
  public let columnOffsetY: Double
  public let stackInset: Double
}

public func meterRailLayout(size: NotchSize, displayScale: Double) -> MeterRailLayout {
  let pixelScale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
  let scale = size.scale
  func value(_ points: Double) -> Double {
    pixelAligned(points * scale, displayScale: pixelScale)
  }

  let railWidth = value(76)
  let railHeight = value(340)
  let cellPadding = value(3)
  let contentSpacing = value(4)
  let iconSlotSize = value(48)
  let primarySlotHeight = value(20)
  let auxiliarySlotHeight = value(12)
  let cellHeight =
    iconSlotSize + primarySlotHeight + auxiliarySlotHeight + (2 * contentSpacing)
    + (2 * cellPadding)
  var cellSpacing = value(16)
  let remainingPixels = Int(
    ((railHeight - (2 * cellHeight) - cellSpacing) * pixelScale).rounded())
  if !remainingPixels.isMultiple(of: 2) {
    cellSpacing += 1 / pixelScale
  }
  let stackInset = (railHeight - (2 * cellHeight) - cellSpacing) / 2
  let columnOffsetY = pixelAligned(
    (primarySlotHeight + auxiliarySlotHeight + (2 * contentSpacing)) / 2,
    displayScale: pixelScale)

  return MeterRailLayout(
    railWidth: railWidth, railHeight: railHeight, cellWidth: value(62),
    cellHeight: cellHeight, cellSpacing: cellSpacing, cellPadding: cellPadding,
    contentSpacing: contentSpacing, iconSlotSize: iconSlotSize,
    primarySlotHeight: primarySlotHeight, auxiliarySlotHeight: auxiliarySlotHeight,
    primarySlotWidth: value(54), auxiliarySlotWidth: value(58), ringInset: value(2),
    columnOffsetX: 0, primaryOffsetX: 1 / pixelScale, columnOffsetY: columnOffsetY,
    stackInset: stackInset)
}

public struct Settings: Codable, Equatable, Sendable {
  public var enabled: Bool
  public var displayId: String?
  public var edge: Edge
  public var size: NotchSize
  public init(
    enabled: Bool = false, displayId: String? = nil, edge: Edge = .right,
    size: NotchSize = .standard
  ) {
    self.enabled = enabled
    self.displayId = displayId
    self.edge = edge
    self.size = size
  }

  private enum CodingKeys: String, CodingKey { case enabled, displayId, edge, size }

  public init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    enabled = try values.decode(Bool.self, forKey: .enabled)
    displayId = try values.decodeIfPresent(String.self, forKey: .displayId)
    edge = try values.decode(Edge.self, forKey: .edge)
    size = try values.decodeIfPresent(NotchSize.self, forKey: .size) ?? .standard
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

  private enum CodingKeys: String, CodingKey {
    case version, type, sequence
  }

  public func encode(to encoder: Encoder) throws {
    var values = encoder.container(keyedBy: CodingKeys.self)
    try values.encode(1, forKey: .version)
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
    }
  }
}

public struct Snapshot: Codable, Equatable, Sendable {
  public let settings: Settings
  public let displays: [Display]
  public let error: String?
}

public struct HostMessage: Decodable, Sendable {
  public let version: Int
  public let sequence: UInt64
  public let snapshot: Snapshot
  public let providers: [Provider]
  public let actionError: String?

  public static func decode(_ data: Data) throws -> HostMessage {
    guard data.count <= 262_144 else { throw ProtocolError.invalidMessage }
    let value = try JSONDecoder().decode(Self.self, from: data)
    guard value.version == 1, value.providers.count <= 2,
      Set(value.providers.map(\.provider)).count == value.providers.count,
      value.providers.allSatisfy({ entry in
        ["claude", "codex"].contains(entry.provider) && entry.currentAccount
          && entry.windows.count <= 100
          && entry.windows.allSatisfy {
            (0...100).contains($0.usedPercent) && $0.usedPercent.isFinite
          }
      }), value.snapshot.displays.count <= 64,
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

public func notchFrame(settings: Settings, displays: [Display], expanded: Bool) -> CGRect? {
  guard settings.enabled, let id = settings.displayId else { return nil }
  let matches = displays.filter { $0.id == id }
  guard matches.count == 1, let display = matches.first, !display.mirrored else { return nil }
  let scale = settings.size.scale
  let railWidth = pixelAligned(76.0 * scale, displayScale: display.scale)
  let width = min(
    pixelAligned((expanded ? 388.0 : 76.0) * scale, displayScale: display.scale),
    display.width)
  let height = min(
    pixelAligned(340.0 * scale, displayScale: display.scale), display.workHeight)
  guard width >= railWidth, height >= 180 * scale else { return nil }
  let x = settings.edge == .left ? display.x : display.x + display.width - width
  let y = display.workY + (display.workHeight - height) / 2
  return CGRect(
    x: pixelAligned(x, displayScale: display.scale),
    y: pixelAligned(y, displayScale: display.scale), width: width, height: height)
}

private func pixelAligned(_ value: Double, displayScale: Double) -> Double {
  (value * displayScale).rounded() / displayScale
}

public struct NotchPanelFrames: Equatable, Sendable {
  public let rail: CGRect
  public let detail: CGRect
  public let combined: CGRect
}

public func notchPanelFrames(settings: Settings, displays: [Display]) -> NotchPanelFrames? {
  guard
    let rail = notchFrame(settings: settings, displays: displays, expanded: false),
    let combined = notchFrame(settings: settings, displays: displays, expanded: true)
  else { return nil }
  let detailWidth = combined.width - rail.width
  guard detailWidth > 0 else { return nil }
  let detail = CGRect(
    x: settings.edge == .left ? rail.maxX : combined.minX,
    y: combined.minY, width: detailWidth, height: combined.height)
  return NotchPanelFrames(rail: rail, detail: detail, combined: combined)
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
