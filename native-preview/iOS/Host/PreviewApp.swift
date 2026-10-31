import UIKit

@main
final class PreviewApp: UIResponder, UIApplicationDelegate {
  func application(
    _ application: UIApplication, configurationForConnecting session: UISceneSession,
    options: UIScene.ConnectionOptions
  ) -> UISceneConfiguration {
    let config = UISceneConfiguration(name: "Terminal Catalog", sessionRole: session.role)
    config.delegateClass = CatalogScene.self
    return config
  }
}
final class CatalogScene: UIResponder, UIWindowSceneDelegate {
  var window: UIWindow?
  private var server: PreviewServer?
  func scene(
    _ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions
  ) {
    guard let scene = scene as? UIWindowScene else { return }
    setenv("IZED_NATIVE_CATALOG", "1", 1)
    let window = CatalogWindow(windowScene: scene)
    let host = ComponentHost()
    self.window = window
    window.rootViewController = host
    window.makeKeyAndVisible()
    host.loadViewIfNeeded()
    host.focus(id: ProcessInfo.processInfo.environment["IZED_CATALOG_INITIAL_SCENE"] ?? "machines")
    if ProcessInfo.processInfo.environment["IZED_CATALOG_SHOW_KEYBOARD"] == "1" {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
        gpui_ios_show_keyboard(gpui_ios_get_window())
      }
    }
    let server = PreviewServer(component: host)
    self.server = server
    do { try server.start() } catch { print("Catalog server: \(error)") }
  }
  func sceneDidBecomeActive(_ scene: UIScene) { gpui_ios_did_become_active(nil) }
  func sceneWillResignActive(_ scene: UIScene) { gpui_ios_will_resign_active(nil) }
}

final class CatalogWindow: UIWindow {
  static var touchEvents = 0
  static var lastTouch = ""
  override func sendEvent(_ event: UIEvent) {
    if let touch = event.allTouches?.first {
      Self.touchEvents += 1
      Self.lastTouch = "\(touch.location(in:self)) \(touch.phase.rawValue)"
    }
    super.sendEvent(event)
  }
}
