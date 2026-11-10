# Android tablet regression report — 2026-11-10

## Outcome

Elad completed the physical Samsung Galaxy Tab S7 FE workflow and reported, “everything seems to work now,” after the final rendering retest. He authorized cleanup, commit and push. This accepts the exercised workflow, not every editor feature or device. The code and this verification report are prepared for the authorized commit and push.

Elad exclusively controlled the physical tablet. Assistant-side work was limited to inspection, builds, installation and authorized Mac support. Emulator automation was separate and is not presented as physical-device evidence.

## Verified physical workflow

- Buttons and typing worked in the user's retest.
- The app connected through embedded Tailscale and SSH to the Mac.
- A remote-file edit containing `print(99)` was independently verified on the Mac.
- Elad ran the file in the remote terminal and reported output `99`.
- The user accepted the final grayscale/rendering retest. Earlier cropped terminal evidence showed missing prompt glyphs; the editor crop was intact. The earlier full-image impression was **not evidence of global text loss**.
- A hardware-keyboard interruption affected other apps too, and Android listed no physical keyboard. It was not established as an iZed-specific input failure.

## Targeted simulator regressions

The final test configuration was a genuine Pixel Tablet AVD with native 2560×1600 resolution and hardware keyboard enabled, using Android Emulator 37.2.12 and `-gpu lavapipe`. Tests used densities 320 and 380; constrained-layout checks included dark mode and full Gboard. An earlier phone AVD with overrides was inadequate tablet evidence and was superseded. The original AVD was preserved.

| Area | Verified result |
| --- | --- |
| Navigation | Initial picker dismissal leaves usable Home; Open Remote reaches the picker. Home remains usable after discarding a test buffer. |
| Tab closing | Visible touch X opens Save / Don't Save / Cancel for an unsaved test file. |
| Keyboard labels | Ctrl/Alt labels appear instead of Apple modifier glyphs. |
| Hardware input | First name-field letter appears without a space. Editor hardware input produced the intended two lines after the Java input-focus correction. |
| Palette Enter | The reproduced Gboard newline-instead-of-confirm bug was corrected; repeating Enter opened the selected picker. |
| Composition | The first correction failed on the second word. The later mark-relative correction passed all six stages, including `ab cd` before final commit with five UTF-16 code units. |
| Keyboard-visible layout | Form footer stays above the IME. Directory list scrolls within its bounds; typing a fixture path and tapping Open This Folder with full keyboard visible opens the remote project. |
| Remote edit/run | Simulator remote open, Vim `:w` save and Python execution passed; host file contents were independently checked. Ctrl-S did not save in the tested Vim state and is not recorded as a pass. |
| Single-window handling | Exact lifecycle helper tests pass, and native build/launch pass. Duplicate-wrapper rejection was not exercised at runtime. |

The nested directory regression was tested with approximately 630 physical pixels of upper content remaining above Gboard. Samsung-specific composition timing was not independently reproduced in that emulator.

## Close Remote Project semantics

The control uses workspace removal without closing Android's sole window. Serializable unsaved editor buffers are retained for hot exit and restored on reopen; this does **not** imply that remote files were saved. Nonserializable dirty items use the existing prompt/cancel path. Earlier assumptions that every dirty close would prompt were disproved by runtime testing and corrected. Tab closing is a separate, tested Save / Don't Save / Cancel path.

## Cleanup verified

- Remote Login OFF.
- Original Administrators-only access list restored.
- Full-disk access for remote users OFF.
- Both temporary emulator/tablet SSH keys removed.
- Test folder moved to Trash.
- Gboard restored and temporary diagnostic IME disabled during emulator cleanup.

Temporary action/input/render diagnostics were removed. Emulator and disposable-folder remote servers are stopped; the Gradle daemon was stopped. Disposable captures/probes were moved to Trash. SDKs, AVDs and reusable build caches remain. The accepted app on the physical tablet was left untouched.

## iOS and verification limits

Android UI/input behavior is conditionally scoped to Android, including compact picker layout, minimum-width constraint, SSH validation wording and full-chain error display. The older SSH timeout/archive-guard changes were also gated to Android, preserving the original iOS transport behavior. The entire uncommitted port still includes shared build changes, notably vendored ARM FP16 annotations. Therefore ‘no iOS source changed’ is not accurate. The final iOS simulator-target compile passed; it does not replace a physical iPhone/iPad runtime test.

Broader agent authentication, debugger, Git and language-server workflows were not accepted by this tablet test. Prior November 4 results remain documented separately in `android-ide-test-scope.md` and `android-ide-port.md`.

## Local evidence index

Local evidence captures are not bundled in this document. Useful basenames: `ized-after-discard.png`, `ized-unsaved-close.png`, `ized-name-first-key.png`, `ized-hardware3-editor.png`, `ized-composition-green.log`, `ized-composition-green.png`, `ized-dir-scroll-crop.png`, `ized-diag-project-small.png`, `ized-remote-open.png`, and `ized-remote-run.png`. Public-key material, account paths, tailnet addresses and device identifiers are deliberately omitted.

## Final source/artifact checks

- Cache-invalidated Android native build: pass.
- Final Gradle debug APK assembly, APK signature and embedded macOS ARM64 server archive check: pass.
- Cache-invalidated `aarch64-apple-ios-sim` compile check: pass.
- Two isolated single-window lifecycle tests: pass.
- Android patch forward/reverse applicability and whitespace checks: pass.
- Cleaned APK SHA-256: `53cae4c73dd09d3909120660270e58d14f5dc5670397a87bc96afdebd3a2fd91`.

The diagnostic-free APK was built and checked, not reinstalled or runtime-retested on the physical tablet. Physical acceptance refers to the preceding candidate; final changes remove diagnostics and isolate behavior from iOS.
