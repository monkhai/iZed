package com.monkhai.ized.platform;

import android.inputmethodservice.InputMethodService;
import android.os.Build;
import android.view.inputmethod.EditorInfo;

/** Test APK only. Binds an InputConnection but never modifies composition.
 * CompositionProbe drives that real connection; no keyboard prediction engine
 * is allowed to finish or replace the controlled composition between stages.
 */
public final class QuietImeService extends InputMethodService {
    @Override public void onCreate() {
        super.onCreate();
        if (!("ranchu".equals(Build.HARDWARE) || "goldfish".equals(Build.HARDWARE)))
            throw new IllegalStateException("QuietImeService is emulator-only");
    }
    @Override public void onStartInput(EditorInfo attribute, boolean restarting) {
        // Deliberately no calls into the editor. The framework still establishes
        // the real input connection which CompositionProbe exercises.
    }
    @Override public void onFinishInput() {
        // Do not finish composition implicitly; each probe stage controls it.
    }
    @Override public boolean onEvaluateInputViewShown() { return false; }
    @Override public boolean onEvaluateFullscreenMode() { return false; }
}
