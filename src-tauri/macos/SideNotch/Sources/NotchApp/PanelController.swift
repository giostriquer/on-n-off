import AppKit
import NotchCore
import SwiftUI

final class NotchPanel: NSPanel {
  var collapse: (() -> Void)?
  var keyable = false
  override var canBecomeKey: Bool { keyable }
  override var canBecomeMain: Bool { false }
  override func cancelOperation(_ sender: Any?) { collapse?() }
}

final class TrackingHostingView<Content: View>: NSHostingView<Content> {
  var entered: (() -> Void)?
  private var tracking: NSTrackingArea?
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
  override func updateTrackingAreas() {
    super.updateTrackingAreas()
    if let tracking = tracking { removeTrackingArea(tracking) }
    let area = NSTrackingArea(
      rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
      owner: self, userInfo: nil)
    addTrackingArea(area)
    tracking = area
  }
  override func mouseEntered(with event: NSEvent) { entered?() }
}

let tailLength = 8.0
private let hoverOpenDelay = 0.12
private let hoverCloseGrace = 0.35
private let pointerPollInterval = 0.08
private var motionDuration: TimeInterval {
  NSWorkspace.shared.accessibilityDisplayShouldReduceMotion ? 0 : 0.18
}
private let edgeSlide = 10.0

@MainActor
private func animate(
  _ changes: () -> Void, completion: (@MainActor @Sendable () -> Void)? = nil
) {
  NSAnimationContext.runAnimationGroup(
    { context in
      context.duration = motionDuration
      context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
      changes()
    },
    completionHandler: completion.map { done in
      { @Sendable in MainActor.assumeIsolated { done() } }
    })
}

private func slidOut(_ frame: NSRect, edge: NotchCore.Edge) -> NSRect {
  switch edge {
  case .right: return frame.offsetBy(dx: edgeSlide, dy: 0)
  case .left: return frame.offsetBy(dx: -edgeSlide, dy: 0)
  case .top: return frame.offsetBy(dx: 0, dy: edgeSlide)
  case .bottom: return frame.offsetBy(dx: 0, dy: -edgeSlide)
  }
}

struct PopoverPlacement {
  let view: NotchPopoverView
  let frame: CGRect
}

@MainActor
func popoverPlacement(
  rail: RailModel, message: HostMessage, cell: RailCell, now: Date,
  openLimits: @escaping () -> Void, openPullRequests: @escaping () -> Void
) -> PopoverPlacement? {
  guard let frame = rail.cellFrame(of: cell) else { return nil }
  let metrics = rail.metrics
  let tail = metrics.value(CGFloat(tailLength))
  let width = metrics.value(CGFloat(popoverWidth))
  let margin = pixelAligned(popoverMargin * rail.settings.size.scale, displayScale: rail.display.scale)
  let maxHeight = min(metrics.value(560), CGFloat(rail.display.workHeight - 2 * margin))
  let content: PopoverContent
  switch cell {
  case .provider(let id):
    content = .provider(id, message.providers.first { $0.provider == id })
  case .pullRequests:
    content = .pullRequests(message.pullRequests)
  }
  func model(tailAt: CGFloat) -> PopoverModel {
    PopoverModel(
      content: content, now: now, edge: rail.edge, metrics: metrics, width: width,
      maxHeight: maxHeight, tailLength: tail, tail: tailAt, actionError: message.actionError,
      openLimits: openLimits, openPullRequests: openPullRequests)
  }
  let probe = NSHostingView(rootView: NotchPopoverView(model: model(tailAt: 0)))
  var size = probe.fittingSize
  size.height = min(size.height, maxHeight + (rail.edge.isVertical ? 0 : tail))
  let placed = popoverFrame(
    cell: frame, edge: rail.edge, size: size, display: rail.display,
    scale: rail.settings.size.scale)
  let tailAt = rail.edge.isVertical ? frame.midY - placed.minY : frame.midX - placed.minX
  return PopoverPlacement(view: NotchPopoverView(model: model(tailAt: tailAt)), frame: placed)
}

