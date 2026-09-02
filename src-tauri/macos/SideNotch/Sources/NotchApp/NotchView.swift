import NotchCore
import SwiftUI

// The app's own palette (`providerStyle.ts` / `tokens.css`, dark theme): each provider's accent
// for its ring and bars, amber from 70 % and red from 90 % like the Limits screen.
let claudeOrange = Color(red: 232 / 255, green: 148 / 255, blue: 74 / 255)
let codexInk = Color(red: 238 / 255, green: 240 / 255, blue: 242 / 255)
let cursorBlue = Color(red: 122 / 255, green: 162 / 255, blue: 255 / 255)
let antigravityMute = Color(red: 140 / 255, green: 147 / 255, blue: 157 / 255)
let fableOrange = Color(red: 247 / 255, green: 173 / 255, blue: 113 / 255)
let warnAmber = Color(red: 224 / 255, green: 179 / 255, blue: 65 / 255)
let tripRed = Color(red: 226 / 255, green: 89 / 255, blue: 76 / 255)
let railInk = Color(white: 0.03)
let popoverInk = Color(red: 0.055, green: 0.055, blue: 0.065)
let mutedInk = Color(white: 0.6)

func providerColor(_ id: ProviderId) -> Color {
  switch id {
  case .claude: return claudeOrange
  case .codex: return codexInk
  case .cursor: return cursorBlue
  case .antigravity: return antigravityMute
  }
}

/// The provider's accent while there is room, amber from 70 %, red from 90 %; grey when unreadable.
func meterColor(_ quota: Quota?, provider: ProviderId, at now: Date) -> Color {
  meterColor(quota, base: providerColor(provider), at: now)
}

func meterColor(_ quota: Quota?, base: Color, at now: Date) -> Color {
  guard let percent = quota?.percent(at: now) else { return Color(white: 0.3) }
  if percent >= 90 { return tripRed }
  if percent >= 70 { return warnAmber }
  return base
}

/// The colour of one pull request's CI rollup on the ring and in the popover.
func ciColor(_ ci: CiState) -> Color {
  switch ci {
  case .success: return liveGreen
  case .failure, .error: return tripRed
  case .pending: return warnAmber
  case .none, .unknown: return Color(white: 0.3)
  }
}

func providerName(_ id: ProviderId) -> String {
  switch id {
  case .claude: return "Claude"
  case .codex: return "Codex"
  case .antigravity: return "Antigravity"
  case .cursor: return "Cursor"
  }
}

/// Where and how the rail draws, computed once per host message or screen change from the
/// settings and the display list; every view and pointer computation reads this value.
struct RailModel {
  let settings: NotchCore.Settings
  let display: Display
  /// The rail in top-left display coordinates.
  let frame: CGRect
  let layout: RailLayout
  let metrics: NotchMetrics

  init?(settings: NotchCore.Settings, displays: [Display]) {
    guard let display = selectedDisplay(settings: settings, displays: displays),
      let frame = notchRailFrame(settings: settings, displays: displays)
    else { return nil }
    self.settings = settings
    self.display = display
    self.frame = frame
    layout = railLayout(size: settings.size, displayScale: display.scale, edge: settings.edge)
    metrics = NotchMetrics(scale: CGFloat(settings.size.scale), backingScale: CGFloat(display.scale))
  }

  var cellIds: [RailCell] { settings.railCells }
  var edge: NotchCore.Edge { settings.edge }

  /// Cell frames in the rail's own top-left coordinates.
  var cells: [CGRect] { railCellFrames(edge: edge, layout: layout, count: cellIds.count) }

  /// The cell containing a point in the rail's own top-left coordinates.
  func cell(at point: CGPoint) -> RailCell? {
    cells.firstIndex { $0.contains(point) }.map { cellIds[$0] }
  }

  /// One cell's frame in top-left display coordinates.
  func cellFrame(of id: RailCell) -> CGRect? {
    cellIds.firstIndex(of: id).map { cells[$0].offsetBy(dx: frame.minX, dy: frame.minY) }
  }

  /// The rail's first ear, in its own top-left coordinates: the cap that toggles how the rail
  /// shows. It lights up on hover; the pin sits at the visible part's centroid, which leans
  /// toward the screen edge because the silhouette narrows away from it.
  var capRect: CGRect {
    edge.isVertical
      ? CGRect(x: 0, y: 0, width: layout.thickness, height: layout.ear)
      : CGRect(x: 0, y: 0, width: layout.ear, height: layout.thickness)
  }

