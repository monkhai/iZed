package com.monkhai.ized.platform;

import android.app.Activity;
import android.content.Context;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.text.InputType;
import android.view.View;
import android.view.ViewGroup;
import android.view.KeyEvent;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.widget.FrameLayout;

/** GPUI editor IME adapter. All View/Editable access stays on the UI thread.
 * Native edits are queued, not executed synchronously by the input connection.
 */
public final class GpuiTextInput extends View {
    private static GpuiTextInput current;
    private final Editable buffer = new SpannableStringBuilder();
    private int offset;
    private int absoluteStart, absoluteEnd;
    private boolean uncachedSelection;
    private long serial;
    private boolean enabled;
    private boolean applyingSnapshot;
    private BaseInputConnection connection;

    private GpuiTextInput(Activity activity) {
        super(activity);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        Selection.setSelection(buffer, 0);
    }

    private InputMethodManager imm() {
        return (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
    }

    public static void update(Activity activity, boolean enabled, String text, int offset,
                              int start, int end, int markedStart, int markedEnd, long acknowledged) {
        activity.runOnUiThread(() -> {
            if (activity.isDestroyed()) return;
            if (current == null || current.getContext() != activity) {
                destroy();
                current = new GpuiTextInput(activity);
                activity.addContentView(current, new FrameLayout.LayoutParams(1, 1));
            }
            GpuiTextInput view = current;
            if (enabled && acknowledged < view.serial) return; // A newer local edit is still queued.
            boolean changedFocus = view.enabled != enabled;
            view.enabled = enabled;
            view.offset = offset;
            view.absoluteStart = start;
            view.absoluteEnd = end;
            view.uncachedSelection = Math.min(start, end) < offset || Math.max(start, end) > offset + text.length();
            view.buffer.replace(0, view.buffer.length(), text);
            BaseInputConnection.removeComposingSpans(view.buffer);
            int a = Math.max(0, Math.min(text.length(), start - offset));
            int b = Math.max(0, Math.min(text.length(), end - offset));
            Selection.setSelection(view.buffer, a, b);
            if (view.connection != null && markedStart >= offset && markedEnd <= offset + text.length()) {
                view.applyingSnapshot = true;
                view.connection.setComposingRegion(markedStart - offset, markedEnd - offset);
                view.applyingSnapshot = false;
            }
            if (enabled) {
                view.requestFocus();
                if (changedFocus) view.imm().restartInput(view);
                view.imm().updateSelection(view, start, end, markedStart, markedEnd);
            } else if (changedFocus) {
                // Keep the key-routing proxy focused in command modes (e.g.
                // Vim Normal), but tell Android there is no text connection.
                // Clearing focus leaves a stale IME consuming printable DOWNs.
                view.requestFocus();
                view.imm().restartInput(view);
                view.imm().hideSoftInputFromWindow(view.getWindowToken(), 0);
            }
        });
    }

    public static void show(Activity activity) {
        activity.runOnUiThread(() -> {
            if (current != null && current.enabled) {
                current.requestFocus();
                current.imm().showSoftInput(current, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    public static void destroy() {
        if (current != null) {
            if (current.getParent() instanceof ViewGroup) ((ViewGroup) current.getParent()).removeView(current);
            current = null;
        }
    }

    @Override public boolean onKeyPreIme(int keyCode, KeyEvent event) {
        // Physical keys belong to GPUI's keymap (including Vim's printable
        // commands). Letting the IME see them first converts keys such as 'i'
        // into committed text, bypassing commands and delaying hardware typing.
        // Software composition still enters through InputConnection below.
        boolean hardwareKey = (event.getFlags() & KeyEvent.FLAG_SOFT_KEYBOARD) == 0;
        if (!event.isSystem() && hardwareKey
                && (event.getAction() == KeyEvent.ACTION_DOWN || event.getAction() == KeyEvent.ACTION_UP)) {
            nativeKey(keyCode, event.getAction(), event.getMetaState(), event.getUnicodeChar());
            return true;
        }
        return super.onKeyPreIme(keyCode, event);
    }

    @Override public boolean dispatchKeyEvent(KeyEvent event) {
        // The proxy can retain focus briefly when moving to a terminal whose
        // IME snapshot is empty. Hardware keys still belong to GPUI.
        if (event.isSystem()) return super.dispatchKeyEvent(event);
        if (event.getAction() == KeyEvent.ACTION_MULTIPLE && event.getCharacters() != null)
            return connection != null && connection.commitText(event.getCharacters(), 1);
        if (event.getAction() != KeyEvent.ACTION_DOWN && event.getAction() != KeyEvent.ACTION_UP)
            return super.dispatchKeyEvent(event);
        nativeKey(event.getKeyCode(), event.getAction(), event.getMetaState(), event.getUnicodeChar());
        return true;
    }

    @Override public boolean onCheckIsTextEditor() { return enabled; }

    @Override public InputConnection onCreateInputConnection(EditorInfo info) {
        if (!enabled) return null;
        info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        info.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_NONE;
        info.initialSelStart = absoluteStart;
        info.initialSelEnd = absoluteEnd;
        connection = new BaseInputConnection(this, true) {
            @Override public Editable getEditable() { return buffer; }

            // A missing selection endpoint is not a selection at the cache edge.
            @Override public CharSequence getSelectedText(int flags) {
                return uncachedSelection ? null : super.getSelectedText(flags);
            }
            @Override public CharSequence getTextBeforeCursor(int length, int flags) {
                return uncachedSelection ? "" : super.getTextBeforeCursor(length, flags);
            }
            @Override public CharSequence getTextAfterCursor(int length, int flags) {
                return uncachedSelection ? "" : super.getTextAfterCursor(length, flags);
            }

            private void changed(String before) {
                String after = buffer.toString();
                int left = 0;
                while (left < before.length() && left < after.length() && before.charAt(left) == after.charAt(left)) left++;
                // Do not split a UTF-16 surrogate pair at the replacement boundary.
                if (left > 0 && left < before.length() && Character.isLowSurrogate(before.charAt(left))) left--;
                int oldEnd = before.length(), newEnd = after.length();
                while (oldEnd > left && newEnd > left && before.charAt(oldEnd - 1) == after.charAt(newEnd - 1)) {
                    oldEnd--; newEnd--;
                }
                if (oldEnd < before.length() && oldEnd > left && Character.isLowSurrogate(before.charAt(oldEnd))) {
                    oldEnd++; newEnd++;
                }
                nativeEdit(0, offset + left, offset + oldEnd,
                        after.substring(left, newEnd), -1, -1, ++serial);
            }

            private int replacementStart() {
                int marked = getComposingSpanStart(buffer);
                return marked >= 0 ? marked : Math.min(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
            }
            private int replacementEnd() {
                int marked = getComposingSpanEnd(buffer);
                return marked >= 0 ? marked : Math.max(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
            }
            @Override public boolean commitText(CharSequence text, int cursor) {
                if (!enabled) return false;
                // Some IMEs send Enter as a newline commit. Dispatch it as a
                // key so palettes confirm and documents insert a newline.
                // Multiline paste remains a text replacement.
                if ("\n".contentEquals(text)) {
                    finishComposingText();
                    return performEditorAction(EditorInfo.IME_ACTION_NONE);
                }
                if (uncachedSelection) {
                    nativeEdit(4, 0, 0, text.toString(), cursor, 0, ++serial);
                    return true;
                }
                int a = replacementStart(), b = replacementEnd();
                boolean result = super.commitText(text, cursor);
                nativeEdit(0, offset + a, offset + b, text.toString(),
                        cursor == 1 ? -1 : offset + Selection.getSelectionStart(buffer),
                        cursor == 1 ? -1 : offset + Selection.getSelectionEnd(buffer), ++serial);
                return result;
            }
            @Override public boolean setComposingText(CharSequence text, int cursor) {
                if (!enabled) return false;
                if (uncachedSelection) {
                    nativeEdit(5, 0, 0, text.toString(), cursor, 0, ++serial);
                    return true;
                }
                int a = replacementStart(), b = replacementEnd();
                boolean result = super.setComposingText(text, cursor);
                nativeEdit(1, offset + a, offset + b, text.toString(),
                        Selection.getSelectionStart(buffer) - a, Selection.getSelectionEnd(buffer) - a, ++serial);
                int caret = Selection.getSelectionEnd(buffer);
                if (caret < a || caret > a + text.length())
                    nativeEdit(3, offset + Selection.getSelectionStart(buffer), offset + caret, "", -1, -1, ++serial);
                return result;
            }
            @Override public boolean setComposingRegion(int start, int end) {
                if (!applyingSnapshot) {
                    start -= offset;
                    end -= offset;
                    if (start < 0 || end < 0 || start > buffer.length() || end > buffer.length()) return false;
                }
                boolean result = super.setComposingRegion(start, end);
                if (!applyingSnapshot) {
                    int a = getComposingSpanStart(buffer), b = getComposingSpanEnd(buffer);
                    if (a >= 0 && b >= a) nativeEdit(1, offset + a, offset + b,
                            buffer.subSequence(a, b).toString(), Selection.getSelectionStart(buffer) - a,
                            Selection.getSelectionEnd(buffer) - a, ++serial);
                }
                return result;
            }
            @Override public boolean finishComposingText() {
                boolean result = super.finishComposingText();
                nativeEdit(2, 0, 0, "", 0, 0, ++serial);
                return result;
            }
            @Override public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                if (!enabled || beforeLength < 0 || afterLength < 0) return false;
                int localStart = Math.min(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                int localEnd = Math.max(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                if (uncachedSelection || beforeLength > localStart || afterLength > buffer.length() - localEnd) {
                    uncachedSelection = true; // Keep following edits native until the acknowledgment.
                    nativeEdit(6, beforeLength, afterLength, "", -1, -1, ++serial);
                    return true;
                }
                String before = buffer.toString();
                boolean result = super.deleteSurroundingText(beforeLength, afterLength);
                changed(before);
                return result;
            }
            @Override public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
                if (!enabled || beforeLength < 0 || afterLength < 0) return false;
                int localStart = Math.min(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                int localEnd = Math.max(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                if (uncachedSelection || beforeLength > Character.codePointCount(buffer, 0, localStart)
                        || afterLength > Character.codePointCount(buffer, localEnd, buffer.length())) {
                    // Bound native code-point context queries as well.
                    if (beforeLength > 4095 || afterLength > 4095) return false;
                    uncachedSelection = true;
                    nativeEdit(7, beforeLength, afterLength, "", -1, -1, ++serial);
                    return true;
                }
                int a = Math.min(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                int b = Math.max(Selection.getSelectionStart(buffer), Selection.getSelectionEnd(buffer));
                int left = Character.offsetByCodePoints(buffer, a, -Math.min(beforeLength, Character.codePointCount(buffer, 0, a)));
                int right = Character.offsetByCodePoints(buffer, b, Math.min(afterLength, Character.codePointCount(buffer, b, buffer.length())));
                return deleteSurroundingText(a - left, right - b);
            }
            @Override public boolean setSelection(int start, int end) {
                if (!enabled || start < 0 || end < 0) return false;
                absoluteStart = start;
                absoluteEnd = end;
                uncachedSelection = Math.min(start, end) < offset || Math.max(start, end) > offset + buffer.length();
                if (!uncachedSelection) super.setSelection(start - offset, end - offset);
                if (!applyingSnapshot) nativeEdit(3, start, end, "", -1, -1, ++serial);
                return true;
            }
            @Override public boolean performEditorAction(int action) {
                nativeKey(KeyEvent.KEYCODE_ENTER, KeyEvent.ACTION_DOWN, 0, '\n');
                nativeKey(KeyEvent.KEYCODE_ENTER, KeyEvent.ACTION_UP, 0, '\n');
                return true;
            }
            @Override public boolean sendKeyEvent(KeyEvent event) {
                return GpuiTextInput.this.dispatchKeyEvent(event);
            }
        };
        return connection;
    }

    private static native void nativeKey(int code, int action, int modifiers, int unicode);

    private static native void nativeEdit(int mode, int start, int end, String text,
                                          int selectionStart, int selectionEnd, long serial);
}
