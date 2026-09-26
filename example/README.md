# iZed application host

This crate contains the iPad app that hosts Zed's workspace on the GPUI Mobile
iOS platform. Start with the [repository README](../README.md) for the current
feature list and physical iPad build instructions.

- `src/ized.rs` contains the Machine picker and remote workspace setup.
- `ios/project.yml` generates the Xcode project with XcodeGen.
- `ios/Assets.xcassets` contains the preview app icon and launch artwork.
- `ios/RemoteServers/` receives locally built server archives from
  `../scripts/prepare-zed.sh`; generated archives are ignored by Git.

The Xcode project, scheme, and app display name are iZed. The GPUI platform
APIs retain their upstream names.
