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
    ScrollView(.vertical, showsIndicators: true) {
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
                Capsule().fill(limitColor(quota, at: now, base: color)).frame(
                  width: geometry.size.width * (quota.percent(at: now) ?? 0) / 100)
              }
            }.frame(height: metrics.value(5)).accessibilityElement(children: .ignore)
              .accessibilityLabel(quota.label)
              .accessibilityValue(
                quota.percent(at: now) == nil
                  ? "Not observed since the reset"
                  : "\(quota.text(at: now)) used"
                    + (quota.isReached(at: now) ? ", limit reached" : ""))
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
