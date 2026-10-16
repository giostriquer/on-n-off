import AppKit
import NotchCore
import SwiftUI

@MainActor
enum Render {
  static func run(messagePath: String, outputDirectory: String) -> Int32 {
    do {
      let message = try HostMessage.decode(Data(contentsOf: URL(fileURLWithPath: messagePath)))
      var settings = message.snapshot.settings
      settings.popoverStyle = .solid
      let displays = message.snapshot.displays
      guard let rail = RailModel(settings: settings, cells: message.cells, displays: displays),
        let styled = RailModel(
          settings: message.snapshot.settings, cells: message.cells, displays: displays)
      else {
        FileHandle.standardError.write(Data("The fixture does not show a rail.\n".utf8))
        return 2
      }
      let now = Date()
      let out = URL(fileURLWithPath: outputDirectory, isDirectory: true)
      try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
      func writeRail(_ model: RailModel, capHovered: Bool, name: String) throws {
        try write(
          NotchRailView(
            model: model, cells: message.cells, now: now, active: nil, capHovered: capHovered,
            action: { _ in }, toggleShow: {}),
          size: model.frame.size, scale: model.display.scale,
          to: out.appendingPathComponent("\(name).png"))
      }
      func writePopover(_ model: RailModel, _ cell: RailCell, suffix: String = "") throws {
        let name: String
        switch cell {
        case .provider(let id): name = "popover-\(id.rawValue)\(suffix)"
        case .pullRequests: name = "popover-pull-requests\(suffix)"
        }
        guard
          let placement = popoverPlacement(
            rail: model, message: message, cell: cell, now: now, openLimits: {},
            openPullRequests: {})
        else { return }
        try write(
          placement.view, size: placement.frame.size, scale: model.display.scale,
          to: out.appendingPathComponent("\(name).png"))
        print("\(name) at \(placement.frame)")
      }
      try writeRail(rail, capHovered: false, name: "rail")
      try writeRail(rail, capHovered: true, name: "rail-cap-hovered")
      if let pill = notchPillFrame(settings: settings, displays: displays) {
        try write(
          NotchPillView(vertical: settings.edge.isVertical), size: pill.size,
          scale: rail.display.scale, to: out.appendingPathComponent("pill.png"))
      }
      for cell in rail.cellIds {
        try writePopover(rail, cell)
      }
      try writeRail(styled, capHovered: false, name: "rail-styled")
      try writePopover(styled, .provider(.claude), suffix: "-styled")
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
