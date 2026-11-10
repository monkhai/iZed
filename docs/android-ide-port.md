# Android iZed workspace port

The `android-iZed` branch starts at `5016d473587931e1c36486c896dcc6f6a852e0f0`. Beam's reviewed branch and PR are separate. The full IDE uses `app/src/ized.rs` and Zed's existing GPUI workspace; `--features ized` selects it on Android, while the existing default still selects Beam.

## Platform boundaries

- Shared: machine/project flow, Zed protocol, editor, workspace, project tree, terminal model, debugger and agent UI.
- Android: existing Beam encrypted SSH key and userspace Tailscale stream supplied to the mobile Zed transport through two narrow hooks; application-private paths and lazy extraction of matching server archives.
- GPUI platform: system clipboard, browser intents, encrypted service credentials, and asynchronous editor IME integration. No JNI calls are added inside editor rendering or text shaping.
- Dependency changes are an additional `patches/android-ide.patch` layered on the existing iOS patch stack. Apply with `python3 scripts/prepare-android-ide.py /path/to/isolated/pinned/zed`. Development uses an isolated Cargo checkout so the reviewed branch's dependency source is unchanged.

## Acceptance against iOS

- Pair and verify a machine, browse folders, provision the matching server and reopen a saved remote workspace.
- Open/edit/save files; project tree, tabs/splits, search, diagnostics, Git views and command palette follow the existing iOS implementation.
- Software IME composition/selection/deletion, Unicode clipboard, hardware shortcuts, keyboard resizing and focus work without duplicate input.
- Remote terminal, tasks, debugger and agent commands execute on the selected machine; reconnect and resume preserve the intended session behavior.
- Account login opens the browser, credentials persist securely and sign-out removes them. Any new account authorization remains user-controlled.
- Compare actual iOS/Android screenshots and responsiveness at comparable logical sizes. Retain GPUI and existing rendering paths; native system fonts/keyboards may differ.

## Build and verification status

Implementation is in progress. Java credential/asset/IME/prompt helper compilation, the Android platform/default Beam compile check, and the full Android IDE compile check passed. NDK r26 failed in WebRTC C++ pointer-traits code; approved NDK `28.1.13356709` was installed alongside r26 and the full check then passed. The SDK manager used the existing accepted SDK license without a new acceptance prompt. Native-library code generation found missing function-level FP16 annotations in pinned `gemm-common` 0.17.1 and 0.18.2. Local copies add only those annotations to four helpers per version; existing runtime CPU dispatch is retained and no global FP16 requirement is enabled. The app lockfile changes those two source entries and adds direct edges to the already-pinned JNI 0.21 and certificate-verifier dependencies. Android initializes the verifier and packages its matching AAR, retaining system certificate checks. The full native Android IDE build passed, including a second build after the final input changes. The complete Gradle Java host compilation also passed offline. The Apple-silicon remote server build passed with GPUI's supported runtime-shader feature, avoiding an uninstalled Xcode Metal compiler; The APK packages and launches successfully. The first launch exposed a missing native home directory; the Java host now supplies its app-private home before loading Rust. Visible machine-filter hardware input, Ctrl+A/Backspace, a real software-keyboard tap, device-settings navigation and saved Tailscale readiness passed. Dark/light rendering and the unsubmitted Add Machine form were captured. The full iOS IDE simulator-target compile check passed after the final phone/input changes; live iOS regression tests and live remote IDE tests have not run. Compile success does not establish runtime parity.

The original iOS README explicitly supports Apple-silicon and Intel Macs; its Xcode resources include `RemoteServers`, and its transport selects the archive matching `uname -m`. There is no shipped iOS app artifact here to verify its contents. The first Android development build will target this Apple-silicon Mac, so no Intel Rust target is needed or installed.

Build the Rust library with `cargo ndk -t arm64-v8a --platform 31 build --manifest-path app/Cargo.toml --lib --features ized --locked -j 2`, using the isolated patched Cargo source and NDK28.1. Build only this Mac's server via `scripts/build-remote-servers.sh /path/to/isolated/zed --native-only`. Package with `-PizedIde=true -PizedServerArchitectures=arm64`. Default server builds and distribution packaging still include both architectures. Missing required assets fail packaging; connecting to an architecture omitted from a development build produces an explicit unsupported-build message.

Run only one simulator at a time. Preserve existing application identities; do not wipe emulator data. No publication of this branch is authorized yet.

Development APK copies use NDK `llvm-strip --strip-unneeded` to reduce emulator storage; the original native artifacts and symbols remain in the isolated target directory. Dynamic exports are preserved for JNI. Gradle excludes the unused `libized_platform.so` dependency cdylib because the app statically links that crate. Use a clean Gradle package after reducing native sizes so incremental ZIP padding does not retain the previous larger APK size. Beam's existing scripted SSH fixture never executes commands, so it cannot validate IDE provisioning or file editing. Starting a real executing remote-server fixture requires a separate, concrete runtime approval.

