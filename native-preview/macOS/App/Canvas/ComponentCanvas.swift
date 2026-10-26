import AppKit
import SwiftUI

/// SwiftUI wrapper around the AppKit canvas that displays the shared Metal texture.
struct ComponentCanvas: NSViewRepresentable {
  let componentID: String
  let overview: Bool
  let revision: Int
  let width: Double

  func makeNSView(context: Context) -> ComponentCanvasView {
    let view = ComponentCanvasView(componentID: componentID)
    view.update(revision: revision, width: width, overview: overview)
    return view
  }

  func updateNSView(_ view: ComponentCanvasView, context: Context) {
    view.update(revision: revision, width: width, overview: overview)
  }

  static func dismantleNSView(_ view: ComponentCanvasView, coordinator: ()) { view.disconnect() }
}

@MainActor
final class ComponentCanvasView: NSView {
  private let client: PreviewClient
  private var revision: Int?
  private var requestedWidth = 360.0
  private var surface: ComponentInfo?
  private var metalView: SharedSurfaceView?
  private var input: SimulatorInput?
  private var connection: Task<Void, Never>?
  private var scrollObserver: NSObjectProtocol?
  private var visible = true
  private var resizeTask: Task<Void, Never>?
  private var touching = false
  private var overview = false
  private var message = "Connecting to the iOS host…"

  override var isFlipped: Bool { true }
  override var acceptsFirstResponder: Bool { true }
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

  init(componentID: String) {
    client = PreviewClient(componentID: componentID)
    super.init(frame: .zero)
    clipsToBounds = true
    wantsLayer = true
    setAccessibilityElement(true)
    setAccessibilityRole(.image)
    setAccessibilityIdentifier("canvas.\(componentID)")
    setAccessibilityLabel("Live iOS preview")
  }

  required init?(coder: NSCoder) { nil }

  func update(revision: Int, width: Double, overview: Bool) {
    if self.overview != overview {
      self.overview = overview
      needsLayout = true
      needsDisplay = true
    }
    let changed = requestedWidth != width
    requestedWidth = width
    guard self.revision != revision else {
      if changed, metalView != nil { resizeComponent() }
      return
    }
    self.revision = revision
    disconnect()
    message = "Connecting to the iOS host…"
    needsDisplay = true
    connection = Task { [weak self] in
      do {
        guard let self else { return }
        let config = try await PreviewRuntime.shared.configuration()
        try Task.checkCancellation()
        let input = SimulatorInput.shared
        input.attach(id: client.componentID, url: config.inputURL) { [weak self] in
          self?.showDisconnected()
        }
        self.input = input
        PreviewRuntime.shared.attach(client.componentID) { [weak self] in self?.showDisconnected() }
        surface = try await client.resize(width: requestedWidth)
        try Task.checkCancellation()
        // Register the receiver before asking iOS to send a surface handle.
        let metal = try SharedSurfaceView(deviceID: config.device, componentID: client.componentID)
        addSubview(metal)
        metalView = metal
        try await client.startSurface()
        try Task.checkCancellation()
        needsLayout = true
        resizeComponent()  // Catch width changes made during connection setup.
        metal.onFrame = { [weak self] in
          guard let self else { return }
          self.message = self.metalView?.proof ?? "Surface disconnected"
          self.setAccessibilityValue(self.message)
          if !self.overview { self.needsDisplay = true }
        }
        observeScrolling()
        updateVisibility()

      } catch {
        if !Task.isCancelled { self?.showDisconnected() }
      }
    }
  }

  private func resizeComponent() {
    guard resizeTask == nil else { return }
    let revision = self.revision
    resizeTask = Task { [weak self] in
      guard let self else { return }
      defer { if self.revision == revision { resizeTask = nil } }
      do {
        repeat {
          let width = requestedWidth
          let info = try await client.resize(width: width)
          try Task.checkCancellation()
          surface = info
          needsLayout = true
          if width == requestedWidth { break }
        } while !Task.isCancelled
      } catch {
        if !Task.isCancelled { showDisconnected() }
      }
    }
  }

