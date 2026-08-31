import NotchCore
import SwiftUI

struct ProviderDetails: View {
  let entry: Provider?
  let now: Date
  let color: Color
  let metrics: NotchMetrics
  private var windows: [Quota] {
    let priority = ["session": 0, "weekly": 1, "model": 2]
    return (entry?.visibleWindows ?? []).sorted {
      priority[$0.kind, default: 3] < priority[$1.kind, default: 3]
    }
  }
  var body: some View {
    Text(
      "Current account"
        + (entry?.plan.map { " · \($0.replacingOccurrences(of: "_", with: " ").capitalized)" } ?? "")
    )
    .font(metrics.font(11)).foregroundColor(.gray)
    ScrollView {
      VStack(alignment: .leading, spacing: metrics.value(12)) {
        if entry?.status != "ok", let entry = entry {
          Text(entry.message ?? "Usage unavailable.").font(metrics.font(11)).foregroundColor(.gray)
          if !windows.isEmpty {
            Text("Refresh paused. Last observed values below.").font(metrics.font(10))
              .foregroundColor(.orange)
          }
        }
        ForEach(windows) { quota in
          VStack(alignment: .leading, spacing: metrics.value(7)) {
            Divider()
            Text(quota.label).font(metrics.font(11)).foregroundColor(.gray)
            Text(quota.text(at: now) + (quota.percent(at: now) == nil ? "" : " used"))
              .font(metrics.font(19, weight: .medium).monospacedDigit())
            GeometryReader { geometry in
              ZStack(alignment: .leading) {
                Capsule().fill(Color(white: 0.18))
                Capsule().fill(color).frame(
                  width: geometry.size.width * (quota.percent(at: now) ?? 0) / 100)
              }
            }.frame(height: metrics.value(5)).accessibilityElement(children: .ignore)
              .accessibilityLabel(quota.label)
              .accessibilityValue(
                quota.percent(at: now) == nil
                  ? "Not observed since the reset" : "\(quota.text(at: now)) used")
            Text(quota.note(at: now)).font(metrics.font(10)).foregroundColor(.gray)
          }
        }
        if windows.isEmpty {
          Text(entry == nil ? "Checking usage…" : "No usage windows available.").font(
            metrics.font(11)
          ).foregroundColor(.gray)
        }
      }.frame(maxWidth: .infinity, alignment: .leading)
    }.frame(maxHeight: .infinity)
  }
}

struct NativeSettings: View {
  @ObservedObject var controller: PanelController
  let metrics: NotchMetrics
  var settings: NotchCore.Settings { controller.message?.snapshot.settings ?? NotchCore.Settings() }
  func change(_ update: (inout NotchCore.Settings) -> Void) {
    var next = settings
    update(&next)
    controller.save(next)
  }
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: metrics.value(12)) {
        Toggle(
          "Enable side notch",
          isOn: Binding(get: { settings.enabled }, set: { value in change { $0.enabled = value } })
        )
        .toggleStyle(SwitchToggleStyle()).font(metrics.font(12))
        Picker(
          "Display",
          selection: Binding(
            get: { settings.displayId ?? "" }, set: { value in change { $0.displayId = value } })
        ) {
          if !controller.displays.contains(where: { $0.id == settings.displayId }) {
            Text(settings.displayId == nil ? "Choose a display" : "Selected display disconnected")
              .tag(settings.displayId ?? "")
          }
          ForEach(Array(controller.displays.enumerated()), id: \.element.id) { index, display in
            Text("\(index + 1). \(display.name)\(display.mirrored ? " · mirrored" : "")")
              .tag(display.id).disabled(display.mirrored)
          }
        }.font(metrics.font(12))
        Picker(
          "Size",
          selection: Binding(get: { settings.size }, set: { value in change { $0.size = value } })
        ) {
          Text("Compact").tag(NotchSize.compact)
          Text("Standard").tag(NotchSize.standard)
          Text("Large").tag(NotchSize.large)
        }.pickerStyle(SegmentedPickerStyle()).font(metrics.font(12))
        Picker(
          "Edge",
          selection: Binding(get: { settings.edge }, set: { value in change { $0.edge = value } })
        ) {
          Text("Left").tag(NotchCore.Edge.left)
          Text("Right").tag(NotchCore.Edge.right)
        }.pickerStyle(SegmentedPickerStyle()).font(metrics.font(12))
        Text("Only on this display. Hidden while disconnected or mirrored.").font(metrics.font(11))
          .foregroundColor(.gray)
        Button {
          controller.emit(.refresh)
        } label: {
          Label("Refresh usage", systemImage: "arrow.clockwise")
        }
        .font(metrics.font(12))
      }.frame(maxWidth: .infinity, alignment: .leading)
    }.disabled(controller.pendingRequest != nil)
  }
}
