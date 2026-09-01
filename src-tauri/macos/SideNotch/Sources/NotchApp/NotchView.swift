import NotchCore
import SwiftUI

let claudeOrange = Color(red: 244 / 255, green: 130 / 255, blue: 102 / 255)
let fableOrange = Color(red: 247 / 255, green: 173 / 255, blue: 113 / 255)
let codexGreen = Color(red: 60 / 255, green: 230 / 255, blue: 172 / 255)

struct NotchRailView: View {
  @ObservedObject var controller: PanelController
  private var edge: NotchCore.Edge { controller.message?.snapshot.settings.edge ?? .right }
  private var scale: CGFloat { controller.message?.snapshot.settings.size.scale ?? 1 }
  private var metrics: NotchMetrics { NotchMetrics(scale: scale) }

  var body: some View {
    ZStack {
      NotchSilhouette().fill(Color(white: 0.02)).scaleEffect(x: edge == .left ? -1 : 1, y: 1)
      VStack(spacing: metrics.value(16)) {
        ForEach(["claude", "codex"], id: \.self) { id in
          MeterButton(
            id: id, entry: controller.message?.providers.first { $0.provider == id },
            now: controller.now, selected: controller.selection == id, metrics: metrics
          ) { controller.toggle(id) }
        }
      }
      .offset(x: edge == .right ? metrics.value(2) : metrics.value(-2))
    }
    .frame(width: metrics.value(76), height: metrics.value(340))
    .foregroundColor(.white)
    .preferredColorScheme(.dark)
  }
}

struct NotchDetailView: View {
  @ObservedObject var controller: PanelController
  private var scale: CGFloat { controller.message?.snapshot.settings.size.scale ?? 1 }
  private var metrics: NotchMetrics { NotchMetrics(scale: scale) }

  var body: some View {
    Group {
      if let selection = controller.selection {
        detailPanel(selection)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .padding(.vertical, metrics.value(18))
          .padding(.horizontal, metrics.value(8))
      } else {
        Color.clear
      }
    }
    .frame(width: metrics.value(312), height: metrics.value(340))
    .foregroundColor(.white)
    .preferredColorScheme(.dark)
  }

  private func detailPanel(_ selection: String) -> some View {
    VStack(alignment: .leading, spacing: metrics.value(12)) {
      HStack {
        Text(selection == "claude" ? "Claude" : "Codex")
          .font(metrics.font(16, weight: .semibold))
        Spacer()
        Button {
          controller.select(nil)
        } label: {
          Image(systemName: "xmark").font(metrics.font(11, weight: .medium))
            .frame(width: metrics.value(20), height: metrics.value(20))
        }
        .buttonStyle(PlainButtonStyle()).help("Collapse side notch")
        .accessibilityLabel("Collapse side notch").keyboardShortcut(.cancelAction)
      }
      ProviderDetails(
        entry: controller.message?.providers.first { $0.provider == selection },
        now: controller.now, color: selection == "claude" ? claudeOrange : codexGreen,
        metrics: metrics)
      Button {
        controller.emit(.openLimits)
        controller.select(nil)
      } label: {
        Label("Open Limits", systemImage: "arrow.up.right").font(metrics.font(12))
      }.buttonStyle(PlainButtonStyle())
      if let error = controller.message?.actionError {
        Text(error).font(metrics.font(10)).foregroundColor(.orange)
      }
    }
    .padding(metrics.value(16))
    .background(
      RoundedRectangle(cornerRadius: metrics.value(20)).fill(
        Color(red: 0.035, green: 0.035, blue: 0.043))
    )
    .overlay(
      RoundedRectangle(cornerRadius: metrics.value(20)).stroke(
        Color.white.opacity(0.08), lineWidth: metrics.value(1)))
  }
}

private struct MeterButton: View {
  let id: String
  let entry: Provider?
  let now: Date
  let selected: Bool
  let metrics: NotchMetrics
  let action: () -> Void
  @State private var hovered = false
  var primary: Quota? { entry?.primary }
  var fable: Quota? { entry?.fable }
  var period: String {
    primary.map { $0.kind == "weekly" ? "weekly" : "5h" }
      ?? (entry == nil ? "updating" : "unavailable")
  }
  var description: String {
    "\(id == "claude" ? "Claude" : "Codex"), \(period), \(primary?.text(at: now) ?? "unavailable") used"
      + (fable.map { ", Fable weekly, \($0.text(at: now)) used" } ?? "")
  }
  var body: some View {
    Button(action: action) {
      VStack(spacing: metrics.value(4)) {
        ZStack {
          Circle().stroke(Color(white: 0.173), lineWidth: metrics.value(4))
          Circle().trim(from: 0, to: (primary?.percent(at: now) ?? 0) / 100)
            .stroke(
              id == "claude" ? claudeOrange : codexGreen,
              style: StrokeStyle(lineWidth: metrics.value(4), lineCap: .round)
            )
            .rotationEffect(.degrees(-90))
          if let fable = fable {
            Circle().stroke(
              Color(red: 53 / 255, green: 42 / 255, blue: 38 / 255),
              lineWidth: metrics.value(3)
            ).padding(metrics.value(6))
            Circle().trim(from: 0, to: (fable.percent(at: now) ?? 0) / 100)
              .stroke(
                fableOrange,
                style: StrokeStyle(lineWidth: metrics.value(3), lineCap: .round)
              )
              .rotationEffect(.degrees(-90)).padding(metrics.value(6))
          }
          ProviderMark(provider: id).fill(Color.white).frame(
            width: metrics.value(24), height: metrics.value(24))
        }.padding(metrics.value(2)).frame(
          width: metrics.value(48), height: metrics.value(48))
        Text(primary?.text(at: now) ?? "—").font(
          metrics.font(17, weight: .medium).monospacedDigit())
        if primary?.kind != "weekly" {
          Text(period + (primary == nil ? "" : " used")).font(metrics.font(9)).foregroundColor(
            Color(white: 0.67))
        }
        if let fable = fable {
          Text("Fable \(fable.text(at: now))").font(metrics.font(9).monospacedDigit())
            .foregroundColor(fableOrange)
        }
      }.padding(metrics.value(3)).frame(width: metrics.value(62))
        .background(
          RoundedRectangle(cornerRadius: metrics.value(12)).fill(
            Color.white.opacity(hovered || selected ? 0.06 : 0)))
    }.buttonStyle(PlainButtonStyle()).onHover { hovered = $0 }
      .accessibilityLabel(description).accessibilityValue(selected ? "Expanded" : "Collapsed")
      .help(description)
  }
}

private struct NotchSilhouette: Shape {
  func path(in rect: CGRect) -> Path {
    var p = Path()
    p.move(to: CGPoint(x: 76, y: 0))
    p.addCurve(
      to: CGPoint(x: 34, y: 36), control1: CGPoint(x: 76, y: 28), control2: CGPoint(x: 58, y: 38))
    p.addCurve(
      to: CGPoint(x: 0, y: 72), control1: CGPoint(x: 12, y: 33), control2: CGPoint(x: 0, y: 50))
    p.addLine(to: CGPoint(x: 0, y: 268))
    p.addCurve(
      to: CGPoint(x: 34, y: 304), control1: CGPoint(x: 0, y: 290), control2: CGPoint(x: 12, y: 307))
    p.addCurve(
      to: CGPoint(x: 76, y: 340), control1: CGPoint(x: 58, y: 302), control2: CGPoint(x: 76, y: 312)
    )
    p.closeSubpath()
    return p.applying(CGAffineTransform(scaleX: rect.width / 76, y: rect.height / 340))
  }
}
