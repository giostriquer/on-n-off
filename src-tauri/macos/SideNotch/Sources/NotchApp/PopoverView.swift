import AppKit
import NotchCore
import SwiftUI

enum PopoverContent {
  case provider(ProviderCell)
  case pullRequests(PullRequestCell)
}

struct PopoverModel {
  let content: PopoverContent
  let now: Date
  let edge: NotchCore.Edge
  let metrics: NotchMetrics
  let width: CGFloat
  let maxHeight: CGFloat
  let tailLength: CGFloat
  let tail: CGFloat
  let style: NotchStyle
  let actionError: String?
  let openLimits: () -> Void
  let openPullRequests: () -> Void

  var identity: String {
    switch content {
    case .provider(let cell): return cell.provider.rawValue
    case .pullRequests: return "pull-requests"
    }
  }
}

struct NotchPopoverView: View {
  let model: PopoverModel?
  var body: some View {
    ZStack(alignment: .topLeading) {
      if let model = model {
        PopoverCard(model: model).id(model.identity).transition(.opacity)
      } else {
        Color.clear.frame(width: 1, height: 1)
      }
    }
  }
}

private struct PopoverCard: View {
  let model: PopoverModel
  private var metrics: NotchMetrics { model.metrics }

  var body: some View {
    VStack(alignment: .leading, spacing: metrics.value(10)) {
      switch model.content {
      case .provider(let cell):
        ProviderSection(cell: cell, now: model.now, metrics: metrics)
      case .pullRequests(let cell):
        PullRequestSection(cell: cell, now: model.now, metrics: metrics)
      }
      if let error = model.actionError {
        Text(error).font(metrics.font(10)).foregroundColor(warnAmber)
      }
      HStack {
        Spacer()
        switch model.content {
        case .provider:
          FooterLink(title: "Open Limits", metrics: metrics, action: model.openLimits)
        case .pullRequests:
          FooterLink(title: "Open Pull requests", metrics: metrics, action: model.openPullRequests)
        }
      }
    }
    .padding(metrics.value(12))
    .frame(width: model.width, alignment: .topLeading)
    .modifier(PopoverSurface(model: model))
    .foregroundColor(.white)
    .preferredColorScheme(.dark)
  }
}

private struct PopoverSurface: ViewModifier {
  let model: PopoverModel
  private var metrics: NotchMetrics { model.metrics }
  private var cornerRadius: CGFloat { metrics.value(12) }

  func body(content: Content) -> some View {
    content.padding(tailPadding).notchSurface(
      popoverInk,
      in: PopoverOutline(
        edge: model.edge, tailLength: model.tailLength, tail: model.tail,
        tailHalfWidth: metrics.value(7), cornerRadius: cornerRadius),
      style: model.style,
      solidBorder: RoundedRectangle(cornerRadius: cornerRadius)
        .strokeBorder(Color.white.opacity(0.11), lineWidth: metrics.value(1))
        .padding(tailPadding))
  }

  private var tailPadding: EdgeInsets {
    var insets = EdgeInsets()
    switch model.edge {
    case .right: insets.trailing = model.tailLength
    case .left: insets.leading = model.tailLength
    case .top: insets.top = model.tailLength
    case .bottom: insets.bottom = model.tailLength
    }
    return insets
  }
}

private struct PopoverOutline: Shape {
  let edge: NotchCore.Edge
  let tailLength: CGFloat
  let tail: CGFloat
  let tailHalfWidth: CGFloat
  let cornerRadius: CGFloat

