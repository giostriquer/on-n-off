import AppKit
import NotchCore
import SwiftUI

final class NotchPanel: NSPanel {
  var collapse: (() -> Void)?
  override var canBecomeKey: Bool { true }
  override var canBecomeMain: Bool { false }
  override func cancelOperation(_ sender: Any?) { collapse?() }
}

final class RailHostingView: NSHostingView<NotchRailView> {
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class DetailHostingView: NSHostingView<NotchDetailView> {
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

@MainActor
final class PanelController: NSObject, NSWindowDelegate, ObservableObject {
  @Published var message: HostMessage?
  @Published var selection: String?
  @Published var now = Date()
  private var lastSequence: UInt64 = 0
  private let railPanel: NotchPanel
  private let detailPanel: NotchPanel
  private var observers: [NSObjectProtocol] = []
  private var workspaceObservers: [NSObjectProtocol] = []
  private var outsideClick: Any?
  private var timer: Timer?
  var emit: (ClientAction) -> Void = { _ in }

  override init() {
    railPanel = Self.makePanel(title: "on-n-off Side notch")
    detailPanel = Self.makePanel(title: "on-n-off Side notch details")
    super.init()
    detailPanel.delegate = self
    detailPanel.collapse = { [weak self] in self?.select(nil) }
    railPanel.contentView = RailHostingView(rootView: NotchRailView(controller: self))
    detailPanel.contentView = DetailHostingView(rootView: NotchDetailView(controller: self))
    observers.append(
      NotificationCenter.default.addObserver(
        forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
      ) { [weak self] _ in
        MainActor.assumeIsolated { self?.screensChanged() }
      })
    workspaceObservers.append(
      NSWorkspace.shared.notificationCenter.addObserver(
        forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
      ) { [weak self] _ in
        MainActor.assumeIsolated { self?.screensChanged() }
      })
    observers.append(
      NotificationCenter.default.addObserver(
        forName: NSApplication.didResignActiveNotification, object: nil, queue: .main
      ) { [weak self] _ in
        MainActor.assumeIsolated { self?.select(nil) }
      })
    outsideClick = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) {
      [weak self] _ in
      MainActor.assumeIsolated { self?.select(nil) }
    }
    timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
      MainActor.assumeIsolated { self?.now = Date() }
    }
  }

  private static func makePanel(title: String) -> NotchPanel {
    let panel = NotchPanel(
      contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
      defer: false)
    panel.title = title
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false
    panel.level = .floating
    panel.hidesOnDeactivate = false
    panel.isReleasedWhenClosed = false
    panel.collectionBehavior = [
      .canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle,
    ]
    return panel
  }

  func shutdown() {
    timer?.invalidate()
    timer = nil
    observers.forEach(NotificationCenter.default.removeObserver)
    observers.removeAll()
    workspaceObservers.forEach(NSWorkspace.shared.notificationCenter.removeObserver)
    workspaceObservers.removeAll()
    if let outsideClick = outsideClick { NSEvent.removeMonitor(outsideClick) }
    outsideClick = nil
    for panel in [railPanel, detailPanel] {
      panel.orderOut(nil)
      panel.contentView = nil
    }
  }

  func accept(_ next: HostMessage) {
    guard next.sequence > lastSequence else { return }
    lastSequence = next.sequence
    let moved =
      message?.snapshot.settings.displayId != next.snapshot.settings.displayId
      || message?.snapshot.settings.edge != next.snapshot.settings.edge
      || message?.snapshot.settings.size != next.snapshot.settings.size
    message = next
    if moved { selection = nil }
    updateFrames()
    emit(.ack(sequence: next.sequence))
  }

  func select(_ value: String?) {
    guard selection != value else { return }
    selection = value
    updateFrames()
    if value != nil, detailPanel.isVisible {
      NSApplication.shared.activate(ignoringOtherApps: true)
      detailPanel.makeKeyAndOrderFront(nil)
    }
  }

  func toggle(_ value: String) { select(selection == value ? nil : value) }

  func windowShouldClose(_ sender: NSWindow) -> Bool {
    select(nil)
    return false
  }

  func screensChanged() {
    updateFrames()
    emit(.screensChanged)
  }

  private func updateFrames() {
    guard let message = message, message.snapshot.error == nil,
      let frames = notchPanelFrames(settings: message.snapshot.settings, displays: Screens.read())
    else {
      selection = nil
      railPanel.orderOut(nil)
      detailPanel.orderOut(nil)
      return
    }
    railPanel.setFrame(Screens.appKitFrame(frames.rail), display: true)
    detailPanel.setFrame(Screens.appKitFrame(frames.detail), display: true)
    if !railPanel.isVisible { railPanel.orderFrontRegardless() }
    if selection == nil {
      detailPanel.orderOut(nil)
    } else if !detailPanel.isVisible {
      detailPanel.orderFrontRegardless()
    }
  }
}
