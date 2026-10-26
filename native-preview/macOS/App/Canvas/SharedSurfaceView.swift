import AppKit
import IOSurface
import MetalKit
import RndrGPUBridge

/// The fragment shader samples an iOS-owned IOSurface directly. No pixel bytes
/// enter the Mac process through HTTP, and no NSImage or image codec is involved.
@MainActor
final class SharedSurfaceView: MTKView, MTKViewDelegate {
  var onFrame: (() -> Void)?
  private var visible = true
  private let componentID: String
  private let receiver = RndrSurfaceReceiver()
  private let queue: MTLCommandQueue
  private let pipeline: MTLRenderPipelineState
  private var sharedSurface: IOSurface?
  private var texture: MTLTexture?
  private var surfaceID: UInt32 = 0
  private(set) var presentedFrames = 0

  init(deviceID: String, componentID: String) throws {
    guard let resources = MetalResources.shared else { throw URLError(.unsupportedURL) }
    let device = resources.device
    self.componentID = componentID
    queue = resources.queue
    pipeline = resources.pipeline
    super.init(frame: .zero, device: device)
    clearColor = MTLClearColorMake(0, 0, 0, 0)
    colorPixelFormat = .bgra8Unorm
    layer?.isOpaque = false
    preferredFramesPerSecond = 60
    isPaused = true
    enableSetNeedsDisplay = true
    delegate = self
    receiver.onSurface = { [weak self] in
      Task { @MainActor in
        guard let self else { return }
        if self.visible { self.needsDisplay = true }
        self.onFrame?()
      }
    }
    try receiver.start(withDeviceID: deviceID, componentID: componentID)
    ViewerMetrics.shared.opened(componentID)
  }

  required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

  func stop() {
    onFrame = nil
    isPaused = true
    delegate = nil
    receiver.onSurface = nil
    receiver.stop()
    ViewerMetrics.shared.closed(componentID)
  }

  func setVisible(_ value: Bool) {
    guard value != visible else { return }
    visible = value
    if value { needsDisplay = true } else { releaseDrawables() }
  }

  override func hitTest(_ point: NSPoint) -> NSView? { nil }

  func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

  func draw(in view: MTKView) {
    guard visible, let device, let surface = receiver.copyLatestSurface() else { return }
    let id = IOSurfaceGetID(surface)
    if texture == nil || id != surfaceID {
      let descriptor = MTLTextureDescriptor.texture2DDescriptor(
        pixelFormat: .bgra8Unorm, width: IOSurfaceGetWidth(surface),
        height: IOSurfaceGetHeight(surface), mipmapped: false)
      descriptor.usage = .shaderRead
      descriptor.storageMode = .shared
      texture = device.makeTexture(descriptor: descriptor, iosurface: surface, plane: 0)
      sharedSurface = surface
      surfaceID = id
    }
    guard let texture, let drawable = currentDrawable, let pass = currentRenderPassDescriptor,
      let command = queue.makeCommandBuffer(),
      let encoder = command.makeRenderCommandEncoder(descriptor: pass)
    else { return }
    encoder.setRenderPipelineState(pipeline)
    encoder.setFragmentTexture(texture, index: 0)
    encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
    encoder.endEncoding()
    let instanceID = componentID
    command.addCompletedHandler { _ in
      Task { @MainActor in ViewerMetrics.shared.completed(instanceID) }
    }
    ViewerMetrics.shared.submitted(
      componentID, surface: surfaceID, handles: Int(receiver.receivedSurfaces))
    command.present(drawable)
    command.commit()
    presentedFrames += 1
  }

  var proof: String {
    "IOSurface \(surfaceID) · \(presentedFrames) presents"
  }
}
