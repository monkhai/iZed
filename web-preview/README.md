# Terminal app browser demo

A GPUI/WebAssembly workspace for exploring the current app's screens and flow.
Use **Scenes** to jump between 16 states, or follow Browse → sign in → authorize →
choose a machine → configure SSH → verify host → terminal. Each scene has a
shareable `?scene=…` URL.

## Run

Requirements: Rust nightly with `rust-src` and `wasm32-unknown-unknown`, and
[Trunk](https://trunkrs.dev/). The toolchain file selects the Rust requirements.
A sibling `zed/` checkout must contain `gpui_web` and `gpui_platform`. The tested
revision is `f8f0a28508393b2620d2a7cead470d9cc11fdf4d`.

```sh
cd web-preview
trunk serve
```

Open <http://127.0.0.1:18491/>. Trunk watches the preview, shared presentation
module, and theme. It supplies the isolation headers for threaded WebAssembly.

## Rendering boundary

The session header, empty landing screen, buttons, and accessory key strip come
from `terminal-app/src/presentation.rs`, also imported by the production iOS app.
The preview loads the existing Vercel Dark theme and JetBrains Mono font.

UIKit machine picker, connection editor, authentication, and system dialogs are
interactive GPUI representations based on the existing Swift screens. They are
labeled in the workspace; UIKit itself is not executing in this browser. The
keyboard is also a GPUI demo. Phone dimensions and safe areas are preview values.

Authentication, machine discovery, host verification, saved sessions, errors,
and SSH connections use local fixtures. Nothing contacts Tailscale or a host.
The terminal is a small simulated command loop, not the production terminal
emulator. Try `help`, `ls`, `pwd`, `whoami`, `date`, `echo …`, `clear`, or `exit`.
Copy-key actions copy an explicitly invalid placeholder. State resets on reload.

Scenes include landing, sign-in, authorization, machines, empty results, refresh
error, connection editor, first host verification, changed host identity,
connecting, terminal, session menu, keyboard, connection error, disconnected,
and sign-out confirmation.

## Compatibility and attribution

The iOS app stays pinned to Zed `5688167d224b5eca54875d49afb8bfd73a07915a`.
The browser harness uses the sibling checkout's newer browser backend. The
shared presentation module uses APIs common to both.

`src/text_input.rs` adapts Zed's GPUI input example, under Apache-2.0; its license
is included as `LICENSE-APACHE-GPUI`. The field supports real GPUI editing,
selection, clipboard, and IME handling.

## Validation

The browser build and iOS Rust compile check passed on 2026-10-24:

```sh
trunk build
cd ../terminal-app
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
    cargo check --locked --target aarch64-apple-ios-sim
```

Browser interaction checks covered authorization, machine filtering, username
validation, host verification, connection completion, command entry, and session
menu/keyboard controls. This is not a native runtime or UIKit fidelity test.
