# Android IDE visual review — November 4

Reference: the repository's actual iPad workspace capture, [ized-ipad-workspace.png](ized-ipad-workspace.png), together with the shared `app/src/ized.rs` presentation and settings. Android evidence is retained locally in the task's `ide-evidence` directory (`ized-file-reopened.png`, `ized-first-key-fixed.png`, and terminal resize/resume captures).

## Findings

| Area | Result |
| --- | --- |
| Rendering path | Shared Zed GPUI editor, project tree, tabs, dock and terminal; no replacement Android widget hierarchy for the IDE. |
| Fonts and theme | Both platforms load the same bundled JetBrains fonts and Vercel theme/settings. Android dark captures visually retain the reference's black surfaces, muted borders, monospaced text and dock controls. |
| Workspace arrangement | Project tree remains on the right, editor in the center and terminal below, as in the iPad reference. The reference also has an authenticated agent pane; that state was not reproduced or authorized for this test. |
| Android system bars and keyboard | Workspace padding follows actual Android insets. Tested content no longer overlaps the status/navigation bars; the terminal/editor viewport resizes above the keyboard. |
| Phone picker/form | Android uses one picker panel at a time below 700 logical pixels to avoid the original two-column form clipping. Wider windows and iOS retain the original arrangement. |
| Phone workspace | The project dock takes most of the available width when open. Its visible toggle restores a readable full-width editor. Automatic compact workspace/dock behavior is not implemented. |
| Pixel equivalence | Not established: the reference is a landscape iPad with a different project, viewport and agent state; the verified Android captures are a portrait phone. These cannot support a pixel-identical or full visual-parity claim. |

The review confirms reuse and the corrected Android inset behavior, not complete visual acceptance at matching tablet dimensions. A matched iPad/Android capture with equivalent viewport, project and panel state remains necessary for that stronger claim. Native keyboards/system UI necessarily differ. No proprietary Apple fonts were added.
