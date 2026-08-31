import AppKit
import NotchCore
import SwiftUI

final class NotchPanel: NSPanel {
  var collapse: (() -> Void)?
  override var canBecomeKey: Bool { true }
  override var canBecomeMain: Bool { false }
  override func cancelOperation(_ sender: Any?) { collapse?() }
}

final class NotchHostingView: NSHostingView<NotchView> {
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

@MainActor
final class PanelController: NSObject, NSWindowDelegate, ObservableObject {
  @Published var message: HostMessage?
  @Published var selection: String?
  @Published var now = Date()
  @Published var pendingRequest: UInt64?
  @Published var displays: [Display] = []
  private var request: UInt64 = 0
  private var lastSequence: UInt64 = 0
  private let panel: NotchPanel
  private var observers: [NSObjectProtocol] = []
  private var workspaceObservers: [NSObjectProtocol] = []
  private var outsideClick: Any?
  private var timer: Timer?
  var emit: (ClientAction) -> Void = { _ in }

  override init() {
    panel = NotchPanel(
      contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
      defer: false)
    super.init()
    panel.title = "on-n-off Side notch"
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false
    panel.level = .floating
    panel.hidesOnDeactivate = false
    panel.isReleasedWhenClosed = false
    panel.collectionBehavior = [
      .canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle,
    ]
    panel.delegate = self
    panel.collapse = { [weak self] in self?.select(nil) }
    panel.contentView = NotchHostingView(rootView: NotchView(controller: self))
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

  func shutdown() {
    timer?.invalidate()
    timer = nil
    observers.forEach(NotificationCenter.default.removeObserver)
    observers.removeAll()
    workspaceObservers.forEach(NSWorkspace.shared.notificationCenter.removeObserver)
    workspaceObservers.removeAll()
    if let outsideClick = outsideClick { NSEvent.removeMonitor(outsideClick) }
    outsideClick = nil
    panel.orderOut(nil)
    panel.contentView = nil
  }

  func accept(_ next: HostMessage) {
    guard next.sequence > lastSequence else { return }
    lastSequence = next.sequence
    let moved =
      message?.snapshot.settings.displayId != next.snapshot.settings.displayId
      || message?.snapshot.settings.edge != next.snapshot.settings.edge
    message = next
    if moved { selection = nil }
    if let pending = pendingRequest, next.completedRequest == pending { pendingRequest = nil }
    updateFrame()
    emit(.ack(sequence: next.sequence))
  }

  func select(_ value: String?) {
    guard selection != value else { return }
    selection = value
    updateFrame()
    if value != nil, panel.isVisible {
      NSApplication.shared.activate(ignoringOtherApps: true)
      panel.makeKeyAndOrderFront(nil)
    }
  }

  func toggle(_ value: String) { select(selection == value ? nil : value) }
  func windowDidResignKey(_ notification: Notification) { select(nil) }
  func windowShouldClose(_ sender: NSWindow) -> Bool {
    select(nil)
    return false
  }

  func save(_ settings: NotchCore.Settings) {
    guard pendingRequest == nil, let message = message else { return }
    request += 1
    pendingRequest = request
    emit(.save(settings: settings, revision: message.revision ?? 0, request: request))
  }

  func screensChanged() {
    updateFrame()
    emit(.screensChanged)
  }

  private func updateFrame() {
    displays = Screens.read()
    guard let message = message, message.snapshot.error == nil,
      let frame = notchFrame(
        settings: message.snapshot.settings, displays: displays, expanded: selection != nil)
    else {
      selection = nil
      panel.orderOut(nil)
      return
    }
    panel.setFrame(Screens.appKitFrame(frame), display: true)
    if !panel.isVisible { panel.orderFrontRegardless() }
  }
}