  var capIconCenter: CGPoint {
    let along = layout.ear * 0.62
    let towardEdge = layout.thickness * 0.62
    let awayFromEdge = layout.thickness - towardEdge
    switch edge {
    case .right: return CGPoint(x: towardEdge, y: along)
    case .left: return CGPoint(x: awayFromEdge, y: along)
    case .top: return CGPoint(x: along, y: awayFromEdge)
    case .bottom: return CGPoint(x: along, y: towardEdge)
    }
  }
}

struct NotchRailView: View {
  let model: RailModel
  let entries: [ProviderId: Provider]
  let pullRequests: PullRequests?
  let now: Date
  let active: RailCell?
  let capHovered: Bool
  let action: (RailCell) -> Void
  let toggleShow: () -> Void

  var body: some View {
    let layout = model.layout
    ZStack {
      NotchSilhouette(edge: model.edge, ear: CGFloat(layout.ear)).fill(railInk)
      if model.edge.isVertical {
        VStack(spacing: CGFloat(layout.cellSpacing)) {
          ForEach(model.cellIds, id: \.self) { cell($0) }
        }
      } else {
        HStack(spacing: CGFloat(layout.cellSpacing)) {
          ForEach(model.cellIds, id: \.self) { cell($0) }
        }
      }
    }
    .frame(width: model.frame.width, height: model.frame.height)
    .overlay(ShowToggleCap(model: model, hovered: capHovered, action: toggleShow))
    .foregroundColor(.white)
    .preferredColorScheme(.dark)
  }

  @ViewBuilder private func cell(_ id: RailCell) -> some View {
    let layout = model.layout
    let vertical = model.edge.isVertical
    let size = CGSize(
      width: CGFloat(vertical ? layout.thickness : layout.cellLength),
      height: CGFloat(vertical ? layout.cellLength : layout.thickness))
    switch id {
    case .provider(let provider):
      MeterCell(
        id: provider, entry: entries[provider], now: now, active: active == id, layout: layout,
        metrics: model.metrics, action: { action(id) }
      )
      .frame(width: size.width, height: size.height)
    case .pullRequests:
      PullRequestCell(
        pulls: pullRequests, active: active == id, layout: layout, metrics: model.metrics,
        action: { action(id) }
      )
      .frame(width: size.width, height: size.height)
    }
  }
}

/// What every rail cell shares: the button, the ring track around a glyph in the icon slot, and
/// the figure beneath. Cells supply their own progress ring and glyph.
private struct RailCellChrome<Ring: View, Glyph: View>: View {
  let layout: RailLayout
  let metrics: NotchMetrics
  let label: String
  /// The percent sign makes a figure read left-heavy; provider cells nudge theirs right.
  let labelOffset: CGFloat
  let description: String
  let active: Bool
  let action: () -> Void
  let ring: Ring
  let glyph: Glyph

  var body: some View {
    Button(action: action) {
      VStack(alignment: .center, spacing: CGFloat(layout.contentSpacing)) {
        ZStack {
          Circle().stroke(Color(white: 0.173), lineWidth: CGFloat(layout.ringStroke))
          ring
          glyph.frame(width: CGFloat(layout.glyphSize), height: CGFloat(layout.glyphSize))
        }
        .padding(CGFloat(layout.ringStroke) / 2)
        .frame(width: CGFloat(layout.iconSlot), height: CGFloat(layout.iconSlot))
        Text(label)
          .font(metrics.font(17, weight: .medium).monospacedDigit())
          .lineLimit(1)
          .frame(height: CGFloat(layout.labelHeight))
          .offset(x: labelOffset)
      }
      .padding(CGFloat(layout.cellPadding))
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .contentShape(Rectangle())
    }
    .buttonStyle(PlainButtonStyle())
    .accessibilityLabel(description)
    .accessibilityValue(active ? "Expanded" : "Collapsed")
    .accessibilityHint(active ? "Collapses the details." : "Shows the details.")
    .help(description)
  }
}

private struct MeterCell: View {
  let id: ProviderId
  let entry: Provider?
  let now: Date
  let active: Bool
  let layout: RailLayout
  let metrics: NotchMetrics
  let action: () -> Void
  private var primary: Quota? { entry?.primary }
  private var fable: Quota? { entry?.fable }
  private var period: String {
    primary.map { $0.kind == "weekly" ? "weekly" : "5 hour" }
      ?? (entry == nil ? "updating" : (entry?.message ?? "unavailable"))
  }
  private var description: String {
    let name = providerName(id)
    guard let primary = primary else { return "\(name), \(period)" }
    let reached = primary.isReached(at: now) ? ", limit reached" : ""
    let fableDescription = fable.map {
      ", Fable weekly, \($0.text(at: now)) used" + ($0.isReached(at: now) ? ", limit reached" : "")
    } ?? ""
    return "\(name), \(period), \(primary.text(at: now)) used\(reached)" + fableDescription
  }