  func path(in rect: CGRect) -> Path {
    var card = rect
    switch edge {
    case .right: card.size.width -= tailLength
    case .left: card.origin.x += tailLength; card.size.width -= tailLength
    case .top: card.origin.y += tailLength; card.size.height -= tailLength
    case .bottom: card.size.height -= tailLength
    }
    let body = Path(roundedRect: card, cornerRadius: cornerRadius)
    if #available(macOS 14.0, *) {
      return body.union(tailPath(card: card, in: rect, foot: 1))
    }
    var path = body
    path.addPath(tailPath(card: card, in: rect, foot: 0))
    return path
  }

  private func tailPath(card: CGRect, in rect: CGRect, foot: CGFloat) -> Path {
    let along = edge.isVertical ? card.minY + tail : card.minX + tail
    let half = tailHalfWidth
    let points: [CGPoint]
    switch edge {
    case .right:
      points = [
        CGPoint(x: card.maxX - foot, y: along - half), CGPoint(x: card.maxX, y: along - half),
        CGPoint(x: rect.maxX, y: along), CGPoint(x: card.maxX, y: along + half),
        CGPoint(x: card.maxX - foot, y: along + half),
      ]
    case .left:
      points = [
        CGPoint(x: card.minX + foot, y: along - half), CGPoint(x: card.minX, y: along - half),
        CGPoint(x: rect.minX, y: along), CGPoint(x: card.minX, y: along + half),
        CGPoint(x: card.minX + foot, y: along + half),
      ]
    case .top:
      points = [
        CGPoint(x: along - half, y: card.minY + foot), CGPoint(x: along - half, y: card.minY),
        CGPoint(x: along, y: rect.minY), CGPoint(x: along + half, y: card.minY),
        CGPoint(x: along + half, y: card.minY + foot),
      ]
    case .bottom:
      points = [
        CGPoint(x: along - half, y: card.maxY - foot), CGPoint(x: along - half, y: card.maxY),
        CGPoint(x: along, y: rect.maxY), CGPoint(x: along + half, y: card.maxY),
        CGPoint(x: along + half, y: card.maxY - foot),
      ]
    }
    var path = Path()
    path.addLines(points)
    path.closeSubpath()
    return path
  }
}

private struct FooterLink: View {
  let title: String
  let metrics: NotchMetrics
  let action: () -> Void
  var body: some View {
    Button(action: action) {
      HStack(spacing: metrics.value(3)) {
        Text(title)
        Image(systemName: "arrow.up.right")
      }
      .font(metrics.font(10.5, weight: .medium)).foregroundColor(mutedInk)
    }
    .buttonStyle(PlainButtonStyle()).accessibilityLabel("\(title) in on-n-off")
  }
}

private struct SectionHeader: View {
  let title: String
  let metrics: NotchMetrics
  let mark: AnyView
  var body: some View {
    HStack(spacing: metrics.value(7)) {
      mark.frame(width: metrics.value(13), height: metrics.value(13))
      Text(title).font(metrics.font(13, weight: .semibold))
      Spacer(minLength: 0)
    }
  }
}

private struct ProviderSection: View {
  let cell: ProviderCell
  let now: Date
  let metrics: NotchMetrics
  private var id: ProviderId { cell.provider }
  private var entry: Reading? { cell.reading }
  private var windows: [Quota] { entry?.windows ?? [] }

  var body: some View {
    SectionHeader(
      title: "\(providerName(id)) Usage", metrics: metrics,
      mark: AnyView(ProviderMark(provider: id).fill(Color.white, style: FillStyle(eoFill: true))))
    if let notice = cell.notice {
      Text(notice.text).font(metrics.font(11))
        .foregroundColor(toneColor(notice.tone)).fixedSize(horizontal: false, vertical: true)
    }
    if let paused = cell.paused {
      Text(paused.text).font(metrics.font(10)).foregroundColor(toneColor(paused.tone))
    }
    ForEach(windows) { quota in
      QuotaBlock(quota: quota, provider: id, now: now, metrics: metrics)
    }
    if let credits = entry?.workspaceCredits {
      QuotaBlock(
        quota: credits.quota, provider: id, now: now, metrics: metrics,
        note: credits.note(at: now), detail: credits.amounts(at: now))
    }
    if !cell.sessions.isEmpty {
      Divider().background(Color.white.opacity(0.1))
      VStack(alignment: .leading, spacing: metrics.value(8)) {
        ForEach(cell.sessions) { session in
          SessionRow(session: session, accent: providerColor(id), now: now, metrics: metrics)
        }
      }
    }
  }
}

