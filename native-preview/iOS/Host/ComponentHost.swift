import UIKit

/// One process owns every fixture. Each UIKit fixture has a native window so
/// presentations (including UIAlertController) stay inside its rendering surface.
/// GPUI switches persistent app entities in its own actual Metal window.
final class ComponentHost: UIViewController {
  var onChange: ((String) -> Void)?
  private var windows: [String: UIWindow] = [:]
  private var widths: [String: CGFloat] = [:]
  private var revisions: [String: Int] = [:]
  private(set) var selectedID = "machines"
  private var displayLink: CADisplayLink?
  var ids: [String] { catalogEntries.map(\.id) }
  var maximumWidth: CGFloat { 600 }
  private var gpuiIDs: Set<String> {
    Set(catalogEntries.filter { $0.renderer == "GPUI" }.map(\.id))
  }
  private func key(_ id: String) -> String { gpuiIDs.contains(id) ? "gpui" : id }
  private func entry(_ id: String) -> CatalogEntry { catalogEntries.first { $0.id == id }! }

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .black
    guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else { return }
    for entry in catalogEntries where entry.renderer == "UIKit" {
      let window = CatalogWindow(windowScene: scene)
      window.frame = CGRect(x: 24, y: 60, width: 390, height: entry.height)
      window.overrideUserInterfaceStyle = .dark
      window.rootViewController = terminalCatalogScreen(entry.id)
      windows[entry.id] = window
    }
    gpui_ios_register_app()
    gpui_ios_run_app()
    windows["gpui"] = scene.windows.first { window in
      guard let root = window.rootViewController else { return false }
      return String(describing: type(of: root)).contains("GPUI")
    }
    for window in windows.values { window.isHidden = true }
    displayLink = CADisplayLink(target: self, selector: #selector(frame))
    displayLink?.add(to: .main, forMode: .common)
    focus(id: "machines")
  }
  override var prefersStatusBarHidden: Bool { true }
  func resize(id: String, width: CGFloat) {
    widths[id] = width
    revisions[id, default: 0] += 1
    if selectedID == id { layoutSelected() }
  }
  func isEditing(id: String) -> Bool {
    guard id == selectedID else { return false }
    if gpuiIDs.contains(id) { return true }
    func editing(_ view: UIView) -> Bool {
      view.isFirstResponder || view.subviews.contains(where: editing)
    }
    return windows[key(id)]?.rootViewController.map { editing($0.view) } ?? false
  }
  func focus(id: String) {
    guard ids.contains(id) else { return }
    if selectedID != id { windows[key(selectedID)]?.endEditing(true) }
    selectedID = id
    for (key, window) in windows { window.isHidden = key != self.key(id) }
    if gpuiIDs.contains(id) { id.withCString { terminal_catalog_select($0) } }
    layoutSelected()
    windows[key(id)]?.makeKeyAndVisible()
    if let vc = windows[key(id)]?.rootViewController, !gpuiIDs.contains(id) {
      terminalCatalogActivate(vc, id: id)
    }
    onChange?(id)
  }
  private func layoutSelected() {
    guard let window = windows[key(selectedID)] else { return }
    window.frame = CGRect(
      x: 24, y: 60, width: widths[selectedID] ?? 390, height: CGFloat(entry(selectedID).height))
    window.rootViewController?.view.setNeedsLayout()
    window.rootViewController?.view.layoutIfNeeded()
  }
  @objc private func frame() {
    if gpuiIDs.contains(selectedID) { gpui_ios_request_frame(gpui_ios_get_window()) }
  }
  func metadata(id: String) -> [String: Any] {
    guard let window = windows[key(id)], let vc = window.rootViewController else {
      return ["id": id, "error": "Native controller unavailable"]
    }
    let e = entry(id)
    let screen = window.screen.bounds.size
    return [
      "id": id, "component": e.title, "renderer": e.renderer, "source": e.source,
      "processID": ProcessInfo.processInfo.processIdentifier,
      "width": id == selectedID ? window.bounds.width : widths[id] ?? 390,
      "height": e.height, "scale": window.screen.scale, "originX": window.frame.minX,
      "originY": window.frame.minY,
      "screenWidth": screen.width, "screenHeight": screen.height,
      "layoutRevision": revisions[id, default: 0],
      "hostIdentity": String(describing: ObjectIdentifier(vc)), "selected": selectedID,
      "touchEvents": CatalogWindow.touchEvents, "lastTouch": CatalogWindow.lastTouch,
      "isKeyWindow": window.isKeyWindow,
      "applicationState": UIApplication.shared.applicationState.rawValue,
      "state": state(id: id, controller: vc),
    ]
  }
  private func state(id: String, controller: UIViewController) -> [String: Any] {
    if gpuiIDs.contains(id), id == selectedID, let value = terminal_catalog_snapshot() {
      defer { terminal_catalog_free(value) }
      if let data = String(cString: value).data(using: .utf8),
        let state = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
      {
        return state
      }
    }
    return terminalCatalogState(controller)
  }
  func renderSharedSurface(id: String) -> [String: Any] {
    guard id == selectedID, let window = windows[key(id)] else { return ["paused": true] }
    return RndrRenderSharedSurface(window.layer, window.screen.scale, id) as? [String: Any] ?? [:]
  }
}
