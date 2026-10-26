import SwiftUI

@main
struct TerminalCatalogApp: App {
  var body: some Scene {
    WindowGroup("iZed · Native catalog") { WorkspaceView().frame(minWidth: 1000, minHeight: 760) }
      .defaultSize(width: 1180, height: 940)
  }
}
