# Live IDE test host and verification scope

## Current verified state — November 4

The approved real-Mac test through embedded Tailscale has run successfully: helper provisioning, scratch-file editing and saving (independently checked on the host), reopening after app/emulator restart, terminal first-character input, Ctrl-C, PTY resize, and background/resume. The actual scratch file is `/tmp/ized-ide-smoke-20261104/round-trip.txt`; its saved marker is `ANDROID_SAVE_CONFIRMED`.

Temporary access was restored: Remote Login off, full-disk access off, original Administrators account selection restored, added authorized key removed (original file absent), and test helper processes stopped. Tailscale remains installed as requested. The custom execution fixture below was never implemented or started.

Final Android native/APK builds and archive-integrity checks pass after removing temporary key diagnostics. The existing focused keyboard test binary passes 25 tests; its tested keyboard source is unchanged. Live integration results above were obtained on the immediately preceding functional build, before diagnostic removal. Final iOS simulator-target compile passed (8m26s). The diagnostic-free APK installed and launched successfully; the complete synthetic search input rendered correctly with the software keyboard visible. Live iOS integration was not rerun. See [visual review](android-ide-visual-review.md) for the limits of the screenshot comparison. Agent authentication, debugger, Git, and language-server workflows have not been validated.

## Historical planning notes

The following records the earlier proposed scope; its statements about pending authorization or tests do not describe the current state above.

Status: alternative design on hold; the execution-capable listener has not been implemented or started. A new custom fixture is not inherently required. Prefer an existing authorized macOS SSH test host and scratch project if one is available. The original README describes physical-iPad testing against a real Mac; the checked-in UI tests do not provision an SSH host. Runtime approval is required before starting this alternative service.

## Preferred path: existing authorized Mac

Use a real macOS SSH host, following the original iOS path, rather than building a new protocol simulator. Identify the host and account, verify its host fingerprint independently, and authorize only the existing Android app public key if necessary. The current Android transport uses the embedded Tailscale stream, so the selected host must also have a confirmed reachable route through that transport. Existing Beam forwarding was configured for a nonexecuting loopback fixture; that alone does not establish access to a real SSH service.

If this development Mac is selected, enabling Remote Login, restricting its allowed accounts, and adding the public key require explicit approval. The resulting SSH access is persistent and has the selected account's permissions; using a scratch project does not sandbox those permissions. No such changes have been made. A temporary executing listener remains an alternative requiring its own approval, not an automatic fallback.

## First end-to-end acceptance test

1. Use a synthetic plain-text project with a known seed file; avoid language-server or account setup for this first test.
2. Check the real host fingerprint, exercise rejection, then accept the independently verified identity.
3. Authenticate with the existing Android identity and let the shared flow provision the matching ARM remote helper.
4. Browse to the scratch project, open `round-trip.txt`, append `ANDROID_EDIT_SAVED` through the actual editor, and save through its UI/shortcut.
5. Independently read the host file and compare exact contents. Reopen it in the app to verify the remote state.
6. Open the real terminal in that project, print a synthetic marker, interrupt a bounded running command with Ctrl+C, and check resize.
7. Background/resume and reopen the saved workspace; verify saved contents and identity persistence without exposing private keys.
8. Capture equivalent iOS/Android states sequentially, with only one simulator running. Stop only test-owned remote processes afterward; do not disable or alter an existing shared SSH service.

The local seed project is `<test-root>/ide-smoke-project`. Creating it does not authorize remote access or execution. Broader project-tree, search, diagnostics, Git, debugger and agent acceptance remains separate from this initial vertical slice.

## Alternative fixture (not implemented or approved)

Beam's `terminal-app/tests/ssh_fixture.py` verifies real SSH authentication and terminal traffic but deliberately never executes commands. Preserve it unchanged. A separate IDE fixture can reuse its Paramiko authentication, ephemeral host-key fingerprint and event-recording approach.

## Connection and identity

- Bind only `127.0.0.1:22222` on the Mac, never a LAN interface.
- Reuse the existing saved userspace Tailscale test helper's virtual port 22 forwarding to that listener. Other tailnet peers could reach the endpoint subject to existing policy, but only the approved Android app's public key is accepted. No tailnet policy or system Remote Login changes.
- Read only the app's existing public key, not its private identity. Generate a fresh in-memory test host key and compare its public fingerprint before accepting it in the app. No new account, password, or client credential is required.

## Execution scope

Use an explicit dispatch table for the app's bootstrap operations: platform query, fixture home query, directory listing/check restricted to the fixture tree, server archive upload, digest verification and installation inside that tree. Do not pass arbitrary received bootstrap strings to a shell.

Allow the pinned, locally built remote helper only with its expected `proxy --identifier` arguments (and optional `--reconnect`). For terminal verification, allow the app's exact terminal-opening request for the test project, starting one clean interactive shell in a PTY. That shell genuinely executes typed commands; this is necessary to verify the IDE terminal rather than scripted output. Reject SSH forwarding, SFTP and other exec requests. Additional task/debugger commands should be explicitly listed when their test is prepared.

Use `<test-root>/ide-live-fixture` for the test home, uploaded server, project, logs and server state. Supply a minimal environment and use synthetic project files. Do not inherit personal authentication environment variables or shell startup files. Disable automatic language-server installation for the initial plain-text editing test.

This directory and entry-point allowlist are **not an OS sandbox**. The Zed server's file APIs and the interactive shell run with the current macOS user's permissions and could access other files or start processes. Restricting bootstrap commands does not remove that capability. Tests will operate only on the synthetic project; stronger confinement would require a separate, explicitly approved environment.

## Lifetime and cleanup

Run only for the approved test session, with no launch agent, login service or automatic restart. Track owned child process groups and the Zed server's PID/state. On completion or failure stop the listener, forwarding helper, terminal and remote server, then verify no owned service remains. Preserve only test evidence and explicitly retained fixture files; never clear the emulator's identity or enrollment. Run one simulator at a time.
