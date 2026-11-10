# Emulator-only composition diagnostic

This is a dependency-free custom `android.app.Instrumentation` runner, not a
production debug endpoint. Configure `defaultConfig.testInstrumentationRunner`
as `com.monkhai.ized.platform.CompositionProbe` in the app Gradle configuration.
No AndroidX runner dependency is required.

Build (preserve the same IDE packaging properties/environment as the main APK):

```sh
./gradlew :app:assembleDebugAndroidTest -PizedIde=true -PizedServerArchitectures=arm64 --offline
```

Install **only on emulator-5554**, then start runner (normally restarts target):

```sh
adb -s emulator-5554 install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
# Save this value and restore it after the probe. Never run these on the tablet.
PREVIOUS_IME=$(adb -s emulator-5554 shell settings get secure default_input_method | tr -d '\r')
adb -s emulator-5554 shell ime enable com.monkhai.ized.test/com.monkhai.ized.platform.QuietImeService
adb -s emulator-5554 shell ime set com.monkhai.ized.test/com.monkhai.ized.platform.QuietImeService
adb -s emulator-5554 shell am instrument -w -e pauseMs 1500 \
  com.monkhai.ized.test/com.monkhai.ized.platform.CompositionProbe
```

While the runner waits, navigate the emulator app to an **empty Name field** in
Add Machine with its real input connection active. Then, from another terminal:

```sh
adb -s emulator-5554 shell run-as com.monkhai.ized touch files/ime-probe-ready
```

The runner refuses non-emulator hardware. It sends first-word composition `a`,
`ab`, finishes and inserts a space, then sends second-word composition `c`, `cd`
and commits it. Each checkpoint waits for native processing and saves a screen:

```sh
adb -s emulator-5554 exec-out run-as com.monkhai.ized \
  cat cache/ime-probe-01-first-a.png > /tmp/ime-probe-01-first-a.png
```

Repeat extraction for stages `02-first-ab`, `03-first-committed`, `04-second-c`,
`05-second-cd`, `06-second-committed`. First two screenshots must show composing
letters **before** Space. Stages 4–6 must preserve the first word and show exactly
`ab c`, `ab cd`, `ab cd`. Verify each screenshot: the temporary native input
trace has been removed from the cleaned production source.

Runner completion and local Java Editable matches are **not** proof the native
editor changed or rendered correctly. Screenshot assertions are
required. The screenshot captures only this deliberately synthetic emulator
fixture; never run the harness on the user's physical tablet. Test runner files
are excluded from the main app. Remove temporary native diagnostics after fixing
and verifying the cause, then rerun the same screen assertions on the final build.

## Isolate and restore the IME

The test APK includes a no-op `QuietImeService`. It establishes an ordinary input
connection but does not autonomously edit or finish composition. A simultaneously
bound Gboard was observed to finish each injected composition about 200ms later,
corrupting the intended test sequence (`a` then `ab` became `aab`). The runner now
refuses to run unless this test-only IME is selected. The no-op IME and runner both
reject physical hardware. No production activity, receiver or debug endpoint is
added. After the run, restore the saved emulator keyboard:

```sh
adb -s emulator-5554 shell ime set "$PREVIOUS_IME"
adb -s emulator-5554 shell ime disable com.monkhai.ized.test/com.monkhai.ized.platform.QuietImeService
```

With the no-op IME there is deliberately no onscreen keyboard. Focusing the Name
field should still establish its real InputConnection. If the runner reports no
connection, stop and inspect focus/IME binding rather than creating a synthetic
connection that bypasses the actual adapter.
