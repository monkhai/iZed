import Foundation
@preconcurrency import Network
import UIKit

/// HTTP carries control and metadata only. Surface handles travel over Mach.
@MainActor
final class PreviewServer {
  private let component: ComponentHost
  private var listener: NWListener?
  private var timer: Timer?
  private var rendering: Set<String> = []
  private var surfaces: [String: [String: Any]] = [:]
  private var renderedSurfaces: [String: Int] = [:]

  private var visible: Set<String>
  private var dirtyUntil: [String: CFTimeInterval] = [:]
  private var continuous = false
  private var requestCount = 0
  private var lastBatchMS = 0.0
  private var peakBatchMS = 0.0

  init(component: ComponentHost) {
    self.component = component
    visible = Set(component.ids)
    component.onChange = { [weak self] id in self?.invalidate(id: id) }
  }

  func start() throws {
    let parameters = NWParameters.tcp
    let port = UInt16(ProcessInfo.processInfo.environment["IZED_CATALOG_CONTROL_PORT"] ?? "43221") ?? 43221
    parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
    let listener = try NWListener(using: parameters)
    self.listener = listener
    listener.newConnectionHandler = { [weak self] connection in
      Task { @MainActor in self?.accept(connection) }
    }
    listener.start(queue: .main)
    print("Rndr preview endpoint: http://127.0.0.1:\(port)")
  }

  private func accept(_ connection: NWConnection) {
    connection.start(queue: .main)
    receiveRequest(connection, buffer: Data())
  }

