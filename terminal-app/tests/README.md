# SSH smoke fixture

`ssh_fixture.py` performs real SSH authentication and channel traffic, but its
terminal responses are scripted: Enter returns `PHONE_SSH_OK`. It never executes
received commands or starts a real shell/PTY process.

From `terminal-app`, install Paramiko in an isolated environment, then run:

```sh
python3 -m venv /tmp/ized-ssh-fixture-venv
/tmp/ized-ssh-fixture-venv/bin/python -m pip install paramiko
/tmp/ized-ssh-fixture-venv/bin/python tests/ssh_fixture.py \
  --key /path/to/device-public-key.pub --events /tmp/ized-ssh-events.jsonl
```

Supply only the test client's OpenSSH public key. The default listener is
`127.0.0.1:22222`; `--bind` and `--port` can override it. Binding beyond loopback
exposes the server on that interface. Tailscale routing is separate; loopback
alone does not make it reachable by the production iZed client.

Compare the printed SHA-256 host fingerprint with the client's trust prompt.
The existing host key is generated in memory on each start, so restarting changes
its fingerprint. Never blindly accept a changed-host warning.

The event file records authentication results, PTY/resize requests, input and
Ctrl-C. Use synthetic input only; logs persist until you remove them. Stop with
Ctrl-C. Do not commit keys, logs or virtual environments.

The fixture echoes bytes, handles CRLF as one submission and ignores Backspace
on empty input. It is not a full Unicode editor or normal shell. ANSI/UTF-8 banner
writes are split deliberately, but SSH/TCP may coalesce them.

## Android live smoke test

Build the embedded library with `bash app/android/build-tailscale.sh`, then build
`ized-app` using cargo-ndk and assemble its existing Gradle app. This reuses the
production Rust SSH transport and terminal renderer. The Android host requires
its app-local SSH identity and an interactive Tailscale sign-in before testing.
Use a dedicated emulator; the script refuses physical-device serials.

After runtime approval, read only `no_backup/device-public-key.pub` with
`adb -s emulator-5554 exec-out run-as com.monkhai.ized cat ...` and use that public
key with the fixture above. Reuse the approved userspace Tailscale helper that
forwards tailnet port 22 to the loopback fixture. Start a fresh fixture process
for each test run so host verification is exercised. Compare its printed host
fingerprint before supplying it to the test; the script refuses a mismatch.

```sh
python3 terminal-app/tests/android_smoke.py \
  --serial emulator-5554 --address smoke@TAILNET_IP --machine ized-ssh-fixture \
  --fingerprint SHA256:EXACT_FIXTURE_FINGERPRINT \
  --events /path/to/fixture-events.jsonl --output /path/to/test-evidence
```

The test restarts only the test app, checks public-key persistence, rejects then
accepts the verified host, and checks real SSH authentication, split ANSI/UTF-8,
synthetic input, accessory and hardware Ctrl-C, remote PTY resize on keyboard
hide/show, and background/foreground resume. It saves
screenshots and a JSON result. It does not approve login, alter tailnet policy,
read private keys, reset the emulator, or configure a personal device. Stop the
fixture and forwarding helper after testing. The fixture never executes commands.

The verified local renderer setup uses Android Emulator 37.2.12 with the
session-only `-gpu lavapipe` option. Emulator 35.2.10 with its SwiftShader Vulkan
backend stalled in `vkCreateGraphicsPipelines` before app initialization. Use an
already-installed compatible engine without wiping the AVD or changing security
settings. Android debug builds must enable `rust-embed/debug-embed`, as iOS does,
so default settings are embedded rather than read from build-machine paths.
