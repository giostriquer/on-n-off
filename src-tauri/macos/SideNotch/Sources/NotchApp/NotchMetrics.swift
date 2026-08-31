import SwiftUI

struct NotchMetrics {
  let scale: CGFloat

  // Resolve preset sizes before drawing so small system-font glyphs stay pixel-aligned.
  func value(_ points: CGFloat) -> CGFloat {
    points * scale
  }

  func font(_ points: CGFloat, weight: Font.Weight = .regular) -> Font {
    .system(size: value(points), weight: weight, design: .default)
  }
}
