import AppKit
import CoreGraphics
import NotchCore

@MainActor
enum Screens {
  static func read() -> [Display] {
    NSScreen.screens.compactMap { screen in
      guard
        let number = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")]
          as? NSNumber,
        let uuid = CGDisplayCreateUUIDFromDisplayID(number.uint32Value)?.takeRetainedValue(),
        let text = CFUUIDCreateString(nil, uuid)
      else { return nil }
      let id = number.uint32Value
      let bounds = CGDisplayBounds(id)
      let visible = screen.visibleFrame
      return Display(
        id: text as String, name: screen.localizedName,
        x: bounds.minX, y: bounds.minY, width: bounds.width, height: bounds.height,
        workY: bounds.minY + screen.frame.maxY - visible.maxY,
        workHeight: visible.height, scale: screen.backingScaleFactor,
        mirrored: CGDisplayIsInMirrorSet(id) != 0)
    }
  }

  static func appKitFrame(_ frame: CGRect) -> CGRect {
    let desktopTop = NSScreen.screens.first?.frame.maxY ?? 0
    return CGRect(
      x: frame.minX, y: desktopTop - frame.maxY, width: frame.width, height: frame.height)
  }
}
