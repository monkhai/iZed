import Foundation

/// One configuration fetch, one heartbeat and one visibility update for the whole canvas.
@MainActor
final class PreviewRuntime: ObservableObject {
  static let shared = PreviewRuntime()
  @Published var continuous = false {
    didSet {
      guard continuous != oldValue else { return }
      Task { try? await PreviewClient(componentID: "card-1").setMode(continuous: continuous) }
    }
  }
  private var configurationTask: Task<InputConfiguration, Error>?
  private var heartbeat: Task<Void, Never>?
  private var visibilityTask: Task<Void, Never>?
  private var failures: [String: () -> Void] = [:]
  private var visible: Set<String> = []
  private var hostPID: Int?

  func configuration() async throws -> InputConfiguration {
    if configurationTask == nil {
      configurationTask = Task {
        try await PreviewClient(componentID: "card-1").inputConfiguration()
      }
    }
    do { return try await configurationTask!.value } catch {
      configurationTask = nil
      throw error
    }
  }

  func attach(_ id: String, onFailure: @escaping () -> Void) {
    failures[id] = onFailure
    guard heartbeat == nil else { return }
    heartbeat = Task { [weak self] in
      do {
        while !Task.isCancelled {
          try await Task.sleep(for: .seconds(1))
          guard let self else { return }
          let health = try await PreviewClient(componentID: "card-1").health()
          if let pid = hostPID, pid != health.processID { throw URLError(.networkConnectionLost) }
          hostPID = health.processID
        }
      } catch {
        if !Task.isCancelled { self?.fail() }
      }
    }
  }

  func detach(_ id: String) {
    failures.removeValue(forKey: id)
    setVisible(id, false)
    if failures.isEmpty {
      heartbeat?.cancel()
      heartbeat = nil
      configurationTask = nil
      hostPID = nil
    }
  }

  func setVisible(_ id: String, _ value: Bool) {
    let changed = value ? visible.insert(id).inserted : visible.remove(id) != nil
    guard changed else { return }
    visibilityTask?.cancel()
    visibilityTask = Task { [weak self] in
      do {
        try await Task.sleep(for: .milliseconds(80))
        guard let self else { return }
        try await PreviewClient(componentID: "card-1").setVisible(ids: visible.sorted())
      } catch {
        if !Task.isCancelled { self?.fail() }
      }
    }
  }

  private func fail() {
    let callbacks = Array(failures.values)
    configurationTask = nil
    for callback in callbacks { callback() }
  }
}
