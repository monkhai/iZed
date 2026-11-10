package com.monkhai.ized.platform;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.Window;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.concurrent.atomic.AtomicBoolean;

/** Native GPUI confirmations; labels and answer indices are supplied by GPUI. */
public final class GpuiPrompt {
    private static final ArrayList<Dialog> dialogs = new ArrayList<>();
    private static int dp(Activity activity, int value) {
        return Math.round(value * activity.getResources().getDisplayMetrics().density);
    }
    private static GradientDrawable background(Activity activity, int color, int radius) {
        GradientDrawable background = new GradientDrawable();
        background.setColor(color);
        background.setCornerRadius(dp(activity, radius));
        return background;
    }
    public static void show(Activity activity, long id, String message, String detail,
                            String[] labels, String roles) {
        activity.runOnUiThread(() -> {
            AtomicBoolean completed = new AtomicBoolean();
            java.util.function.IntConsumer complete = answer -> {
                if (completed.compareAndSet(false, true)) nativeComplete(id, answer);
            };
            if (activity.isFinishing() || activity.isDestroyed()) { complete.accept(-1); return; }
            Dialog dialog = new Dialog(activity);
            dialog.setOwnerActivity(activity);
            dialog.requestWindowFeature(Window.FEATURE_NO_TITLE);
            dialog.setOnDismissListener(ignored -> { dialogs.remove(dialog); complete.accept(-1); });
            dialog.setOnCancelListener(ignored -> complete.accept(-1));
            dialog.setCanceledOnTouchOutside(true);
            LinearLayout panel = new LinearLayout(activity);
            panel.setOrientation(LinearLayout.VERTICAL);
            panel.setPadding(dp(activity, 20), dp(activity, 24), dp(activity, 20), dp(activity, 16));
            TextView title = new TextView(activity);
            title.setText(message); title.setTextSize(17); title.setTextColor(Color.WHITE);
            title.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
            panel.addView(title);
            if (!detail.isEmpty()) {
                TextView body = new TextView(activity);
                body.setText(detail); body.setTextSize(16); body.setTextColor(0xffa0a0a0);
                LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
                params.topMargin = dp(activity, 8); params.bottomMargin = dp(activity, 12);
                panel.addView(body, params);
            }
            for (int index = 0; index < labels.length; index++) {
                final int answer = index;
                boolean cancel = index < roles.length() && roles.charAt(index) == 'c';
                Button button = new Button(activity);
                button.setText(labels[index]); button.setAllCaps(false); button.setTextSize(17);
                button.setTextColor(cancel ? Color.WHITE : 0xff0a84ff);
                button.setTypeface(Typeface.create("sans-serif", cancel ? Typeface.BOLD : Typeface.NORMAL));
                button.setBackground(background(activity, 0xff3b3b3b, 25));
                button.setOnClickListener(view -> { complete.accept(answer); dialog.dismiss(); });
                LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
                button.setMinHeight(dp(activity, 48)); params.topMargin = dp(activity, 8);
                panel.addView(button, params);
            }
            ScrollView scroll = new ScrollView(activity); scroll.addView(panel); dialog.setContentView(scroll);
            try {
                dialogs.add(dialog);
                dialog.show();
                Window window = dialog.getWindow();
                window.setBackgroundDrawable(background(activity, 0xff222222, 32));
                window.setLayout(Math.min(dp(activity, 320), activity.getResources().getDisplayMetrics().widthPixels - dp(activity, 48)), -2);
                window.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);
                WindowManager.LayoutParams params = window.getAttributes(); params.dimAmount = 0.6f;
                window.setAttributes(params);
            } catch (RuntimeException error) {
                complete.accept(-1); dialogs.remove(dialog); dialog.dismiss();
            }
        });
    }
    public static void destroy(Activity activity) {
        for (Dialog dialog : new ArrayList<>(dialogs)) {
            if (dialog.getContext() == activity || dialog.getOwnerActivity() == activity) dialog.dismiss();
        }
    }
    private static native void nativeComplete(long id, int answer);
}
