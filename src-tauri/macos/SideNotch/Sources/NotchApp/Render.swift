import AppKit
import NotchCore
import SwiftUI

/// `on-n-off-notch --render <message.json> <out-dir>`: draws the rail, the hover pill, and one
/// popover per provider from a fixture host message into PNGs, so native visuals can be checked
/// without a display. The fixture's displays stand in for the live screens; no panel is created.
@MainActor
enum Render {
  static func run(messagePath: String, outputDirectory: String) -> Int32 {
    do {
      let message = try HostMessage.decode(Data(contentsOf: URL(fileURLWithPath: messagePath)))
      let settings = message.snapshot.settings
      let displays = message.snapshot.displays
      guard let rail = RailModel(settings: settings, displays: displays) else {
        FileHandle.standardError.write(Data("The fixture does not show a rail.\n".utf8))
        return 2
      }
      let now = Date()
      let entries = Dictionary(uniqueKeysWithValues: message.providers.map { ($0.provider, $0) })
      let out = URL(fileURLWithPath: outputDirectory, isDirectory: true)
      try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
      try write(
        NotchRailView(
          model: rail, entries: entries, pullRequests: message.pullRequests, now: now, active: nil,
          action: { _ in }, toggleShow: {}),
        size: rail.frame.size, scale: rail.display.scale, to: out.appendingPathComponent("rail.png"))
      if let pill = notchPillFrame(settings: settings, displays: displays) {
        try write(
          NotchPillView(vertical: settings.edge.isVertical), size: pill.size,
          scale: rail.display.scale, to: out.appendingPathComponent("pill.png"))
      }
      for cell in rail.cellIds {
        guard
          let placement = popoverPlacement(
            rail: rail, message: message, cell: cell, now: now, openLimits: {},
            openPullRequests: {})
        else { continue }
        let name: String
        switch cell {
        case .provider(let id): name = id.rawValue
        case .pullRequests: name = "pull-requests"
        }
        try write(
          placement.view, size: placement.frame.size, scale: rail.display.scale,
          to: out.appendingPathComponent("popover-\(name).png"))
        print("popover-\(name) at \(placement.frame)")
      }
      print("rail at \(rail.frame)")
      return 0
    } catch {
      FileHandle.standardError.write(Data("Render failed: \(error)\n".utf8))
      return 1
    }
  }

  private static func write<Content: View>(
    _ view: Content, size: CGSize, scale: Double, to url: URL
  ) throws {
    let host = NSHostingView(rootView: view)
    host.frame = CGRect(origin: .zero, size: size)
    let window = NSWindow(
      contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.isOpaque = false
    window.backgroundColor = .clear
    window.contentView = host
    defer {
      window.contentView = nil
      window.close()
    }
    host.layoutSubtreeIfNeeded()
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int((size.width * scale).rounded()),
        pixelsHigh: Int((size.height * scale).rounded()), bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0,
        bitsPerPixel: 0)
    else { throw RenderError.bitmap }
    bitmap.size = size
    host.cacheDisplay(in: host.bounds, to: bitmap)
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
      throw RenderError.bitmap
    }
    try png.write(to: url)
  }

  enum RenderError: Error { case bitmap }
}
