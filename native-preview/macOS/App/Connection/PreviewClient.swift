import Foundation

/// Two local services: the input-only HID bridge and the iOS layout/render host.
struct PreviewClient {
  let componentID: String
  private let base = "http://127.0.0.1:43221"

  func inputConfiguration() async throws -> InputConfiguration {
    let value: InputConfiguration = try await request("http://127.0.0.1:43220/api")
    guard value.isLocal else { throw URLError(.unsupportedURL) }
    return value
  }

  func info() async throws -> ComponentInfo { try await request(base + "/info?id=\(componentID)") }

  func resize(width: Double) async throws -> ComponentInfo {
    try await request(base + "/resize?id=\(componentID)&width=\(width)", method: "POST")
  }

  func health() async throws -> PreviewHealth { try await request(base + "/health") }

  func setVisible(ids: [String]) async throws {
    var url = URLComponents(string: base + "/visibility")!
    url.queryItems = [URLQueryItem(name: "ids", value: ids.joined(separator: ","))]
    let _: PreviewHealth = try await request(url.url!.absoluteString, method: "POST")
  }

  func setMode(continuous: Bool) async throws {
    let mode = continuous ? "continuous" : "on-demand"
    let _: PreviewHealth = try await request(base + "/render-mode?mode=\(mode)", method: "POST")
  }

  func invalidate() async throws {
    let _: ComponentInfo = try await request(base + "/invalidate?id=\(componentID)", method: "POST")
  }

  func focus() async throws {
    let _: ComponentInfo = try await request(base + "/focus?id=\(componentID)", method: "POST")
  }

  func startSurface() async throws {
    let result: SurfaceExport = try await request(
      base + "/surface/start?id=\(componentID)", method: "POST")
    guard result.machSendResult == 0 else { throw URLError(.cannotConnectToHost) }
  }

  private func request<T: Decodable>(_ address: String, method: String = "GET") async throws -> T {
    var request = URLRequest(url: URL(string: address)!)
    request.httpMethod = method
    request.timeoutInterval = 5
    let (data, response) = try await URLSession.shared.data(for: request)
    guard (response as? HTTPURLResponse)?.statusCode == 200 else {
      throw URLError(.badServerResponse)
    }
    return try JSONDecoder().decode(T.self, from: data)
  }
}

struct InputConfiguration: Decodable {
  let device: String
  let inputURL: URL

  var isLocal: Bool {
    ["localhost", "127.0.0.1"].contains(inputURL.host ?? "") && inputURL.port == 43220
      && inputURL.scheme == "http"
  }
}

private struct SurfaceExport: Decodable {
  let surfaceID: UInt32
  let machSendResult: Int
}

/// Native bounds and window origin let the canvas translate mouse input into iOS touches.
struct ComponentInfo: Decodable {
  let width: Double
  let height: Double
  let scale: Double
  let originX: Double
  let originY: Double
  let screenWidth: Double
  let screenHeight: Double
  let layoutRevision: Int
  let hostIdentity: String
}

struct PreviewHealth: Decodable { let processID: Int }
