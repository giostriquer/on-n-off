import NotchCore
import SwiftUI

let claudeOrange = Color(red: 244 / 255, green: 130 / 255, blue: 102 / 255)
let fableOrange = Color(red: 247 / 255, green: 173 / 255, blue: 113 / 255)
let codexGreen = Color(red: 60 / 255, green: 230 / 255, blue: 172 / 255)

struct NotchView: View {
  @ObservedObject var controller: PanelController
  var edge: NotchCore.Edge { controller.message?.snapshot.settings.edge ?? .right }
  var scale: CGFloat { controller.message?.snapshot.settings.size.scale ?? 1 }
  var baseWidth: CGFloat { controller.selection == nil ? 76 : 388 }
  var body: some View {
    content.frame(width: baseWidth, height: 340)
      .scaleEffect(scale, anchor: edge == .left ? .leading : .trailing)
      .frame(
        width: baseWidth * scale, height: 340 * scale,
        alignment: edge == .left ? .leading : .trailing)
  }
  private var content: some View {
    HStack(spacing: 0) {
      if edge == .left { rail }
      if let selection = controller.selection {
        detailPanel(selection).frame(maxWidth: .infinity, maxHeight: .infinity)
          .padding(.vertical, 18).padding(.horizontal, 8)
      }
      if edge == .right { rail }
    }.foregroundColor(.white).preferredColorScheme(.dark)
  }
  private var rail: some View {
    ZStack {
      NotchSilhouette().fill(Color(white: 0.02)).scaleEffect(x: edge == .left ? -1 : 1, y: 1)
      VStack(spacing: 16) {
        ForEach(["claude", "codex"], id: \.self) { id in
          MeterButton(
            id: id, entry: controller.message?.providers.first { $0.provider == id },
            now: controller.now, selected: controller.selection == id
          ) { controller.toggle(id) }
        }
        SettingsTrigger(selected: controller.selection == "settings") {
          controller.toggle("settings")
        }
        .padding(.top, -8)
      }.padding(.top, 11).offset(x: edge == .right ? 2 : -2)
    }.frame(width: 76)
  }
  private func detailPanel(_ selection: String) -> some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text(selection == "settings" ? "Side notch" : selection == "claude" ? "Claude" : "Codex")
          .font(.system(size: 16, weight: .semibold))
        Spacer()
        Button {
          controller.select(nil)
        } label: {
          Image(systemName: "xmark").frame(width: 20, height: 20)
        }
        .buttonStyle(PlainButtonStyle()).help("Collapse side notch")
        .accessibilityLabel("Collapse side notch").keyboardShortcut(.cancelAction)
      }
      if selection == "settings" {
        NativeSettings(controller: controller)
      } else {
        ProviderDetails(
          entry: controller.message?.providers.first { $0.provider == selection },
          now: controller.now, color: selection == "claude" ? claudeOrange : codexGreen)
        Button {
          controller.emit(.openLimits)
          controller.select(nil)
        } label: {
          Label("Open Limits", systemImage: "arrow.up.right").font(.system(size: 12))
        }.buttonStyle(PlainButtonStyle())
      }
      if let error = controller.message?.actionError {
        Text(error).font(.system(size: 10)).foregroundColor(.orange)
      }
    }.padding(16)
      .background(
        RoundedRectangle(cornerRadius: 20).fill(Color(red: 0.035, green: 0.035, blue: 0.043))
      )
      .overlay(RoundedRectangle(cornerRadius: 20).stroke(Color.white.opacity(0.08), lineWidth: 1))
  }
}

private struct MeterButton: View {
  let id: String
  let entry: Provider?
  let now: Date
  let selected: Bool
  let action: () -> Void
  @State private var hovered = false
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
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
      VStack(spacing: 4) {
        ZStack {
          Circle().stroke(Color(white: 0.173), lineWidth: 4)
          Circle().trim(from: 0, to: (primary?.percent(at: now) ?? 0) / 100)
            .stroke(
              id == "claude" ? claudeOrange : codexGreen,
              style: StrokeStyle(lineWidth: 4, lineCap: .round)
            )
            .rotationEffect(.degrees(-90))
          if let fable = fable {
            Circle().stroke(Color(red: 53 / 255, green: 42 / 255, blue: 38 / 255), lineWidth: 3)
              .padding(6)
            Circle().trim(from: 0, to: (fable.percent(at: now) ?? 0) / 100)
              .stroke(fableOrange, style: StrokeStyle(lineWidth: 3, lineCap: .round))
              .rotationEffect(.degrees(-90)).padding(6)
          }
          ProviderMark(provider: id).fill(Color.white).frame(width: 24, height: 24)
        }.padding(2).frame(width: 48, height: 48)
        Text(primary?.text(at: now) ?? "—").font(
          .system(size: 17, weight: .medium).monospacedDigit())
        if primary?.kind != "weekly" {
          Text(period + (primary == nil ? "" : " used")).font(.system(size: 9)).foregroundColor(
            Color(white: 0.67))
        }
        if let fable = fable {
          Text("Fable \(fable.text(at: now))").font(.system(size: 9).monospacedDigit())
            .foregroundColor(fableOrange)
        }
      }.padding(3).frame(width: 62)
        .background(
          RoundedRectangle(cornerRadius: 12).fill(
            Color.white.opacity(hovered || selected ? 0.06 : 0)))
    }.buttonStyle(PlainButtonStyle()).onHover { hovered = $0 }
      .accessibilityLabel(description).accessibilityValue(selected ? "Expanded" : "Collapsed")
      .help(description).animation(reduceMotion ? nil : .easeOut(duration: 0.2))
  }
}

private struct SettingsTrigger: View {
  let selected: Bool
  let action: () -> Void
  @State private var hovered = false
  var body: some View {
    Button(action: action) {
      Group {
        if hovered || selected {
          Image(systemName: "gearshape").font(.system(size: 19))
        } else {
          SettingsArc().stroke(
            Color(white: 0.65), style: StrokeStyle(lineWidth: 2.5, lineCap: .round)
          ).frame(width: 28, height: 15)
        }
      }.frame(width: 44, height: 36)
    }.buttonStyle(PlainButtonStyle()).onHover { hovered = $0 }.help("Side notch settings")
      .accessibilityLabel("Side notch settings")
  }
}
private struct SettingsArc: Shape {
  func path(in rect: CGRect) -> Path {
    var p = Path()
    p.move(to: CGPoint(x: 3, y: 3))
    p.addQuadCurve(
      to: CGPoint(x: rect.maxX - 3, y: 3), control: CGPoint(x: rect.midX, y: rect.maxY + 3))
    return p
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