private struct QuotaBlock: View {
  let quota: Quota
  let provider: ProviderId
  let now: Date
  let metrics: NotchMetrics
  var note: String? = nil
  var detail: String? = nil
  private var figure: String {
    guard quota.percent(at: now) != nil else { return "—" }
    return "\(quota.text(at: now)) Used" + (detail.map { " · \($0)" } ?? "")
  }
  var body: some View {
    VStack(alignment: .leading, spacing: metrics.value(5)) {
      HStack(alignment: .firstTextBaseline) {
        Text(quota.label).font(metrics.font(11, weight: .semibold)).lineLimit(1)
        Spacer(minLength: metrics.value(8))
        Text(note ?? quota.note(at: now)).font(metrics.font(10)).foregroundColor(mutedInk)
          .lineLimit(1)
      }
      GeometryReader { geometry in
        ZStack(alignment: .leading) {
          Capsule().fill(Color.white.opacity(0.14))
          Capsule().fill(meterColor(quota, provider: provider, at: now)).frame(
            width: geometry.size.width * CGFloat(quota.percent(at: now) ?? 0) / 100)
        }
      }
      .frame(height: metrics.value(4))
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(quota.label)
      .accessibilityValue(
        quota.percent(at: now) == nil
          ? "Usage unavailable"
          : "\(quota.text(at: now)) used" + (quota.isReached(at: now) ? ", limit reached" : "")
            + (detail.map { ", \($0)" } ?? ""))
      Text(figure)
        .font(metrics.font(10.5, weight: .medium).monospacedDigit())
    }
  }
}

private struct SessionRow: View {
  let session: Session
  let accent: Color
  let now: Date
  let metrics: NotchMetrics
  var body: some View {
    VStack(alignment: .leading, spacing: metrics.value(2)) {
      HStack(alignment: .firstTextBaseline) {
        Text(session.name).font(metrics.font(11, weight: .semibold)).lineLimit(1)
        Spacer(minLength: metrics.value(8))
        HStack(spacing: metrics.value(3)) {
          Image(systemName: session.isWorking ? "arrow.triangle.2.circlepath" : "circle")
            .font(metrics.font(8, weight: .semibold))
          Text(session.isWorking ? "working" : "idle")
        }
        .font(metrics.font(10.5, weight: .medium))
        .foregroundColor(session.isWorking ? accent : mutedInk)
      }
      HStack {
        Text("\(session.place) · \(session.project)").font(metrics.font(10)).lineLimit(1)
        Spacer(minLength: metrics.value(8))
        Text(session.age(at: now)).font(metrics.font(10).monospacedDigit())
      }
      .foregroundColor(mutedInk)
    }
    .accessibilityElement(children: .combine)
  }
}

private struct PullRequestSection: View {
  let cell: PullRequestCell
  let now: Date
  let metrics: NotchMetrics

  var body: some View {
    SectionHeader(
      title: "Pull requests", metrics: metrics,
      mark: AnyView(
        PullRequestMark().stroke(
          Color.white, style: StrokeStyle(lineWidth: metrics.value(1.4), lineCap: .round))))
    if let notice = cell.notice {
      Text(notice.text).font(metrics.font(11)).foregroundColor(toneColor(notice.tone))
        .fixedSize(horizontal: false, vertical: true)
    }
    ForEach(cell.reading?.lists ?? [], id: \.id) { list in
      VStack(alignment: .leading, spacing: metrics.value(6)) {
        HStack {
          Text(list.title.uppercased())
            .font(metrics.font(9.5, weight: .semibold)).tracking(0.4)
          Spacer(minLength: metrics.value(8))
          Text(list.total > UInt64(list.items.count) ? "\(list.items.count) of \(list.total)" : "\(list.items.count)")
            .font(metrics.font(9.5).monospacedDigit())
        }
        .foregroundColor(mutedInk)
        if list.items.isEmpty {
          Text("Nothing open.").font(metrics.font(10.5)).foregroundColor(mutedInk)
        }
        ForEach(list.items) { pull in
          PullRequestRow(pull: pull, metrics: metrics)
        }
      }
    }
  }
}

private struct PullRequestRow: View {
  let pull: PullRequest
  let metrics: NotchMetrics
  @State private var copied = false