  var body: some View {
    RailCellChrome(
      layout: layout, metrics: metrics, label: primary?.text(at: now) ?? "—",
      labelOffset: primary == nil ? 0 : metrics.value(2), description: description,
      active: active, action: action,
      ring: ZStack {
        Circle().trim(from: 0, to: CGFloat(primary?.percent(at: now) ?? 0) / 100)
          .stroke(
            meterColor(primary, provider: id, at: now),
            style: StrokeStyle(lineWidth: CGFloat(layout.ringStroke), lineCap: .round)
          )
          .rotationEffect(.degrees(-90))
        if let fable = fable {
          Circle().stroke(
            Color(red: 53 / 255, green: 42 / 255, blue: 38 / 255),
            lineWidth: CGFloat(layout.innerRingStroke)
          ).padding(CGFloat(layout.innerRingInset))
          Circle().trim(from: 0, to: CGFloat(fable.percent(at: now) ?? 0) / 100)
            .stroke(
              meterColor(fable, base: fableOrange, at: now),
              style: StrokeStyle(lineWidth: CGFloat(layout.innerRingStroke), lineCap: .round)
            )
            .rotationEffect(.degrees(-90)).padding(CGFloat(layout.innerRingInset))
        }
      },
      glyph: ProviderMark(provider: id).fill(Color.white, style: FillStyle(eoFill: true)))
  }
}

/// Segments on the pull-request ring beyond this stop being legible; the count still says how
/// many there are.
let maxRingSegments = 24

/// The pull-request cell: the ring is split into one arc per listed pull request, coloured by
/// its CI state, with the count beneath.
private struct PullRequestCell: View {
  let pulls: PullRequests?
  let active: Bool
  let layout: RailLayout
  let metrics: NotchMetrics
  let action: () -> Void
  private var readable: Bool { pulls?.readable == true }
  private var segments: [CiState] {
    guard let pulls = pulls, readable else { return [] }
    return Array(pulls.rows.map(\.ci).prefix(maxRingSegments))
  }
  private var description: String {
    guard let pulls = pulls else { return "Pull requests, updating" }
    guard readable else { return "Pull requests, \(pulls.hint ?? "unavailable")" }
    var attention: [String] = []
    if pulls.failing > 0 { attention.append("\(pulls.failing) failing CI") }
    if pulls.changesRequested > 0 { attention.append("\(pulls.changesRequested) with changes requested") }
    if pulls.ready > 0 { attention.append("\(pulls.ready) ready to merge") }
    return (["Pull requests, \(pulls.count) open"] + attention).joined(separator: ", ")
  }

  var body: some View {
    RailCellChrome(
      layout: layout, metrics: metrics, label: readable ? "\(pulls?.count ?? 0)" : "—",
      labelOffset: 0, description: description, active: active, action: action,
      ring: SegmentedRing(colors: segments.map(ciColor), lineWidth: CGFloat(layout.ringStroke)),
      glyph: PullRequestMark().stroke(
        Color.white, style: StrokeStyle(lineWidth: metrics.value(1.6), lineCap: .round)))
  }
}

/// One arc per entry around the ring, equal in length, separated by a small gap, starting at
/// twelve o'clock; a single entry fills the ring.
struct SegmentedRing: View {
  let colors: [Color]
  let lineWidth: CGFloat
  var body: some View {
    let count = colors.count
    let gap: CGFloat = count > 1 ? 0.025 : 0
    let span = 1 / CGFloat(max(count, 1))
    ZStack {
      ForEach(Array(colors.enumerated()), id: \.offset) { index, color in
        let start = CGFloat(index) * span
        Circle()
          .trim(from: start + gap / 2, to: start + span - gap / 2)
          .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .butt))
          .rotationEffect(.degrees(-90))
      }
    }
  }
}

/// GitHub's pull-request glyph: a branch dot joined to a base dot, and a merge dot on the right.
struct PullRequestMark: Shape {
  func path(in rect: CGRect) -> Path {
    var path = Path()
    let unit = min(rect.width, rect.height) / 16
    func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
      CGPoint(x: rect.minX + x * unit, y: rect.minY + y * unit)
    }
    let radius = 2.2 * unit
    path.addEllipse(in: CGRect(origin: point(4 - 2.2, 4 - 2.2), size: CGSize(width: 2 * radius, height: 2 * radius)))
    path.addEllipse(in: CGRect(origin: point(4 - 2.2, 12 - 2.2), size: CGSize(width: 2 * radius, height: 2 * radius)))
    path.addEllipse(in: CGRect(origin: point(12 - 2.2, 12 - 2.2), size: CGSize(width: 2 * radius, height: 2 * radius)))
    path.move(to: point(4, 6.2))
    path.addLine(to: point(4, 9.8))
    path.move(to: point(7, 4))
    path.addLine(to: point(9.5, 4))
    path.addCurve(to: point(12, 6.5), control1: point(11.4, 4), control2: point(12, 4.6))
    path.addLine(to: point(12, 9.8))
    return path
  }
}

