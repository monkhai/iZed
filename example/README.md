# iZed application host

This crate contains the iPad app that hosts Zed's workspace on the GPUI Mobile
iOS platform. Start with the [repository README](../README.md) for the current
feature list and physical iPad build instructions.

- `src/editor_spike.rs` contains the Machine picker and remote workspace setup.
- `ios/project.yml` generates the Xcode project with XcodeGen.
- `ios/Assets.xcassets` contains the preview app icon and launch artwork.
- `ios/RemoteServers/` receives locally built server archives from
  `../scripts/prepare-zed.sh`; generated archives are ignored by Git.

The Xcode scheme retains the inherited `GpuiExample` name for now. The app's
display name is iZed.