  private func receiveRequest(_ connection: NWConnection, buffer: Data) {
    connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) {
      [weak self] data, _, done, error in
      Task { @MainActor in
        guard let self else {
          connection.cancel()
          return
        }
        var buffer = buffer
        if let data { buffer.append(data) }
        if let separator = buffer.range(of: Data("\r\n\r\n".utf8)) {
          let header = String(decoding: buffer[..<separator.lowerBound], as: UTF8.self)
          self.route(header, connection: connection)
        } else if buffer.count > 8192 || done || error != nil {
          connection.cancel()
        } else {
          self.receiveRequest(connection, buffer: buffer)
        }
      }
    }
  }

  private func route(_ header: String, connection: NWConnection) {
    let parts = header.components(separatedBy: "\r\n")[0].split(separator: " ")
    guard parts.count >= 2,
      let url = URLComponents(string: "http://localhost\(parts[1])")
    else {
      connection.cancel()
      return
    }
    requestCount += 1
    if parts[0] == "GET", url.path == "/health" {
      reply(health(), connection: connection)
      return
    }
    if parts[0] == "POST", url.path == "/render-mode" {
      let mode = url.queryItems?.first(where: { $0.name == "mode" })?.value
      guard ["continuous", "on-demand"].contains(mode) else {
        reply(["error": "Unknown render mode"], connection: connection, status: "400 Bad Request")
        return
      }
      continuous = mode == "continuous"
      timer?.invalidate()
      timer = nil
      for id in rendering { invalidate(id: id) }
      reply(health(), connection: connection)
      return
    }
    if parts[0] == "POST", url.path == "/visibility" {
      let value = url.queryItems?.first(where: { $0.name == "ids" })?.value ?? ""
      let next = Set(value.split(separator: ",").map(String.init))
      guard next.isSubset(of: Set(component.ids)) else {
        reply(
          ["error": "Unknown visible component"], connection: connection, status: "400 Bad Request")
        return
      }
      let entering = next.subtracting(visible)
      visible = next
      for id in entering { invalidate(id: id) }
      reply(health(), connection: connection)
      return
    }
    if parts[0] == "GET", url.path == "/components" {
      reply(
        [
          "processID": ProcessInfo.processInfo.processIdentifier,
          "activeCount": rendering.count, "visibleCount": visible.count,
          "renderMode": continuous ? "continuous" : "on-demand", "requestCount": requestCount,
          "lastBatchMS": lastBatchMS, "peakBatchMS": peakBatchMS,
          "catalog": catalogEntries.map { $0.dictionary },
          "components": component.ids.map { status(id: $0) },
        ], connection: connection)
      return
    }
    guard let id = url.queryItems?.first(where: { $0.name == "id" })?.value,
      component.ids.contains(id)
    else {
      reply(["error": "Unknown component ID"], connection: connection, status: "404 Not Found")
      return
    }
    switch (String(parts[0]), url.path) {
    case ("POST", "/invalidate"):
      invalidate(id: id, duration: 0.4)
      reply(component.metadata(id: id), connection: connection)
    case ("GET", "/info"):
      reply(component.metadata(id: id), connection: connection)
    case ("GET", "/surface/status"):
      reply(status(id: id), connection: connection)
    case ("POST", "/surface/start"):
      component.focus(id: id)
      rendering.insert(id)
      invalidate(id: id)
      CATransaction.flush()
      renderOne(id: id)
      reply(surfaces[id] ?? [:], connection: connection)
    case ("POST", "/focus"):
      component.focus(id: id)
      invalidate(id: id, duration: 0.6)
      reply(component.metadata(id: id), connection: connection)
    case ("POST", "/resize"):
      guard let value = url.queryItems?.first(where: { $0.name == "width" })?.value,
        let width = Double(value), width.isFinite,
        width >= 240, width <= component.maximumWidth
      else {
        reply(
          ["error": "Width must fit the host (240–600 pt). Use an iPad simulator."],
          connection: connection, status: "422 Unprocessable Content")
        return
      }
      component.resize(id: id, width: width)
      invalidate(id: id)
      reply(component.metadata(id: id), connection: connection)
    default:
      reply(["error": "Unknown preview endpoint"], connection: connection, status: "404 Not Found")
    }
  }

  private func reply(_ value: [String: Any], connection: NWConnection, status: String = "200 OK") {
    let body = (try? JSONSerialization.data(withJSONObject: value, options: .sortedKeys)) ?? Data()
    let header =
      "HTTP/1.1 \(status)\r\nContent-Type: application/json\r\nContent-Length: \(body.count)\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    connection.send(
      content: Data(header.utf8) + body, completion: .contentProcessed { _ in connection.cancel() })
  }

  private func status(id: String) -> [String: Any] {
    [
      "rendering": rendering.contains(id), "renderedSurfaces": renderedSurfaces[id, default: 0],
      "surface": surfaces[id] ?? [:], "component": component.metadata(id: id),
    ]
  }

  private func renderOne(id: String) {
    surfaces[id] = component.renderSharedSurface(id: id)
    renderedSurfaces[id, default: 0] += 1
  }

  private func health() -> [String: Any] {
    [
      "processID": ProcessInfo.processInfo.processIdentifier, "activeCount": rendering.count,
      "visibleCount": visible.count, "renderMode": continuous ? "continuous" : "on-demand",
      "requestCount": requestCount,
    ]
  }

  private func invalidate(id: String, duration: Double = 0.35) {
    dirtyUntil[id] = CACurrentMediaTime() + duration
    guard timer == nil else { return }
    let timer = Timer(timeInterval: continuous ? 1.0 / 15 : 1.0 / 60, repeats: true) {
      [weak self] _ in
      Task { @MainActor in self?.renderFrame() }
    }
    self.timer = timer
    RunLoop.main.add(timer, forMode: .common)
  }

  private func renderFrame() {
    let start = CACurrentMediaTime()
    let due = component.ids.filter {
      rendering.contains($0) && visible.contains($0) && component.selectedID == $0
        && (continuous || dirtyUntil[$0, default: 0] > start || component.isEditing(id: $0))
    }
    guard !due.isEmpty else {
      timer?.invalidate()
      timer = nil
      lastBatchMS = 0
      return
    }
    CATransaction.flush()
    for id in due { renderOne(id: id) }
    lastBatchMS = (CACurrentMediaTime() - start) * 1000
    peakBatchMS = max(peakBatchMS, lastBatchMS)
  }
}