UI smoke evidence is in the task-local `ide-evidence` directory. Full remote editing, terminal execution/resize, reconnect, debugger/agent flows and matched iOS screenshot comparison remain unverified. See `android-ide-test-scope.md` for the preferred existing-host test path and the unapproved alternative fixture scope.

The original fixed two-column Add Machine form clipped at phone width. Android windows narrower than 700 logical pixels now navigate the same picker states one panel at a time; wider windows and iOS retain the original layout. The final 1080×2400/density420 device check passed Add Machine → visible form → Back with the software keyboard present. Gesture duration now uses Android input timestamps rather than processing time, preventing delayed rendering from turning a short tap into a long press; cancelled touches remain cancelled. Original phone dimensions, density and light mode were verified across restart before shutdown. The last tablet capture caught the splash and is not valid parity evidence. The original full-IDE XCTest file contains coordinate/screenshot actions, with no SSH fixture provisioning or saved-file-content assertion; do not equate the earlier Beam 2/2 result with full IDE coverage.

## Resume checkpoint — 2026-11-04

The isolated worktree remains on `android-iZed` at `5016d473587931e1c36486c896dcc6f6a852e0f0`, with implementation changes uncommitted. The original worktree still contains only its three previously unrelated dirty Cargo files. No commit, publication or terminal-PR merge is authorized for this unit.

Preserved final-build evidence: `/tmp/ized-ide-phone-build5.log` (Android native build), `/tmp/ized-ide-phone-apk5.log` (APK), and `/tmp/ized-ide-ios-check3.log` (iOS compile check). These are the October 31 results, not new November 4 runtime tests. The preserved APK SHA256 remains `31bf492c0ad3e08e3653e28c5c09d5e4bbad9880892f5d9c0654197691d2ae9f`. `git diff --check` passed on resume. No simulator was running at the read-only resume check.

No local TCP/22 listener or loaded `com.openssh.sshd` service was observed at the initial resume check.

### Real Mac test — November 4 (in progress)

The user subsequently approved a real Mac test through Tailscale, temporary account-only Remote Login with full-disk access off, and temporary authorization of the Android app's existing public key. The first session verified the independent SSH host fingerprint, back-out/retry, public-key authentication, helper provisioning, remote project/file loading and terminal startup. The installed helper matched the packaged binary byte-for-byte. A synthetic project in `/tmp` avoids requiring access to macOS-protected Documents. Saving the edited buffer has **not passed**; repeated reconnects and workspace input/layout defects are under diagnosis. These results do not establish full IDE parity.

Android asset packaging expanded the `.gz` helper and removed its suffix. The Android asset now has an opaque `.gz.bin` name and is restored to `.gz` during extraction. `scripts/check-android-server-assets.py` fails against the old APK and passes against the corrected APK by comparing exact archive hashes.

The SSH workspace was using the finite-command 30-second idle timeout. Its connection now uses a bounded startup timeout and keepalives. Changes inside the isolated Git dependency require explicit target-specific Cargo cache invalidation before rebuilding; ordinary incremental builds may reuse the old crate. Keyboard translation's 25 tests pass, including control characters not being treated as text. Runtime save and idle-stability verification remain pending. Temporary `[DEBUG-ide-key]` diagnostics must be removed after diagnosis.

After an execution disconnect, the stopped build/emulator and remaining host helper processes were reconciled: the test key was removed, helper processes stopped, and TCP/22 verified closed before restarting the build. The local access ledger records current temporary-access state and the original settings; it is not a repository artifact.

### November 4 restart checkpoint (paused after requested cleanup)

- Recovered native build and rebuilt APK successfully; server gzip integrity check passed.
- Reverted an incorrect content-rect addition to touch coordinates. Real taps now select the saved Mac, scratch project and file at their displayed positions.
- Workspace safe-area padding visibly keeps content below the Android status bar and above navigation/IME. Closing the project dock via its actual control provides usable portrait editor width; automatic compact-pane behavior remains undecided/unimplemented.
- Real Tailscale SSH authentication and scratch-file opening passed. The same proxy/server PIDs remained alive for more than two minutes, beyond the earlier reconnect interval; no spontaneous reconnect occurred during that observation. This is bounded evidence, not a long-run stability claim.
- Edit marker appeared in the editor but did not persist on the Mac. Targeted logs showed Escape DOWN being consumed with the IME open (only UP reached native code), leaving Vim insertion mode where Ctrl-S is signature help.
- Added `GpuiTextInput.onKeyPreIme` routing Escape through existing native input. APK build/install passed; runtime verification remains pending. The first reopen after installation reported that the saved Tailscale connection was not ready yet. Test was stopped at the requested checkpoint before retry.
- Temporary DEBUG-ide-key diagnostics remain intentionally for the next verification and must be removed before final acceptance.
- Verified cleanup: Remote Login OFF, full-disk access OFF, original Administrators-only allow list restored, test public key removed (authorized_keys absent as originally), owned helper processes stopped, emulator shut down. Tailscale remains installed as requested.
- Pending: validate Escape fix, save and independently verify host contents/reopen, terminal Ctrl-C/resize, resume, final visual and affected iOS regression checks. No commit/push/merge.