  private var context: (name: String, ink: Color)? {
    pull.tag.map { ($0.label.lowercased(), toneColor($0.tone)) }
  }

  private var ciLabel: String {
    switch pull.ci {
    case .success: return "CI passing"
    case .failure: return "CI failing"
    case .error: return "CI errored"
    case .pending: return "CI pending"
    case .none, .unknown: return "No checks"
    }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: metrics.value(2)) {
      HStack(alignment: .top, spacing: metrics.value(8)) {
        Button(action: { Actions.open(pull) }) {
          Text(pull.title).font(metrics.font(11, weight: .semibold)).lineLimit(2)
            .multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityLabel(
          [pull.title, context?.name, "\(pull.repo) number \(pull.number)"].compactMap { $0 }
            .joined(separator: ", "))
        .accessibilityHint("Opens the pull request on GitHub.")
        Button(action: copy) {
          Image(systemName: copied ? "checkmark" : "doc.on.doc")
            .font(metrics.font(11, weight: .medium))
            .foregroundColor(copied ? liveGreen : context?.ink ?? mutedInk)
            .frame(width: metrics.value(20), height: metrics.value(16))
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityLabel(
          copied ? "Copied" : context.map { "Copy a \($0.name) review request" } ?? "Copy a review request")
        .accessibilityHint("Copies “review please” with the pull request linked, ready for Slack.")
        .help(
          context.map { "Copy “review please: <title>” for a \($0.name) channel" }
            ?? "Copy “review please: <title>” with the title linked")
      }
      HStack(alignment: .center, spacing: metrics.value(8)) {
        Button(action: { Actions.open(pull) }) {
          HStack(spacing: metrics.value(5)) {
            Text("\(pull.repo) #\(pull.number)").font(metrics.font(10).monospacedDigit())
              .foregroundColor(mutedInk).lineLimit(1)
            ForEach(Array(pull.badges.enumerated()), id: \.offset) { _, badge in
              Text(badge.label).font(metrics.font(9.5, weight: .medium))
                .foregroundColor(toneColor(badge.tone)).lineLimit(1)
            }
          }
          .frame(maxWidth: .infinity, alignment: .leading)
          .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityHidden(true)
        Group {
          if pull.ci == .none || pull.ci == .unknown {
            Circle().strokeBorder(Color(white: 0.35), lineWidth: metrics.value(1))
          } else {
            Circle().fill(ciColor(pull.ci))
          }
        }
        .frame(width: metrics.value(7), height: metrics.value(7))
        .frame(width: metrics.value(20))
        .accessibilityLabel(ciLabel)
        .help(ciLabel)
      }
    }
    .overlay(
      Group {
        if let context = context {
          Capsule().fill(context.ink).frame(width: metrics.value(2))
            .offset(x: -metrics.value(8))
            .accessibilityHidden(true)
        }
      },
      alignment: .leading)
  }

  private func copy() {
    Actions.copyReviewRequest(pull)
    withAnimation(.easeInOut(duration: 0.15)) { copied = true }
    DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) {
      withAnimation(.easeInOut(duration: 0.15)) { copied = false }
    }
  }
}

let liveGreen = Color(red: 74 / 255, green: 200 / 255, blue: 120 / 255)
let workBlue = Color(red: 96 / 255, green: 165 / 255, blue: 250 / 255)
let personalLavender = Color(red: 170 / 255, green: 155 / 255, blue: 215 / 255)

func toneColor(_ tone: Tone) -> Color {
  switch tone {
  case .muted: return mutedInk
  case .green: return liveGreen
  case .red: return tripRed
  case .amber: return warnAmber
  case .work: return workBlue
  case .personal: return personalLavender
  }
}

enum Actions {
  static func open(_ pull: PullRequest) {
    guard let url = pull.link else { return }
    NSWorkspace.shared.open(url)
  }

  static func copyReviewRequest(_ pull: PullRequest) {
    guard let url = pull.link else { return }
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    pasteboard.setString(reviewRequestHtml(title: pull.title, url: url), forType: .html)
    pasteboard.setString(reviewRequestText(title: pull.title, url: url), forType: .string)
  }
}
