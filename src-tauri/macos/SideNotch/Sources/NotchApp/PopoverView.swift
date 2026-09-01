import AppKit
import NotchCore
import SwiftUI

/// What one popover shows.
enum PopoverContent {
  case provider(ProviderId, Provider?)
  case pullRequests(PullRequests?)
}

/// Everything the popover renders, captured by value so the panel can be measured synchronously.
struct PopoverModel {
  let content: PopoverContent
  let now: Date
  let edge: NotchCore.Edge
  let metrics: NotchMetrics
  let width: CGFloat
  /// Height cap; the body scrolls once the content is taller.
  let maxHeight: CGFloat
  let tailLength: CGFloat
  /// Centre of the tail along the rail's axis, in the card's coordinates.
  let tail: CGFloat
  let actionError: String?
  let openLimits: () -> Void
  let openPullRequests: () -> Void

  var identity: String {
    switch content {
    case .provider(let id, _): return id.rawValue
    case .pullRequests: return "pull-requests"
    }
  }
}

struct NotchPopoverView: View {
  let model: PopoverModel?
  var body: some View {
    // Keyed by cell so a hover that moves to another cell crossfades the cards.
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
      case .provider(let id, let entry):
        ProviderSection(id: id, entry: entry, now: model.now, metrics: metrics)
      case .pullRequests(let pulls):
        PullRequestSection(pulls: pulls, now: model.now, metrics: metrics)
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
    .background(
      RoundedRectangle(cornerRadius: metrics.value(12)).fill(popoverInk))
    .overlay(
      RoundedRectangle(cornerRadius: metrics.value(12)).strokeBorder(
        Color.white.opacity(0.11), lineWidth: metrics.value(1)))
    .overlay(tail)
    .padding(tailPadding)
    .foregroundColor(.white)
    .preferredColorScheme(.dark)
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

  private var tail: some View {
    GeometryReader { geometry in
      let size = geometry.size
      let length = model.tailLength
      let half = metrics.value(7)
      TailShape(edge: model.edge).fill(popoverInk)
        .frame(
          width: model.edge.isVertical ? length : half * 2,
          height: model.edge.isVertical ? half * 2 : length
        )
        .position(tailPosition(in: size, length: length))
    }
  }

  private func tailPosition(in size: CGSize, length: CGFloat) -> CGPoint {
    switch model.edge {
    case .right: return CGPoint(x: size.width + length / 2, y: model.tail)
    case .left: return CGPoint(x: -length / 2, y: model.tail)
    case .top: return CGPoint(x: model.tail, y: -length / 2)
    case .bottom: return CGPoint(x: model.tail, y: size.height + length / 2)
    }
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

// MARK: - Provider usage

private struct ProviderSection: View {
  let id: ProviderId
  let entry: Provider?
  let now: Date
  let metrics: NotchMetrics
  private var windows: [Quota] { entry?.orderedWindows ?? [] }
  private var readable: Bool { entry?.status == "ok" && entry?.currentAccount == true }

  var body: some View {
    SectionHeader(
      title: "\(providerName(id)) Usage", metrics: metrics,
      mark: AnyView(ProviderMark(provider: id).fill(Color.white, style: FillStyle(eoFill: true))))
    if let entry = entry, !readable {
      Text(entry.message ?? "Usage unavailable.").font(metrics.font(11))
        .foregroundColor(mutedInk).fixedSize(horizontal: false, vertical: true)
      if !windows.isEmpty {
        Text("Refresh paused. Last observed values below.").font(metrics.font(10))
          .foregroundColor(warnAmber)
      }
    } else if entry == nil {
      Text("Checking usage…").font(metrics.font(11)).foregroundColor(mutedInk)
    }
    ForEach(windows) { quota in
      QuotaBlock(quota: quota, provider: id, now: now, metrics: metrics)
    }
    if let sessions = entry?.sessions, !sessions.isEmpty {
      Divider().background(Color.white.opacity(0.1))
      VStack(alignment: .leading, spacing: metrics.value(8)) {
        ForEach(sessions) { session in
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
  var body: some View {
    VStack(alignment: .leading, spacing: metrics.value(5)) {
      HStack(alignment: .firstTextBaseline) {
        Text(quota.label).font(metrics.font(11, weight: .semibold)).lineLimit(1)
        Spacer(minLength: metrics.value(8))
        Text(quota.note(at: now)).font(metrics.font(10)).foregroundColor(mutedInk).lineLimit(1)
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
          ? "Not observed since the reset"
          : "\(quota.text(at: now)) used" + (quota.isReached(at: now) ? ", limit reached" : ""))
      Text(quota.percent(at: now) == nil ? "—" : "\(quota.text(at: now)) Used")
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

// MARK: - Pull requests

private struct PullRequestSection: View {
  let pulls: PullRequests?
  let now: Date
  let metrics: NotchMetrics

  var body: some View {
    SectionHeader(
      title: "Pull requests", metrics: metrics,
      mark: AnyView(
        PullRequestMark().stroke(
          Color.white, style: StrokeStyle(lineWidth: metrics.value(1.4), lineCap: .round))))
    if let pulls = pulls {
      if let hint = pulls.hint, pulls.status != "ok" {
        Text(hint).font(metrics.font(11)).foregroundColor(pulls.readable ? warnAmber : mutedInk)
          .fixedSize(horizontal: false, vertical: true)
      }
      if pulls.readable {
        ForEach(pulls.lists, id: \.id) { list in
          VStack(alignment: .leading, spacing: metrics.value(6)) {
            HStack {
              Text(list.id.title.uppercased())
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
    } else {
      Text("Checking pull requests…").font(metrics.font(11)).foregroundColor(mutedInk)
    }
  }
}

private struct PullRequestRow: View {
  let pull: PullRequest
  let metrics: NotchMetrics
  @State private var copied = false

  private var ciLabel: String {
    switch pull.ci {
    case "success": return "CI passing"
    case "failure": return "CI failing"
    case "error": return "CI errored"
    case "pending": return "CI pending"
    default: return "No checks"
    }
  }

  private var badges: [(String, Color)] {
    var badges: [(String, Color)] = []
    if pull.isDraft { badges.append(("Draft", mutedInk)) }
    switch pull.reviewDecision {
    case "APPROVED": badges.append(("Approved", liveGreen))
    case "CHANGES_REQUESTED": badges.append(("Changes requested", tripRed))
    default: break
    }
    switch pull.mergeKind {
    case "conflicts": badges.append(("Conflicts", tripRed))
    case "queued": badges.append(("Queued", liveGreen))
    case "autoMerge": badges.append(("Auto-merge", mutedInk))
    case "ready": badges.append(("Ready to merge", liveGreen))
    case "behind": badges.append(("Behind base", warnAmber))
    case "blocked": badges.append(("Blocked", warnAmber))
    default: break
    }
    return badges
  }

  var body: some View {
    // Two lines, each with its own trailing control, so the copy button sits with the title and
    // the CI dot with the repository line however many lines the title takes.
    VStack(alignment: .leading, spacing: metrics.value(2)) {
      HStack(alignment: .top, spacing: metrics.value(8)) {
        Button(action: { Actions.open(pull) }) {
          Text(pull.title).font(metrics.font(11, weight: .semibold)).lineLimit(2)
            .multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityLabel("\(pull.title), \(pull.repo) number \(pull.number)")
        .accessibilityHint("Opens the pull request on GitHub.")
        Button(action: copy) {
          Image(systemName: copied ? "checkmark" : "doc.on.doc")
            .font(metrics.font(11, weight: .medium))
            .foregroundColor(copied ? liveGreen : mutedInk)
            .frame(width: metrics.value(20), height: metrics.value(16))
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityLabel(copied ? "Copied" : "Copy a review request")
        .accessibilityHint("Copies “review please” with the pull request linked, ready for Slack.")
        .help("Copy “review please: <title>” with the title linked")
      }
      HStack(alignment: .center, spacing: metrics.value(8)) {
        Button(action: { Actions.open(pull) }) {
          HStack(spacing: metrics.value(5)) {
            Text("\(pull.repo) #\(pull.number)").font(metrics.font(10).monospacedDigit())
              .foregroundColor(mutedInk).lineLimit(1)
            ForEach(Array(badges.enumerated()), id: \.offset) { _, badge in
              Text(badge.0).font(metrics.font(9.5, weight: .medium)).foregroundColor(badge.1)
                .lineLimit(1)
            }
          }
          .frame(maxWidth: .infinity, alignment: .leading)
          .contentShape(Rectangle())
        }
        .buttonStyle(PlainButtonStyle())
        .accessibilityHidden(true)
        // The CI rollup as a dot: the same colours as the ring, hollow when nothing reported.
        Group {
          if pull.ci == "none" {
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

/// The two things a pull-request row can do on this machine; neither talks to GitHub.
enum Actions {
  /// Opens the pull request in the default browser. Only `https://github.com` links qualify.
  static func open(_ pull: PullRequest) {
    guard let url = pull.link else { return }
    NSWorkspace.shared.open(url)
  }

  /// Puts “review please: <title>” on the pasteboard with the title linked, as rich text for
  /// chat apps that keep links (Slack, Notes) and as plain text for everything else.
  static func copyReviewRequest(_ pull: PullRequest) {
    guard let url = pull.link else { return }
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    pasteboard.setString(reviewRequestHtml(title: pull.title, url: url), forType: .html)
    pasteboard.setString(reviewRequestText(title: pull.title, url: url), forType: .string)
  }
}

/// A triangle pointing from the card toward the rail.
private struct TailShape: Shape {
  let edge: NotchCore.Edge
  func path(in rect: CGRect) -> Path {
    var path = Path()
    switch edge {
    case .right:
      path.move(to: CGPoint(x: rect.minX, y: rect.minY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
      path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
    case .left:
      path.move(to: CGPoint(x: rect.maxX, y: rect.minY))
      path.addLine(to: CGPoint(x: rect.minX, y: rect.midY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
    case .top:
      path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
      path.addLine(to: CGPoint(x: rect.midX, y: rect.minY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
    case .bottom:
      path.move(to: CGPoint(x: rect.minX, y: rect.minY))
      path.addLine(to: CGPoint(x: rect.midX, y: rect.maxY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
    }
    path.closeSubpath()
    return path
  }
}
