import Foundation

/// Small stress-test recorder. The benchmark reads this local JSON once per second.
@MainActor
final class ViewerMetrics: ObservableObject {
  static let shared = ViewerMetrics()
  @Published private(set) var summary = "Connecting native surface…"
  private var live: Set<String> = []
  private var submitted: [String: Int] = [:]
  private var completed: [String: Int] = [:]
  private var surfaces: [String: UInt32] = [:]
  private var received: [String: Int] = [:]
  private var previousCompleted = 0
  private var previousTime = Date().timeIntervalSince1970
  private var timer: Timer?

  private init() {
    timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
      Task { @MainActor in self?.snapshot() }
    }
  }

  func opened(_ id: String) { live.insert(id) }
  func closed(_ id: String) {
    live.remove(id)
    surfaces.removeValue(forKey: id)
  }
  func submitted(_ id: String, surface: UInt32, handles: Int) {
    submitted[id, default: 0] += 1
    surfaces[id] = surface
    received[id] = handles
  }
  func completed(_ id: String) { completed[id, default: 0] += 1 }

  private func snapshot() {
    let now = Date().timeIntervalSince1970
    let total = completed.values.reduce(0, +)
    let rate = Double(total - previousCompleted) / max(0.001, now - previousTime)
    summary =
      "\(live.count) Metal views · \(surfaces.count) surfaces received · \(Int(rate)) GPU completions/sec total"
    let data: [String: Any] = [
      "timestamp": now, "processID": ProcessInfo.processInfo.processIdentifier,
      "liveIDs": live.sorted(), "submitted": submitted, "completed": completed,
      "surfaces": surfaces, "receivedHandles": received,
    ]
    if let json = try? JSONSerialization.data(withJSONObject: data, options: [.sortedKeys]) {
      try? json.write(
        to: FileManager.default.temporaryDirectory.appendingPathComponent(
          "ized-catalog-metrics.json"), options: .atomic)
    }
    previousCompleted = total
    previousTime = now
  }
}
