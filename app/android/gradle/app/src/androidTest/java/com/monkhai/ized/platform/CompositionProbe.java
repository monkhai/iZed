package com.monkhai.ized.platform;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.Bitmap;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.provider.Settings;
import android.text.Editable;
import android.view.inputmethod.InputConnection;
import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.Field;

/** Emulator-only diagnostic runner. Never packaged into the production APK.
 * Exercises the real Java InputConnection and native asynchronous IME adapter.
 * Local Editable contents are only a checkpoint, NOT proof of native rendering;
 * compare each PNG against its expected visible editor state.
 */
public final class CompositionProbe extends Instrumentation {
    private Object proxy;
    private InputConnection connection;
    private Field bufferField;
    private long pauseMs = 1500;

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        if (arguments != null) pauseMs = Math.max(500, Math.min(10000,
                Long.parseLong(arguments.getString("pauseMs", "1500"))));
        start();
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        File ready = null;
        try {
            // Fail closed on the user's physical tablet, even if invoked there accidentally.
            if (!("ranchu".equals(Build.HARDWARE) || "goldfish".equals(Build.HARDWARE)))
                throw new IllegalStateException("CompositionProbe is emulator-only");
            String selectedIme = Settings.Secure.getString(
                    getTargetContext().getContentResolver(), Settings.Secure.DEFAULT_INPUT_METHOD);
            if (selectedIme == null || !selectedIme.equals(getContext().getPackageName()
                    + "/com.monkhai.ized.platform.QuietImeService"))
                throw new IllegalStateException("Select the test-only QuietImeService first; real keyboards interfere with controlled composition");
            ready = new File(getTargetContext().getFilesDir(), "ime-probe-ready");
            if (ready.exists() && !ready.delete())
                throw new IllegalStateException("Could not clear previous probe gate");
            Intent launch = new Intent();
            launch.setClassName(getTargetContext().getPackageName(),
                    "com.monkhai.ized.platform.GpuiActivity");
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivitySync(launch);
            status("WAITING: navigate emulator to empty Name, then create files/ime-probe-ready");
            long deadline = SystemClock.uptimeMillis() + 180000;
            while (!ready.exists() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(200);
            if (!ready.exists()) throw new IllegalStateException("Timed out waiting for ready gate");
            if (!ready.delete()) throw new IllegalStateException("Could not consume ready gate");

            Class<?> adapter = getTargetContext().getClassLoader()
                    .loadClass("com.monkhai.ized.platform.GpuiTextInput");
            Field current = adapter.getDeclaredField("current"); current.setAccessible(true);
            Field active = adapter.getDeclaredField("enabled"); active.setAccessible(true);
            Field input = adapter.getDeclaredField("connection"); input.setAccessible(true);
            bufferField = adapter.getDeclaredField("buffer"); bufferField.setAccessible(true);
            final Throwable[] failure = {null};
            runOnMainSync(() -> {
                try {
                    proxy = current.get(null);
                    if (proxy == null || !active.getBoolean(proxy))
                        throw new IllegalStateException("No focused GPUI text input");
                    connection = (InputConnection) input.get(proxy);
                    if (connection == null) throw new IllegalStateException("No real InputConnection");
                    if (((Editable) bufferField.get(proxy)).length() != 0)
                        throw new IllegalStateException("Focused field must be empty before probe");
                } catch (Throwable error) { failure[0] = error; }
            });
            if (failure[0] != null) throw new IllegalStateException("Probe setup failed", failure[0]);

            stage("01-first-a", "a", () -> connection.setComposingText("a", 1));
            stage("02-first-ab", "ab", () -> connection.setComposingText("ab", 1));
            stage("03-first-committed", "ab ", () -> {
                connection.finishComposingText();
                return connection.commitText(" ", 1);
            });
            stage("04-second-c", "ab c", () -> connection.setComposingText("c", 1));
            stage("05-second-cd", "ab cd", () -> connection.setComposingText("cd", 1));
            stage("06-second-committed", "ab cd", () -> connection.commitText("cd", 1));
            result.putString("stream", "Composition sequence finished. Inspect cache/ime-probe-*.png and native post-edit logs; this is NOT a native-rendering pass assertion.");
            finish(Activity.RESULT_OK, result);
        } catch (Throwable error) {
            result.putString("stream", "CompositionProbe failed: " + error.toString());
            finish(Activity.RESULT_CANCELED, result);
        } finally {
            if (ready != null && ready.exists()) ready.delete();
        }
    }

    private interface Edit { boolean apply(); }

    private void stage(String name, String expected, Edit edit) throws Exception {
        final boolean[] accepted = {false};
        runOnMainSync(() -> accepted[0] = edit.apply());
        if (!accepted[0]) throw new IllegalStateException(name + " InputConnection rejected edit");
        SystemClock.sleep(pauseMs); // native queue, GPUI update and screenshot checkpoint
        final boolean[] localMatches = {false};
        final int[] localLength = {-1};
        runOnMainSync(() -> {
            try {
                Editable value = (Editable) bufferField.get(proxy);
                localLength[0] = value.length();
                localMatches[0] = expected.contentEquals(value);
            } catch (IllegalAccessException error) { throw new RuntimeException(error); }
        });
        Bitmap screenshot = getUiAutomation().takeScreenshot();
        if (screenshot == null) throw new IllegalStateException("No screenshot at " + name);
        File target = new File(getTargetContext().getCacheDir(), "ime-probe-" + name + ".png");
        try (FileOutputStream stream = new FileOutputStream(target)) {
            if (!screenshot.compress(Bitmap.CompressFormat.PNG, 100, stream))
                throw new IllegalStateException("Could not encode screenshot at " + name);
        } finally { screenshot.recycle(); }
        status(name + " localLength=" + localLength[0] + " localMatches=" + localMatches[0]
                + "; inspect native log and " + target.getName());
    }

    private void status(String message) {
        Bundle status = new Bundle();
        status.putString("stream", message + "\n");
        sendStatus(0, status);
    }
}
