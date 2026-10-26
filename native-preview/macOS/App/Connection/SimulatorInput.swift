import AppKit

/// One ordered input queue. Native injection is acknowledged before the next event.
@MainActor
final class SimulatorInput {
  static let shared = SimulatorInput()
  private var inputURL: URL?
  private var pending: Task<Void, Never>?
  private var failures: [String: () -> Void] = [:]

  private init() {}

  func attach(id: String, url: URL, onError: @escaping () -> Void) {
    failures[id] = onError
    inputURL = url
  }

  func release(id: String) {
    failures.removeValue(forKey: id)
    guard failures.isEmpty else { return }
    pending?.cancel()
    pending = nil
    inputURL = nil
  }

  private func fail() {
    let callbacks = Array(failures.values)
    for callback in callbacks { callback() }
  }

  func touch(
    _ type: String, componentID: String, x: Double, y: Double, width: Double, height: Double
  ) {
    send(
      tag: 3, value: ["type": type, "x": x, "y": y, "width": width, "height": height],
      focusID: type == "begin" ? componentID : nil, invalidateID: componentID)
  }

  func key(_ event: NSEvent) -> Bool {
    guard !event.modifierFlags.contains(.command), !event.modifierFlags.contains(.control),
      let usage = Self.keyUsages[event.keyCode]
    else { return false }
    let shift = event.modifierFlags.contains(.shift)
    if shift { send(tag: 6, value: ["type": "down", "usage": 225]) }
    send(tag: 6, value: ["type": "down", "usage": usage])
    send(tag: 6, value: ["type": "up", "usage": usage])
    if shift { send(tag: 6, value: ["type": "up", "usage": 225]) }
    return true
  }

  private func send(
    tag: UInt8, value: [String: Any], focusID: String? = nil, invalidateID: String? = nil
  ) {
    guard let json = try? JSONSerialization.data(withJSONObject: value) else { return }
    var packet = Data([tag])
    packet.append(json)
    // Preserve down/up ordering without blocking AppKit's event loop.
    let previous = pending
    pending = Task { [weak self] in
      await previous?.value
      guard !Task.isCancelled, let self, let url = self.inputURL else { return }
      do {
        if let focusID {
          try await PreviewClient(componentID: focusID).focus()
        } else if let invalidateID {
          try await PreviewClient(componentID: invalidateID).invalidate()
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = packet
        request.timeoutInterval = 5
        let (_, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 204 else {
          throw URLError(.badServerResponse)
        }
      } catch { if !Task.isCancelled { self.fail() } }
    }
  }

  // Physical ANSI keyboard -> USB HID usage. IME and clipboard are not supported yet.
  private static let keyUsages: [UInt16: Int] = [
    0: 4, 1: 22, 2: 7, 3: 9, 4: 11, 5: 10, 6: 29, 7: 27, 8: 6, 9: 25, 11: 5,
    12: 20, 13: 26, 14: 8, 15: 21, 16: 28, 17: 23, 18: 30, 19: 31, 20: 32,
    21: 33, 22: 35, 23: 34, 24: 46, 25: 38, 26: 36, 27: 45, 28: 37, 29: 39,
    30: 48, 31: 18, 32: 24, 33: 47, 34: 12, 35: 19, 36: 40, 37: 15, 38: 13,
    39: 52, 40: 14, 41: 51, 42: 49, 43: 54, 44: 56, 45: 17, 46: 16, 47: 55,
    48: 43, 49: 44, 50: 53, 51: 42, 53: 41, 123: 80, 124: 79, 125: 81, 126: 82,
  ]
}
