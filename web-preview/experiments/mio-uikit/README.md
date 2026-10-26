# MIOFramework compatibility experiment

Result on 2026-10-24: **the existing UIKit screen did not render**. No Swift UI
was rewritten for this experiment. The browser page is a capability diagnostic,
not a port of the screen.

Target: `terminal-app/ios/MachinePicker.swift` (picker and its connection editor).
Exact input hash and line references are in `evidence/result.json`.

## What actually ran

1. Cloned the official `miolabs/MIOFramework` repository at
   `86b7c45922947c117c12bf685a40b687c78b36ba` into an isolated temporary directory.
2. Copied the actual, unmodified Swift file to its `.build/app.swift` input and
   ran the repository's `npm run buildTs`. Exit 127: the custom Swift compiler
   executable was absent. See `evidence/transpile.log`. No TypeScript screen was
   produced. The official framework and compiler GitHub release endpoints both
   returned empty release lists at test time.
3. Installed pinned published browser packages, then loaded their actual JS in
   the in-app browser. A MIO UILabel rendered successfully as a positive control.
4. Ran ten capability checks against APIs needed by our source. One passed;
   nine failed. The published UIKit bundle does not contain
   UITableViewController, UINavigationBarAppearance, NSLayoutConstraint,
   UIStackView, UIAction, layout anchors, safe/keyboard layout guides, or the
   system-name image initializer checked by this probe.
5. Also searched the newer `Libs/UIKit-ts/sources` tree in the checkout. It has
   UITableViewController, but no implementations matching NSLayoutConstraint,
   UIStackView, UINavigationBarAppearance, leadingAnchor, safeAreaLayoutGuide,
   keyboardLayoutGuide, or UIAction. This source tree was inspected, not built.
   Generated Apple API metadata elsewhere is not a runtime implementation.

The missing layout APIs are a blocker independent of the compiler installation.
The compiler documentation additionally targets a custom Swift 5-era fork,
Intel macOS build paths and iOS 12.2 SDKs. This machine is Apple Silicon with a
current Xcode SDK. Building/adapting that compiler was not attempted; it would
not supply the missing browser layout runtime by itself.

## Reproduce runtime checks

```sh
cd web-preview/experiments/mio-uikit
npm ci --ignore-scripts
npm run serve
```

Open <http://127.0.0.1:18492/>. It loads local package assets and makes no external
service calls. `probe.js` maps the screen's required APIs to runtime checks.
The small UILabel control is deliberately the only rendered UIKit-like control;
we do not present an HTML recreation as a successfully transpiled Swift screen.

## Reproduce the failed transpiler invocation

In a fresh clone of <https://github.com/miolabs/MIOFramework>, make `.build/`,
copy the actual `MachinePicker.swift` to `.build/app.swift`, then run
`npm run buildTs`. The compiler must first be supplied at the path configured
by the upstream package script for this to get past the observed failure.

## Conclusion

MIO proves that implementing UIKit-like APIs in a browser is possible. These
versions do not provide an off-the-shelf route for our existing screen. Running
it would require a working Swift transpiler and significant UIKit/layout
coverage work. The actual app screen has **not** been executed in this test.

Sources: [framework](https://github.com/miolabs/MIOFramework),
[compiler instructions](https://github.com/miolabs/MIOFramework/blob/master/MIOSwiftTranspiler/README.md).
MIO runtime packages are Apache-2.0 and retain their licenses in `node_modules`.