/// The rail's first ear as a control: the same colour as the rail until the pointer reaches it,
/// then a lighter cap with a pin in the middle. A click flips between always showing the rail
/// and showing it on hover.
private struct ShowToggleCap: View {
  let model: RailModel
  let hovered: Bool
  let action: () -> Void
  private var pinned: Bool { model.settings.show == .always }
  private var help: String { pinned ? "Show on hover instead" : "Always show the notch" }

  var body: some View {
    let cap = model.capRect
    let metrics = model.metrics
    ZStack(alignment: .topLeading) {
      NotchSilhouette(edge: model.edge, ear: CGFloat(model.layout.ear))
        .fill(Color.white.opacity(hovered ? 0.11 : 0))
        .mask(
          Rectangle().frame(width: cap.width, height: cap.height)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        )
        .animation(.easeInOut(duration: 0.12), value: hovered)
      Image(systemName: pinned ? "pin.fill" : "pin")
        .font(metrics.font(12, weight: .semibold))
        .foregroundColor(Color.white.opacity(hovered ? 0.95 : 0))
        .position(model.capIconCenter)
        .animation(.easeInOut(duration: 0.12), value: hovered)
      Button(action: action) {
        Color.clear.frame(width: cap.width, height: cap.height).contentShape(Rectangle())
      }
      .buttonStyle(PlainButtonStyle())
      .accessibilityLabel(pinned ? "Always showing" : "Showing on hover")
      .accessibilityHint(help)
      .help(help)
    }
  }
}

/// The "show on hover" strip: reaching it opens the rail.
struct NotchPillView: View {
  let vertical: Bool
  var body: some View {
    Capsule().fill(Color.white.opacity(0.32))
      .padding(vertical ? .horizontal : .vertical, 1)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(Color.black.opacity(0.001))
      .accessibilityLabel("Show side notch")
  }
}

/// The notch silhouette: a bar whose inner side is straight and whose two ends flare into the
/// screen edge with an S-curve `ear` points long, so the rail reads as part of the bezel. The
/// edge side is drawn one point past the panel so no outline shows along the screen edge.
struct NotchSilhouette: Shape {
  let edge: NotchCore.Edge
  let ear: CGFloat

  func path(in rect: CGRect) -> Path {
    let thickness = edge.isVertical ? rect.width : rect.height
    let length = edge.isVertical ? rect.height : rect.width
    let ear = min(self.ear, length / 2)
    // Local frame: the screen edge is x == thickness, the rail extends inward to x == 0, and
    // the axis runs along y. The proportions come from the previous notch's hand-tuned curve.
    let edgeX = thickness + 1
    var path = Path()
    path.move(to: CGPoint(x: edgeX, y: 0))
    path.addCurve(
      to: CGPoint(x: thickness * 0.45, y: ear * 0.5),
      control1: CGPoint(x: thickness, y: ear * 0.39),
      control2: CGPoint(x: thickness * 0.76, y: ear * 0.53))
    path.addCurve(
      to: CGPoint(x: 0, y: ear), control1: CGPoint(x: thickness * 0.16, y: ear * 0.46),
      control2: CGPoint(x: 0, y: ear * 0.69))
    path.addLine(to: CGPoint(x: 0, y: length - ear))
    path.addCurve(
      to: CGPoint(x: thickness * 0.45, y: length - ear * 0.5),
      control1: CGPoint(x: 0, y: length - ear * 0.69),
      control2: CGPoint(x: thickness * 0.16, y: length - ear * 0.46))
    path.addCurve(
      to: CGPoint(x: edgeX, y: length), control1: CGPoint(x: thickness * 0.76, y: length - ear * 0.53),
      control2: CGPoint(x: thickness, y: length - ear * 0.39))
    path.closeSubpath()
    let placement: CGAffineTransform
    switch edge {
    case .right: placement = .identity
    case .left: placement = CGAffineTransform(a: -1, b: 0, c: 0, d: 1, tx: thickness, ty: 0)
    case .top: placement = CGAffineTransform(a: 0, b: -1, c: 1, d: 0, tx: 0, ty: thickness)
    case .bottom: placement = CGAffineTransform(a: 0, b: 1, c: 1, d: 0, tx: 0, ty: 0)
    }
    return path.applying(placement.concatenating(CGAffineTransform(translationX: rect.minX, y: rect.minY)))
  }
}
