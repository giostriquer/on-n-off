import SwiftUI

struct NotchMetrics {
  let scale: CGFloat
  let backingScale: CGFloat

  init(scale: CGFloat, backingScale: CGFloat = 1) {
    self.scale = scale
    self.backingScale = max(backingScale, 1)
  }

  // Compact and large presets produce fractional points. Snap every native metric to the target
  // display's pixel grid so text and vector strokes do not land between pixels on 1x monitors.
  func value(_ points: CGFloat) -> CGFloat {
    (points * scale * backingScale).rounded() / backingScale
  }

  func font(_ points: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default)
    -> Font
  {
    .system(size: value(points), weight: weight, design: design)
  }
}
