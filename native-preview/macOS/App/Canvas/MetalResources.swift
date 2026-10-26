import MetalKit

@MainActor
final class MetalResources {
  static let shared = try? MetalResources()
  let device: MTLDevice
  let queue: MTLCommandQueue
  let pipeline: MTLRenderPipelineState

  private init() throws {
    guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
      throw URLError(.unsupportedURL)
    }
    self.device = device
    self.queue = queue
    let source = """
      #include <metal_stdlib>
      using namespace metal;
      struct V { float4 position [[position]]; float2 uv; };
      vertex V vertexMain(uint i [[vertex_id]]) {
          float2 p[3] = {float2(-1,-1),float2(3,-1),float2(-1,3)};
          V v; v.position=float4(p[i],0,1); v.uv=float2((p[i].x+1)*0.5,1-(p[i].y+1)*0.5); return v;
      }
      fragment float4 fragmentMain(V in [[stage_in]], texture2d<float> surface [[texture(0)]]) {
          constexpr sampler s(filter::linear, address::clamp_to_edge);
          return surface.sample(s,in.uv);
      }
      """
    let library = try device.makeLibrary(source: source, options: nil)
    let descriptor = MTLRenderPipelineDescriptor()
    descriptor.vertexFunction = library.makeFunction(name: "vertexMain")
    descriptor.fragmentFunction = library.makeFunction(name: "fragmentMain")
    descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
    pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
  }
}