  func disconnect() {
    connection?.cancel()
    connection = nil
    resizeTask?.cancel()
    resizeTask = nil
    input?.release(id: client.componentID)
    input = nil
    PreviewRuntime.shared.detach(client.componentID)
    metalView?.stop()
    metalView?.removeFromSuperview()
    metalView = nil
    surface = nil
    if let scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }
    scrollObserver = nil
    touching = false
  }

  private func showDisconnected() {
    disconnect()
    message = "Start scripts/run-ios.sh <simulator-udid>, then reconnect the viewer."
    setAccessibilityValue(message)
    needsDisplay = true
  }

  private var displayRect: NSRect {
    guard let surface else { return .zero }
    if overview {
      let scale = min(
        max(0, bounds.width - 8) / surface.width, max(0, bounds.height - 8) / surface.height)
      let size = NSSize(width: surface.width * scale, height: surface.height * scale)
      return NSRect(
        x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width,
        height: size.height)
    }
    let scale = min(
      1, max(0, bounds.width - 64) / surface.width,
      max(0, bounds.height - 88) / surface.height)
    let size = NSSize(width: surface.width * scale, height: surface.height * scale)
    return NSRect(
      x: (bounds.width - size.width) / 2, y: 56 + (bounds.height - 72 - size.height) / 2,
      width: size.width, height: size.height)
  }

  override func layout() {
    super.layout()
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    metalView?.frame = displayRect
    CATransaction.commit()
    updateVisibility()
  }

  override func viewDidMoveToWindow() {
    super.viewDidMoveToWindow()
    observeScrolling()
    updateVisibility()
  }

  private func observeScrolling() {
    if let scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }
    if let clip = enclosingScrollView?.contentView {
      clip.postsBoundsChangedNotifications = true
      scrollObserver = NotificationCenter.default.addObserver(
        forName: NSView.boundsDidChangeNotification,
        object: clip, queue: .main
      ) { [weak self] _ in
        Task { @MainActor in self?.updateVisibility() }
      }
    }
    updateVisibility()
  }

  private func updateVisibility() {
    let next = window != nil && !visibleRect.isEmpty && !bounds.isEmpty
    if next != visible || metalView != nil {
      visible = next
      metalView?.setVisible(next)
      PreviewRuntime.shared.setVisible(client.componentID, next && metalView != nil)
    }
  }

  override func draw(_ dirtyRect: NSRect) {
    NSColor.windowBackgroundColor.setFill()
    bounds.fill()
    guard !overview else { return }
    (message as NSString).draw(
      in: NSRect(x: 24, y: 20, width: bounds.width - 48, height: 36),
      withAttributes: [
        .font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.secondaryLabelColor,
      ])
  }

  override func mouseDown(with event: NSEvent) {
    guard metalView != nil, displayRect.contains(convert(event.locationInWindow, from: nil)) else {
      return
    }
    window?.makeFirstResponder(self)
    touching = true
    touch("begin", event: event)
  }

  override func mouseDragged(with event: NSEvent) { if touching { touch("move", event: event) } }
  override func mouseUp(with event: NSEvent) {
    if touching { touch("end", event: event) }
    touching = false
  }

  private func touch(_ type: String, event: NSEvent) {
    let rect = displayRect
    guard rect.width > 0, rect.height > 0, let surface else { return }
    let point = convert(event.locationInWindow, from: nil)
    let x = max(0, min(1, (point.x - rect.minX) / rect.width)) * surface.width
    let y = max(0, min(1, (point.y - rect.minY) / rect.height)) * surface.height
    input?.touch(
      type, componentID: client.componentID, x: (surface.originX + x) / surface.screenWidth,
      y: (surface.originY + y) / surface.screenHeight,
      width: surface.screenWidth * surface.scale, height: surface.screenHeight * surface.scale)
  }

  override func keyDown(with event: NSEvent) {
    if input?.key(event) != true { super.keyDown(with: event) }
  }
}
