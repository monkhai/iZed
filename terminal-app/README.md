# iZed Terminal

A separate iPhone app for SSH terminals, using iZed's GPUI iOS platform,
Zed's terminal engine (Alacritty), the same Vercel theme, and JetBrains Mono.
It connects directly to ordinary SSH servers. It does not deploy a Zed server.

The app has its own bundle ID (`com.monkhai.ized-terminal`), host settings,
and device-specific Ed25519 key in Keychain. The iPad app is a separate target.

## Connect

The app opens to a searchable machine picker. Recent connections come first;
other computers from the imported Tailscale roster appear below them. It checks
SSH reachability through the Tailscale app already on the iPhone. It does not
embed another Tailscale client or require a second Tailscale login.

Tap a new computer, confirm its SSH username, and open the terminal. Later taps
reuse the saved connection. Launch commands such as `tmux new-session -A -s phone`
are available under the optional settings. “Add manually” supports other hosts
and custom ports. New SSH identities still require fingerprint verification.

SSH access remains separate from Tailscale network access. Authorize this phone's
public key in `~/.ssh/authorized_keys` on each host; the connection screen offers
a copy button. Private keys stay in the iPhone Keychain.

### Where the machine list comes from

An iOS app cannot read another app's private Tailscale device roster. This build
uses a public-metadata cache imported from a Mac's `tailscale status --json`.
No Tailscale API tokens, private keys, or account credentials are transferred.
Once the roster-source Mac is paired over SSH, the refresh button reads its
current roster over an authenticated SSH exec channel. Successful terminal
connections to that Mac also refresh the roster. If the source is unavailable,
the cached list remains usable and each phone-side SSH check remains independent.
The source Mac's view of the tailnet can differ from the phone's access rules.

To seed a development device:

```sh
/Applications/Tailscale.app/Contents/MacOS/Tailscale status --json > /tmp/tailnet-status.json
python3 scripts/import-tailnet.py /tmp/tailnet-status.json /tmp/tailnet-machines.json
```

Copy the generated metadata to `Documents/tailnet-machines.json` in this app's
container with Xcode's device tools, then relaunch. The metadata is intentionally
not bundled or committed. Refresh requires the Tailscale CLI on the source Mac.

Tap the terminal's connection title for session and clipboard controls; use + to
open the machine picker. The key strip supplies Esc, Tab, a one-shot Ctrl
modifier, and repeating arrows. Hold a finger to select text, then copy from
the session menu.

Current scope: key authentication, direct SSH, one live session per saved
connection. Password authentication, key import, jump hosts, file transfers,
and automatic session restoration are not implemented. iOS can suspend SSH
connections in the background; reconnect and attach to tmux to resume work.

## Build

Prepare the pinned Zed source with the repository's patch stack first:

```sh
../scripts/prepare-zed.sh --patch-only
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

Install Paramiko into a temporary Python virtual environment, copy the public-only
`Documents/device-public-key.pub` from the app container, then start the fixture:

```sh
python tests/ssh_fixture.py --bind YOUR_MAC_IP --key phone.pub --events /tmp/ssh-events.jsonl
```

Run Xcode tests with `IZED_TEST_HOST=smoke@YOUR_MAC_IP:22222`. The test uses a
separate host-settings file; the debug-only automatic test connection is enabled
only when both test launch environment variables are present. Review the fixture
events as well as the XCTest results and screenshots. Stop the fixture afterward.