### November 4 save/terminal retry checkpoint

The reported crash was an Android ANR after typing a scratch marker, Escape, then Ctrl-S. The native ANR stack resolved to `AndroidPlatform::primary_window` called from the queued `nativeKey` callback inside `flush_main_thread_tasks`. That method held the platform state mutex while invoking callbacks. It now clones the dispatcher before invoking callbacks, releasing the state lock. Added a regression test asserting queued callbacks can acquire that lock; the Android-specific unit test has not yet been executed independently. Native build, APK packaging and archive-integrity verification passed. Runtime Escape no longer hung and delivered both key actions.

Passed on the real Mac through Tailscale:
- Typed `ANDROID_SAVE_CONFIRMED`, Escape, Ctrl-S; independent exact host-file assertion passed.
- Terminal command output, Ctrl-C interrupt of a verified `sleep 30` process, and remote PTY resize from 14x42 to 22x44.
- Background/resume followed by a successful `echo RESUME_OK`; same helper processes remained connected for over five minutes.
- Full app restart reconnected to the saved workspace after a manual retry while Tailscale became ready. File contents after reopening the file itself remain unverified at the requested stop checkpoint.

Outstanding: one first-character loss when switching into terminal input (subsequent focused commands passed); improve/verify this before full acceptance. Initial connection attempts before saved Tailscale readiness still require retry. Remove temporary key diagnostics, run final affected checks including iOS, verify saved-file reopen and complete visual acceptance. No full IDE pass claimed.

Cleanup independently verified again: TCP22 refused, authorized_keys absent as originally, no owned helper processes, Remote Login/full-disk access OFF, original Administrators allow list restored, emulator stopped. No commit/push/merge.

### November 4 reopen and first-key verification

Saved-file reopening passed after restarting both app and emulator: the visible editor contained `ANDROID_SAVE_CONFIRMED` and an independent exact host-content assertion passed.

First-character loss was reproduced and traced: `GpuiTextInput.dispatchKeyEvent` received the first `e` DOWN with `enabled=false`, then native GPUI received only UP. The proxy can retain focus during a transition to a terminal with an empty IME snapshot. It now forwards ordinary hardware keys even when its IME state is disabled; system keys retain normal handling. The fresh-terminal retry received both DOWN and UP, executed the complete `echo FIRST_KEY_FIXED` command and displayed `FIRST_KEY_FIXED` on the first attempt.

The APK update initially failed for insufficient emulator storage. Excluding the unneeded `libized_platform.so` and stripping unneeded symbol tables from only the device copy (full original retained on Mac, dynamic exports verified identical) reduced a clean APK from 657 MiB to 423 MiB. Installation and launch then passed without deleting app data. Gradle incremental APK padding required a clean packaging pass to realize the reduction.

Startup behavior: early requests before saved Tailscale readiness require retry; later retries passed. The UI now preserves the full error chain, so the readiness cause is available rather than only the host address. The updated error presentation was not observed during this final successful first-attempt launch.

At requested pause, temporary key and helper processes removed, Remote Login/full-disk access OFF, Administrators-only baseline restored, TCP22 refused, emulator stopped. Remaining: remove DEBUG-ide-key diagnostics, build/install final diagnostic-free artifact, rerun affected Android checks and iOS compile regression, review final visual parity. No full final regression pass or publication claimed.

### November 4 final verification

Temporary key diagnostics removed. Final Android native build, Gradle APK packaging, archive integrity, patch applicability, and whitespace checks pass. Existing focused keyboard test binary passes 25/25 on unchanged keyboard source. Final iOS IDE simulator-target compile passes (one pre-existing dead-code warning). The final APK installs, launches, and renders complete synthetic hardware-key input with the software keyboard visible; data and saved enrollment are preserved. APK SHA256: `923cb433f3b0b070962ae05aecae948f4fc6965dd2aa8118d687b40dff6d6775`.

The real-Tailscale integration results above precede removal of diagnostics; that full integration sequence was not repeated afterward. Live iOS integration and broader agent/Git/debugger/language-server workflows remain unverified. [Visual review](android-ide-visual-review.md) records shared styling and corrected insets, but no matched-viewport pixel-parity claim. The phone project dock remains wide when open. Early Tailscale startup may require manual retry. Temporary SSH access remains removed and the test emulator is stopped. No commit, push or merge performed.