@MainActor
final class PanelController: NSObject, NSWindowDelegate {
  private var message: HostMessage?
  private var rail: RailModel?
  private var now = Date()
  private var activeCell: RailCell?
  private var hovered: RailCell?
  private var pinned: RailCell?
  private var capHovered = false
  private var railOpen = false
  private var lastSequence: UInt64 = 0
  private let pillPanel: NotchPanel
  private let railPanel: NotchPanel
  private let popoverPanel: NotchPanel
  private var railHost: TrackingHostingView<NotchRailView?>!
  private var pillHost: TrackingHostingView<NotchPillView>!
  private var popoverHost: TrackingHostingView<NotchPopoverView>!
  private var observers: [NSObjectProtocol] = []
  private var workspaceObservers: [NSObjectProtocol] = []
  private var outsideClick: Any?
  private var clock: Timer?
  private var shownMinute = 0
  private var pointerPoll: Timer?
  private var openWork: DispatchWorkItem?
  private var lastInside = Date.distantPast
  private var popoverGeneration = 0
  private var railGeneration = 0
  var emit: (ClientAction) -> Void = { _ in }

  override init() {
    pillPanel = Self.makePanel(title: "on-n-off Side notch handle")
    railPanel = Self.makePanel(title: "on-n-off Side notch")
    popoverPanel = Self.makePanel(title: "on-n-off Side notch details")
    super.init()
    let railHost = TrackingHostingView<NotchRailView?>(rootView: nil)
    railHost.entered = { [weak self] in self?.startPolling() }
    let pillHost = TrackingHostingView(rootView: NotchPillView(vertical: true))
    pillHost.entered = { [weak self] in self?.pillEntered() }
    let popoverHost = TrackingHostingView(rootView: NotchPopoverView(model: nil))
    popoverHost.entered = { [weak self] in self?.startPolling() }
    self.railHost = railHost
    self.pillHost = pillHost
    self.popoverHost = popoverHost
    railPanel.contentView = railHost
    pillPanel.contentView = pillHost
    popoverPanel.contentView = popoverHost
    popoverPanel.delegate = self
    popoverPanel.collapse = { [weak self] in self?.dismiss() }
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
        MainActor.assumeIsolated { self?.dismiss() }
      })
    outsideClick = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) {
      [weak self] _ in
      MainActor.assumeIsolated { self?.dismiss() }
    }
    clock = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
      MainActor.assumeIsolated { self?.tick() }
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
    clock?.invalidate()
    clock = nil
    stopPolling()
    openWork?.cancel()
    observers.forEach(NotificationCenter.default.removeObserver)
    observers.removeAll()
    workspaceObservers.forEach(NSWorkspace.shared.notificationCenter.removeObserver)
    workspaceObservers.removeAll()
    if let outsideClick = outsideClick { NSEvent.removeMonitor(outsideClick) }
    outsideClick = nil
    for panel in [pillPanel, railPanel, popoverPanel] {
      panel.orderOut(nil)
      panel.contentView = nil
    }
  }

  func accept(_ next: HostMessage) {
    guard next.sequence > lastSequence else { return }
    lastSequence = next.sequence
    let previous = message?.snapshot.settings
    message = next
    now = Date()
    let moved =
      previous?.displayId != next.snapshot.settings.displayId
      || previous?.edge != next.snapshot.settings.edge
      || previous?.size != next.snapshot.settings.size
      || previous?.show != next.snapshot.settings.show
      || previous?.railCells != next.snapshot.settings.railCells
    if moved {
      pinned = nil
      hovered = nil
      railOpen = false
      hidePopover()
    }
    relayout()
    emit(.ack(sequence: next.sequence))
  }

  func screensChanged() {
    relayout()
    emit(.screensChanged)
  }

  private func relayout() {
    rail = message.flatMap { message in
      message.snapshot.error == nil
        ? RailModel(settings: message.snapshot.settings, displays: Screens.read()) : nil
    }
    updateFrames()
  }

  private func pillEntered() {
    guard rail?.settings.show == .onHover, !railOpen else { return }
    railOpen = true
    updateFrames()
    startPolling()
  }

  private func startPolling() {
    lastInside = Date()
    guard pointerPoll == nil else { return }
    pointerPoll = Timer.scheduledTimer(withTimeInterval: pointerPollInterval, repeats: true) {
      [weak self] _ in
      MainActor.assumeIsolated { self?.pollPointer() }
    }
    pollPointer()
  }

  private func stopPolling() {
    pointerPoll?.invalidate()
    pointerPoll = nil
    if capHovered {
      capHovered = false
      refreshRail()
    }
  }

  private func pollPointer() {
    guard let rail = rail else {
      stopPolling()
      return
    }
    let location = NSEvent.mouseLocation
    let railFrame = railPanel.frame
    let inRail = railPanel.isVisible && railFrame.contains(location)
    let inPopover = popoverPanel.isVisible && popoverPanel.frame.contains(location)
    let local = CGPoint(x: location.x - railFrame.minX, y: railFrame.maxY - location.y)
    let target = inRail ? rail.cell(at: local) : nil
    let onCap = inRail && rail.capRect.contains(local)
    if onCap != capHovered {
      capHovered = onCap
      refreshRail()
    }
    if target != hovered {
      hovered = target
      openWork?.cancel()
      if let target = target {
        if pinned != nil {
          pinned = target
          showPopover(target)
        } else {
          let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
              guard let self = self, self.pinned == nil, self.hovered == target else { return }
              self.showPopover(target)
            }
          }
          openWork = work
          DispatchQueue.main.asyncAfter(deadline: .now() + hoverOpenDelay, execute: work)
        }
      }
    }
    if pinned != nil {
      if !inRail, !inPopover { stopPolling() }
      return
    }
    let sample = Date()
    if inRail || inPopover { lastInside = sample }
    if sample.timeIntervalSince(lastInside) > hoverCloseGrace {
      hidePopover()
      if rail.settings.show == .onHover, railOpen {
        railOpen = false
        updateFrames()
      }
    }
    if !inRail, !inPopover, activeCell == nil, !railOpen { stopPolling() }
  }

  private func toggleShow() {
    emit(.setShow(rail?.settings.show == .always ? .onHover : .always))
  }

  private func tick() {
    let minute = Int(Date().timeIntervalSinceReferenceDate / 60)
    guard minute != shownMinute else { return }
    shownMinute = minute
    now = Date()
    refreshRail()
    if activeCell != nil { refreshPopover() }
  }

  private func toggle(_ id: RailCell) {
    if pinned == id {
      pinned = nil
      hovered = nil
      hidePopover()
      return
    }
    pinned = id
    showPopover(id)
    popoverPanel.keyable = true
    NSApplication.shared.activate(ignoringOtherApps: true)
    popoverPanel.makeKeyAndOrderFront(nil)
  }

  func dismiss() {
    guard pinned != nil else { return }
    pinned = nil
    hovered = nil
    hidePopover()
  }

  private func dismissAfterAction() {
    pinned = nil
    hovered = nil
    hidePopover()
  }

  func windowShouldClose(_ sender: NSWindow) -> Bool {
    dismiss()
    return false
  }

  private enum PopoverTransition { case none, appear, morph }

  private func showPopover(_ id: RailCell) {
    now = Date()
    let switching = popoverPanel.isVisible && activeCell != nil && activeCell != id
    activeCell = id
    popoverGeneration += 1
    refreshPopover(transition: switching ? .morph : .appear)
    refreshRail()
  }

  private func hidePopover() {
    guard activeCell != nil else { return }
    activeCell = nil
    popoverPanel.keyable = false
    popoverGeneration += 1
    let generation = popoverGeneration
    animate {
      popoverPanel.animator().alphaValue = 0
    } completion: { [weak self] in
      guard let self = self, self.popoverGeneration == generation else { return }
      self.popoverPanel.orderOut(nil)
      self.popoverPanel.alphaValue = 1
      self.popoverHost.rootView = NotchPopoverView(model: nil)
    }
    refreshRail()
  }

  private func refreshRail() {
    guard let rail = rail, let message = message else {
      railHost.rootView = nil
      return
    }
    railHost.rootView = NotchRailView(
      model: rail,
      entries: Dictionary(uniqueKeysWithValues: message.providers.map { ($0.provider, $0) }),
      pullRequests: message.pullRequests, now: now, active: activeCell,
      capHovered: capHovered, action: { [weak self] id in self?.toggle(id) },
      toggleShow: { [weak self] in self?.toggleShow() })
  }

  private func refreshPopover(transition: PopoverTransition = .none) {
    guard let id = activeCell, let rail = rail, let message = message,
      let placement = popoverPlacement(
        rail: rail, message: message, cell: id, now: now,
        openLimits: { [weak self] in
          self?.emit(.openLimits)
          self?.dismissAfterAction()
        },
        openPullRequests: { [weak self] in
          self?.emit(.openPullRequests)
          self?.dismissAfterAction()
        })
    else {
      hidePopover()
      return
    }
    let frame = Screens.appKitFrame(placement.frame)
    switch transition {
    case .appear:
      popoverHost.rootView = placement.view
      popoverPanel.setFrame(frame, display: true)
      popoverPanel.alphaValue = 0
      popoverPanel.orderFrontRegardless()
      animate { popoverPanel.animator().alphaValue = 1 }
    case .morph:
      popoverPanel.alphaValue = 1
      withAnimation(.easeInOut(duration: motionDuration)) {
        popoverHost.rootView = placement.view
      }
      animate { popoverPanel.animator().setFrame(frame, display: true) }
    case .none:
      popoverHost.rootView = placement.view
      if popoverPanel.frame != frame { popoverPanel.setFrame(frame, display: true) }
      if !popoverPanel.isVisible {
        popoverPanel.alphaValue = 1
        popoverPanel.orderFrontRegardless()
      }
    }
  }

  private func updateFrames() {
    guard let rail = rail, let pill = notchPillFrame(settings: rail.settings, displays: [rail.display])
    else {
      pinned = nil
      hovered = nil
      railOpen = false
      hidePopover()
      railPanel.orderOut(nil)
      pillPanel.orderOut(nil)
      railHost.rootView = nil
      stopPolling()
      return
    }
    let railFrame = Screens.appKitFrame(rail.frame)
    pillPanel.setFrame(Screens.appKitFrame(pill), display: true)
    pillHost.rootView = NotchPillView(vertical: rail.edge.isVertical)
    refreshRail()
    let railVisible = rail.settings.show == .always || railOpen
    if railVisible {
      revealRail(at: railFrame, edge: rail.edge)
    } else {
      concealRail(edge: rail.edge)
    }
    if rail.settings.show == .onHover, !railOpen {
      if !pillPanel.isVisible {
        pillPanel.alphaValue = 0
        pillPanel.orderFrontRegardless()
        animate { pillPanel.animator().alphaValue = 1 }
      }
    } else {
      pillPanel.orderOut(nil)
    }
    if activeCell != nil, railVisible { refreshPopover() } else { hidePopover() }
  }

  private func revealRail(at frame: NSRect, edge: NotchCore.Edge) {
    railGeneration += 1
    if !railPanel.isVisible {
      railPanel.setFrame(slidOut(frame, edge: edge), display: true)
      railPanel.alphaValue = 0
      railPanel.orderFrontRegardless()
    } else if railPanel.frame == frame, railPanel.alphaValue == 1 {
      return
    }
    animate {
      railPanel.animator().setFrame(frame, display: true)
      railPanel.animator().alphaValue = 1
    }
  }

  private func concealRail(edge: NotchCore.Edge) {
    guard railPanel.isVisible else { return }
    railGeneration += 1
    let generation = railGeneration
    let frame = railPanel.frame
    animate {
      railPanel.animator().setFrame(slidOut(frame, edge: edge), display: true)
      railPanel.animator().alphaValue = 0
    } completion: { [weak self] in
      guard let self = self, self.railGeneration == generation else { return }
      self.railPanel.orderOut(nil)
      self.railPanel.setFrame(frame, display: false)
      self.railPanel.alphaValue = 1
    }
  }
}
