// swift-tools-version: 6.0
import PackageDescription

// The Mac viewer is a Swift package. scripts/build-ios.sh builds the simulator host.
let package = Package(
  name: "Rndr",
  platforms: [.macOS(.v14)],
  targets: [
    .target(
      name: "RndrGPUBridge",
      path: "macOS/SurfaceReceiver",
      publicHeadersPath: "include",
      cSettings: [.unsafeFlags(["-fobjc-arc"])],
      linkerSettings: [.linkedFramework("Foundation"), .linkedFramework("IOSurface")]
    ),
    .executableTarget(name: "Rndr", dependencies: ["RndrGPUBridge"], path: "macOS/App"),
  ]
)
