# iZed Terminal

A separate iPhone SSH terminal using iZed's GPUI iOS platform, Zed's terminal
engine (Alacritty), the same Vercel theme, and JetBrains Mono. Connections are
limited to machines reachable through the app's embedded Tailscale node. It
does not deploy a Zed server.

The app has its own bundle ID (`com.monkhai.ized-terminal`), persistent
Tailscale node identity, host settings, and device-specific Ed25519 key in
Keychain. Signing in to this app creates a separate device in the user's
tailnet; it does not reuse the login from the separate Tailscale app. The iPad
app is a separate target.

## Connect

The app opens to a searchable machine picker. The first use asks the user to
sign in to Tailscale, then lists computers from the embedded node's peer status.
The machine roster is cached on the device and refreshed on launch, when the
app returns to the foreground, or by an explicit refresh. The app does not
probe every machine when the picker opens.

Tap a new computer, confirm its SSH username, and open the terminal. Later taps
reuse the saved connection. Launch commands such as `tmux new-session -A -s phone`
are available under the optional settings. “Add manually” supports other hosts
and custom ports. New SSH identities still require fingerprint verification.

SSH access remains separate from Tailscale network access. Authorize this phone's
public key in `~/.ssh/authorized_keys` on each host; the connection screen offers
a copy button. Private keys stay in the iPhone Keychain.

### Tailscale and SSH

The embedded Tailscale client is the app's own tailnet device. Tailscale
provides network access and the peer list; SSH separately verifies each
machine's host fingerprint and authenticates with this phone's key. The app
uses TailscaleKit's loopback SOCKS5 proxy for SSH, so SSH never falls back to a
public or non-tailnet route.

Private node state stays in the app's Application Support directory, the SSH
private key stays in iOS Keychain, and peer metadata stays in the app container.
The app does not use a Tailscale admin API token or require a paired Mac.

Machine discovery is event-driven from the user's perspective: a cached roster
is shown immediately, and a new status snapshot is requested when the app opens,
returns to the foreground, or the user refreshes. There is no periodic machine
reachability sweep or background execution mode. SSH keepalives are disabled by
default. On return from suspension, the app keeps its existing Tailscale node
and tries to continue open SSH sessions. iOS or the network may have already
ended a session; reconnecting then opens a new shell. Use `tmux` when remote
work must survive a lost connection.

Tap the terminal's connection title for session and clipboard controls; use + to
open the machine picker. The key strip supplies Esc, Tab, a one-shot Ctrl
modifier, and repeating arrows. Hold a finger to select text, then copy from
the session menu.

Current scope: key authentication, SSH over the embedded tailnet, one live
session per saved connection. Password authentication, key import, jump hosts,
file transfers, and automatic session restoration are not implemented. iOS can
suspend the app in the background, so session continuity is best-effort.

## Android UI milestone

The Android host in `app/src/android_terminal.rs` opens the shared terminal
presentation from `src/presentation.rs`: session header, Machines landing screen,
buttons, and accessory keys. It uses JetBrains Mono and Vercel Dark colors.
The iOS application and its native services remain separate.

This is explicitly a **local UI preview**, not an SSH terminal yet. Browse opens
an input fixture; accessory keys record local events, Ctrl applies to the next
key, and Esc/Back returns to Machines. The session menu can reset the fixture.
No shell, machine discovery, Tailscale, credential storage, or remote connection
is initialized. No success is simulated for those unavailable services.

Build from the repository root with
`RUSTUP_TOOLCHAIN=1.94.1 ./app/build.sh android --no-run`.
Use the dedicated newer emulator for rendering checks; shader metadata and
validation remain enabled. Physical devices and iOS runtime compatibility have
not been validated by this milestone.

## Build

Requirements: Xcode 16.1 or later, Go 1.27.1 or later, Rust, and an iPhone or
iOS Simulator running iOS 18.1 or later. The TailscaleKit submodule is pinned in this repo.
The build script compiles its device and simulator frameworks from source.
Prepare the pinned Zed source with the repository's patch stack first:

```sh
../scripts/prepare-zed.sh --patch-only
git submodule update --init --recursive
rustup target add aarch64-apple-ios
cargo fetch
xcodegen generate --spec ios/project.yml
```

Open `ios/iZedTerminal.xcodeproj`, choose your signing team and iPhone, and Run.
The project builds the Rust static library automatically. Update the development
team and bundle identifier in `ios/project.yml` for another developer account.

## Verification

`ios/TerminalUITests.swift` exercises the machine picker, search, connection details,
and a real SSH connection on a physical phone. `tests/test_roster_import.py` checks
that roster import includes computers and excludes mobile devices and credentials.
`tests/ssh_fixture.py` is an optional Paramiko fixture that accepts only a supplied
public key and **never executes received commands**. It deliberately splits ANSI
and UTF-8 sequences across packets and records PTY sizes, input, and Ctrl-C.

See [tests/README.md](tests/README.md) for setup, host fingerprint verification,
and fixture limitations.

Install Paramiko into a temporary Python virtual environment, copy the public-only
`Documents/device-public-key.pub` from the app container, then start the fixture:

```sh
python tests/ssh_fixture.py --key phone.pub --events /tmp/ssh-events.jsonl
```

For the live iOS tests, separately forward the fixture machine's Tailscale port 22
to this loopback listener on port 22222. Run Xcode tests with
`IZED_TEST_HOST=smoke@FIXTURE_TAILSCALE_IP` (port 22) and
`IZED_TEST_FINGERPRINT=SHA256:...` set to the independently observed fixture
fingerprint. The tests use the normal
machine picker after live Tailscale discovery and isolate saved hosts with
`IZED_PICKER_TEST`; they do not use the automatic smoke-test connection.
Show the simulator software keyboard before the keyboard-layout test. Start a
fresh fixture for the host-verification tests and verify its printed fingerprint
before allowing trust. Review fixture events, XCTest results and screenshots.
Run only one platform simulator at a time to avoid unnecessary contention.
Stop the fixture and forwarding helper afterward; saved Tailscale node state can
be retained for later runs.
