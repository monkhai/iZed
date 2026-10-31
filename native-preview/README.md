# iZed native catalog

A live catalog of the **terminal app** in `terminal-app/`. It contains 34 screens,
states and components from production UIKit and GPUI source, rendered on iOS and
displayed in a Mac canvas using Rndr's shared GPU-surface approach.

This is separate from the older GPUI demonstration app in `app/src/screens`.
The previous browser demo remains under `web-preview/`.

## Open it

Requires Apple Silicon, full Xcode at `/Applications/Xcode.app`, a booted iPad
simulator, Rust's `aarch64-apple-ios-sim` target, and Node/npm.

Find the simulator ID:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun simctl list devices booted
```

From `native-preview/`, keep the native host/input bridge running:

```sh
zsh scripts/run-ios.sh <booted-ipad-simulator-udid>
```

In another terminal, build and open the canvas:

```sh
zsh scripts/run.sh
```

Select any entry in the sidebar. Click or type directly in the preview. The width
slider runs native layout again; it does not stretch a fixed rendering. Reconnect
reopens the surface connection and preserves fixture state. Restarting the host
restores all fixture defaults. Only the selected entry renders; hidden fixtures
keep their state.

## What's included

| Source | Screens and states | Isolated components |
| --- | --- | --- |
| UIKit | Machines, device settings, signed out, empty, refreshing, roster error, search results/no results, connection editor, advanced command, username error, copied key, sign-out, first/changed host verification | Search header, device settings form, saved/online/offline rows, section heading |
| GPUI | Landing, connected terminal, session menu, terminal options, connecting, access error, disconnected, empty session menu | Session header, empty-state body, accessory strip, control-active strip, button |

`catalog.json` is the inventory and source map. `scripts/generate-catalog.py`
generates the identical manifest for both clients and checks that source files
exist. Add an inventory entry and its fixture setup to extend the catalog.

## Production code and fixtures

- The build reads **the current** `terminal-app/ios/MachinePicker.swift`, rather
  than maintaining a copied screen implementation. Its `private` declarations
  become `fileprivate` in a generated build input solely to allow fixture setup.
  Production source and UI behavior are unchanged by that access adjustment.
- `iOS/Fixtures/TerminalFixtures.swift` supplies local data and selects actual
  controller states. Isolated views come from the production controller's
  header/footer/table-row factories. It does not redraw their controls.
- Host identity alerts use `terminal-app/ios/TrustAlert.h`, shared with the
  shipping app. The production alert construction was extracted unchanged.
- `terminal-app/src/native_catalog.rs` creates persistent `PhoneTerminal`
  entities and calls the same GPUI render implementation and shared presentation
  functions as the app. The terminal emulator parses real terminal output and
  receives local echo through in-memory channels; no shell or SSH session runs.
- The Rust harness requires the explicit `native-catalog` Cargo feature **and**
  `IZED_NATIVE_CATALOG=1`. The separate preview app supplies that environment.
  It uses its own simulator app container and stubs authentication/key services.
  Normal app builds do not activate the catalog.

## Rendering and input

```text
Actual UIKit / GPUI + fixture data (one simulator app)
    → Core Animation window layer
    → IOSurface
    → Mach surface handle
    → Metal canvas in the Mac app

Mac click/key → local ordered HID bridge → original iOS event handling
```

Each UIKit fixture has its own window inside the same app process so native
modal presentations are included. GPUI uses one real Metal-backed iOS window
and switches among persistent app entities. Resizing changes native window
bounds. HTTP transports metadata/control only; pixel buffers are shared directly.
There is no PNG/JPEG/video pipeline in the live preview.

The bridge is adapted from `/Users/yohaiwiener/Developer/rndr`, which remains
untouched. Ports `43220` (input) and `43221` (control) and the
`dev.ized.catalog.component.*` Mach namespace avoid its existing experiment.

## Verify

```sh
python3 scripts/check-catalog.py
```

Checks all 34 entries, fixture state, source mapping, native surface dimensions,
four resize paths with stable controllers, and invalid width rejection.
The last recorded run is in `docs/verification.json`. Build checks also passed
for the production Rust library without the feature and the production Objective-C
entry point after extracting the shared trust alert.

Manual canvas checks passed for UIKit search/typing, picker-to-editor navigation,
username validation, real host-verification dialogs, GPUI session menu, and text
entry through the real terminal emulator, accessory control toggling, and local
terminal disconnect/reconnect. The terminal's local echo is a fixture,
not a functional SSH connection.

## Limits

This is a local preview prototype using private Apple render-server and
CoreSimulator APIs. It needs the simulator runtime and is not a browser UIKit
interpreter or an App Store component. Shared-buffer synchronization retains
Rndr's prototype limitations. Basic physical keys/touch/drag are forwarded;
IME, clipboard shortcuts and wheel forwarding are not implemented in the Mac
input bridge. Copy buttons execute inside the isolated preview with explicitly
invalid fixture keys.

System-owned surfaces such as the software keyboard and external Tailscale
browser authorization are not separate catalog entries. UIKit fields use their
real system input behavior, but the catalog exports the selected app window,
not the separate keyboard window. Native animations run during interaction;
continuous GPUI updates are limited to the selected fixture. The canvas shows
one active entry at a time, with persistent fixture state when switching.

For a full-device keyboard review, launch the installed host on an iPhone with
`SIMCTL_CHILD_IZED_CATALOG_FULL_DEVICE=1`,
`SIMCTL_CHILD_IZED_CATALOG_INITIAL_SCENE=terminal-tools`, and
`SIMCTL_CHILD_IZED_CATALOG_SHOW_KEYBOARD=1` before `xcrun simctl launch`.
Set `SIMCTL_CHILD_IZED_CATALOG_CONTROL_PORT=43421` to keep the iPad catalog running
on its default port. Mirror that iPhone with `serve-sim <iPhone-UDID>` to include
the real system keyboard. If hardware keyboard simulation suppresses it, choose
Device Hub → Device → Keyboard → Toggle Software Keyboard. The full-device
mode uses the screen bounds; ordinary catalog windows retain their fixture size.

Clipboard actions live under the accessory bar’s **…** button. The top menu
contains active sessions and Disconnect. The session-menu fixture has three
connected local terminals and a saved, inactive computer that does not appear in
the switcher. Switching preserves the other terminals; **+** opens the real UIKit
connection picker. The header expands with an interruptible spring and respects
iOS Reduce Motion. Copy selection is available only with selected
text; Copy full output includes scrollback. Right-clicking a word also opens the
terminal options. The options panel stays above the accessory bar/keyboard inset.
