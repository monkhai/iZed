package com.monkhai.ized.platform;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.Dialog;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.*;
import org.json.JSONArray;
import org.json.JSONObject;

/** Native machine selection and trust UI; SSH and terminal state live in Rust. */
public final class TerminalUi {
    private static Dialog picker;
    private static Dialog trust;
    private static Activity owner;
    private static JSONObject model = new JSONObject();
    private static LinearLayout rows;
    private static TextView status;
    private static EditText search;
    private static View screen;
    private static View inputView;
    private static int lastKeyboardHeight;
    private static LinearLayout footer;
    private static Dialog deviceSettings;
    private static LinearLayout settingsForm;
    private static View refreshControl;
    private static final int BG = 0xff0a0a0a, SURFACE = 0xff111111, BORDER = 0xff2e2e2e,
            TEXT = 0xffededed, MUTED = 0xffa0a0a0, GREEN = 0xff00ac3a;
    private static int dp(Context c, float value) { return Math.round(value * c.getResources().getDisplayMetrics().density); }
    private static android.graphics.drawable.GradientDrawable shape(Context c, int color, float radius, boolean border) {
        android.graphics.drawable.GradientDrawable d = new android.graphics.drawable.GradientDrawable();
        d.setColor(color); d.setCornerRadius(dp(c, radius));
        if (border) d.setStroke(dp(c, 1), BORDER);
        return d;
    }
    private static void font(TextView v, int size, boolean medium) {
        v.setTypeface(android.graphics.Typeface.createFromAsset(v.getContext().getAssets(),
                medium ? "JetBrainsMono-Medium.ttf" : "JetBrainsMono-Regular.ttf"));
        v.setTextSize(size); v.setIncludeFontPadding(false); v.setTextColor(TEXT);
    }
    private static LinearLayout.LayoutParams size(Context c, int height) {
        return new LinearLayout.LayoutParams(-1, dp(c, height));
    }
    private static void gap(LinearLayout parent, int height) {
        parent.addView(new View(parent.getContext()), size(parent.getContext(), height));
    }
    private static void present(Dialog dialog, View content) {
        dialog.setContentView(content); dialog.show();
        dialog.getWindow().setBackgroundDrawableResource(android.R.color.transparent);
        dialog.getWindow().setLayout(-1, -1);
        dialog.getWindow().setStatusBarColor(BG); dialog.getWindow().setNavigationBarColor(BG);
        dialog.getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
    }
    private static View navigation(Activity a, String title, Runnable back, Runnable refresh) {
        FrameLayout bar = new FrameLayout(a); bar.setBackgroundColor(BG);
        TextView name = label(a, title); font(name, 17, true); name.setGravity(android.view.Gravity.CENTER);
        bar.addView(name, new FrameLayout.LayoutParams(-1, dp(a, 60)));
        boolean groupedActions=title.equals("Machines")&&refresh!=null;
        if(groupedActions) {
            View capsule=new View(a);capsule.setBackground(shape(a,0xff272727,22,true));
            FrameLayout.LayoutParams params=new FrameLayout.LayoutParams(dp(a,104),dp(a,44),android.view.Gravity.RIGHT|android.view.Gravity.CENTER_VERTICAL);
            params.rightMargin=dp(a,16);bar.addView(capsule,params);
        }
        if (back != null) {
            boolean close = title.equals("Machines");
            View icon = new View(a) {
                protected void onDraw(android.graphics.Canvas canvas) {
                    android.graphics.Paint paint=new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
                    canvas.save();canvas.scale(getWidth()/44f,getHeight()/44f);
                    paint.setColor(0xff272727);canvas.drawCircle(22,22,21,paint);
                    paint.setStyle(android.graphics.Paint.Style.STROKE);paint.setStrokeWidth(1);paint.setColor(BORDER);
                    canvas.drawCircle(22,22,21,paint);paint.setColor(TEXT);paint.setStrokeWidth(2);
                    paint.setStrokeCap(android.graphics.Paint.Cap.ROUND);paint.setStrokeJoin(android.graphics.Paint.Join.ROUND);
                    if(close){canvas.drawLine(16,16,28,28,paint);canvas.drawLine(28,16,16,28,paint);}
                    else {android.graphics.Path path=new android.graphics.Path();path.moveTo(25,14);path.lineTo(17,22);path.lineTo(25,30);canvas.drawPath(path,paint);}
                    canvas.restore();
                }
            };
            icon.setContentDescription(close?"Close machines":"Machines");icon.setOnClickListener(v->back.run());
            FrameLayout.LayoutParams params=new FrameLayout.LayoutParams(dp(a,44),dp(a,44),android.view.Gravity.LEFT|android.view.Gravity.CENTER_VERTICAL);
            params.leftMargin=dp(a,16);bar.addView(icon,params);
        }
        if (refresh != null) {
            View icon = new View(a) {
                protected void onDraw(android.graphics.Canvas canvas) {
                    android.graphics.Paint paint = new android.graphics.Paint(3);
                    float scale = getWidth() / 44f;
                    canvas.save(); canvas.scale(scale, scale);
                    if(!groupedActions){paint.setColor(0xff272727);canvas.drawCircle(22,22,21,paint);
                        paint.setColor(BORDER);paint.setStyle(android.graphics.Paint.Style.STROKE);paint.setStrokeWidth(1);canvas.drawCircle(22,22,21,paint);}
                    paint.setStyle(android.graphics.Paint.Style.STROKE);
                    paint.setColor(TEXT); paint.setStrokeWidth(1.8f); paint.setStrokeCap(android.graphics.Paint.Cap.ROUND);
                    canvas.drawArc(12,12,32,33,-60,-300,false,paint);
                    canvas.drawLine(22,10,27,13.4f,paint); canvas.drawLine(27,13.4f,23,18,paint);
                    canvas.restore();
                }
            };
            refreshControl=icon;
            icon.setContentDescription("Refresh machines"); icon.setOnClickListener(v -> refresh.run());
            FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(dp(a,44),dp(a,44),android.view.Gravity.RIGHT|android.view.Gravity.CENTER_VERTICAL);
            params.rightMargin=dp(a,groupedActions?18:16); bar.addView(icon,params);
        }
        if (title.equals("Machines")) {
            View settings = new View(a) {
                protected void onDraw(android.graphics.Canvas canvas) {
                    android.graphics.Paint paint=new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
                    canvas.save();canvas.scale(getWidth()/44f,getHeight()/44f);
                    if(!groupedActions){paint.setColor(0xff272727);canvas.drawCircle(22,22,21,paint);
                        paint.setStyle(android.graphics.Paint.Style.STROKE);paint.setStrokeWidth(1);paint.setColor(BORDER);canvas.drawCircle(22,22,21,paint);}
                    paint.setStyle(android.graphics.Paint.Style.STROKE);paint.setColor(TEXT);paint.setStrokeWidth(1.5f);paint.setStrokeJoin(android.graphics.Paint.Join.ROUND);
                    android.graphics.Path gear=new android.graphics.Path();
                    for(int i=0;i<48;i++) {
                        double angle=-Math.PI/2+i*Math.PI/24;
                        float radius=(i%6==0||i%6==5)?8:10;
                        float x=22+(float)Math.cos(angle)*radius,y=22+(float)Math.sin(angle)*radius;
                        if(i==0)gear.moveTo(x,y);else gear.lineTo(x,y);
                    }
                    gear.close();canvas.drawPath(gear,paint);canvas.drawCircle(22,22,3.5f,paint);canvas.restore();
                }
            };
            settings.setContentDescription("Device settings");settings.setOnClickListener(v->showDeviceSettings(a));
            FrameLayout.LayoutParams params=new FrameLayout.LayoutParams(dp(a,44),dp(a,44),android.view.Gravity.RIGHT|android.view.Gravity.CENTER_VERTICAL);
            params.rightMargin=dp(a,groupedActions?74:64);bar.addView(settings,params);
        }
        View line = new View(a); line.setBackgroundColor(BORDER);
        bar.addView(line,new FrameLayout.LayoutParams(-1,dp(a,0.5f),android.view.Gravity.BOTTOM));
        bar.setLayoutParams(size(a,60)); return bar;
    }

    private TerminalUi() {}
    private static native void nativeEvent(String action, String payload);
    private static void event(String action) { nativeEvent(action, ""); }

    public static void update(Activity activity, String action, String payload) {
        activity.runOnUiThread(() -> {
            if (activity.isFinishing() || activity.isDestroyed()) return;
            try {
                switch (action) {
                    case "show":
                    case "update":
                        model = new JSONObject(payload);
                        if (action.equals("show")) show(activity);
                        else if (picker != null && picker.isShowing()) renderRows();
                        break;
                    case "trust": showTrust(activity, new JSONObject(payload)); break;
                    case "login":
                        Uri uri = Uri.parse(payload);
                        if (!"https".equals(uri.getScheme()) || !"login.tailscale.com".equals(uri.getHost())) {
                            throw new IllegalArgumentException("Unexpected Tailscale sign-in address");
                        }
                        activity.startActivity(new Intent(Intent.ACTION_VIEW, uri));
                        break;
                    case "screen": updateScreen(activity, new JSONObject(payload)); break;
                    case "keyboard": keyboard(activity, payload.equals("show")); break;
                }
            } catch (Exception error) {
                android.util.Log.e("TerminalUi", "action="+action+" error="+error.getClass().getSimpleName());
                nativeEvent("error", "Could not update Android terminal controls. Try again.");
            }
        });
    }

    private static TextView label(Activity activity, String text) {
        TextView view = new TextView(activity); view.setText(text); font(view, 10, false);
        view.setTextColor(MUTED); return view;
    }
    private static Button button(Activity activity, String text, Runnable action) {
        Button button = new Button(activity); button.setText(text); button.setAllCaps(false);
        font(button,12,true); button.setMinHeight(0); button.setMinimumHeight(0);
        button.setMinWidth(0); button.setMinimumWidth(0); button.setPadding(0,0,0,0);
        button.setGravity(android.view.Gravity.LEFT|android.view.Gravity.CENTER_VERTICAL);
        button.setBackgroundColor(Color.TRANSPARENT);
        button.setOnClickListener(v -> action.run()); return button;
    }
    private static LinearLayout column(Activity activity) {
        LinearLayout layout = new LinearLayout(activity); layout.setOrientation(LinearLayout.VERTICAL);
        layout.setBackgroundColor(BG); return layout;
    }
    private static EditText input(Activity activity, String hint, String value) {
        EditText input = new EditText(activity); input.setSingleLine(true); font(input,14,false);
        input.setHintTextColor(0xff606060); input.setHint(hint); input.setContentDescription(hint);
        input.setText(value); input.setPadding(dp(activity,12),0,dp(activity,12),0);
        input.setBackground(shape(activity,SURFACE,6,true));
        input.setInputType(android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        input.setImeOptions(android.view.inputmethod.EditorInfo.IME_ACTION_DONE);
        input.setOnEditorActionListener((view, action, event) -> {
            if (action != android.view.inputmethod.EditorInfo.IME_ACTION_DONE) return false;
            ((android.view.inputmethod.InputMethodManager) activity.getSystemService(Context.INPUT_METHOD_SERVICE))
                    .hideSoftInputFromWindow(view.getWindowToken(), 0);
            view.clearFocus(); return true;
        });
        return input;
    }
    private static void show(Activity activity) {
        if (picker != null && picker.isShowing()) return;
        owner=activity; picker=new Dialog(activity,android.R.style.Theme_DeviceDefault_NoActionBar);
        LinearLayout content=column(activity); content.setFocusableInTouchMode(true);
        content.addView(navigation(activity,"Machines",model.optBoolean("has_session") ? () -> picker.dismiss() : null, () -> event("refresh")));
        ScrollView scroll=new ScrollView(activity); scroll.setFillViewport(false);
        LinearLayout body=column(activity); scroll.addView(body); content.addView(scroll,new LinearLayout.LayoutParams(-1,0,1));
        LinearLayout header=column(activity); header.setPadding(dp(activity,18),dp(activity,18),dp(activity,18),0);
        LinearLayout searchBox=new LinearLayout(activity); searchBox.setGravity(android.view.Gravity.CENTER_VERTICAL);
        searchBox.setBackground(shape(activity,SURFACE,6,true));
        View magnifier=new View(activity) {
            protected void onDraw(android.graphics.Canvas canvas) {
                android.graphics.Paint p=new android.graphics.Paint(3); p.setColor(MUTED); p.setStyle(android.graphics.Paint.Style.STROKE); p.setStrokeWidth(dp(activity,1.4f));
                canvas.drawCircle(dp(activity,18),dp(activity,20),dp(activity,6),p);
                canvas.drawLine(dp(activity,22),dp(activity,24),dp(activity,28),dp(activity,30),p);
            }
        };
        searchBox.addView(magnifier,new LinearLayout.LayoutParams(dp(activity,40),-1));
        search=input(activity,"Find a machine",""); search.setBackgroundColor(Color.TRANSPARENT); search.setPadding(0,0,dp(activity,8),0);
        searchBox.addView(search,new LinearLayout.LayoutParams(0,-1,1)); header.addView(searchBox,size(activity,44));
        gap(header,12); status=label(activity,""); status.setMaxLines(2); status.setPadding(dp(activity,2),0,0,0); header.addView(status);
        body.addView(header,size(activity,118)); rows=column(activity); body.addView(rows);
        footer=column(activity); body.addView(footer);
        search.addTextChangedListener(new TextWatcher() {
            public void beforeTextChanged(CharSequence s,int start,int count,int after) {}
            public void onTextChanged(CharSequence s,int start,int before,int count) { renderRows(); }
            public void afterTextChanged(Editable value) {}
        });
        present(picker,content); renderRows();
    }
    private static void section(String text) {
        TextView title=label(owner,text); font(title,10,true); title.setTextColor(MUTED);
        title.setGravity(android.view.Gravity.CENTER_VERTICAL); title.setPadding(dp(owner,18),0,0,0);
        rows.addView(title,size(owner,34));
    }
    // Draw the same simple terminal, computer and server silhouettes used by the iOS picker.
    private static View machineIcon(Activity activity, String kind) {
        return new View(activity) {
            protected void onDraw(android.graphics.Canvas canvas) {
                android.graphics.Paint paint=new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
                paint.setColor(MUTED); paint.setStyle(android.graphics.Paint.Style.STROKE);
                paint.setStrokeWidth(1.4f); paint.setStrokeCap(android.graphics.Paint.Cap.ROUND);
                paint.setStrokeJoin(android.graphics.Paint.Join.ROUND);
                canvas.save(); canvas.translate(0,(getHeight()-dp(activity,24))/2f);
                canvas.scale(dp(activity,24)/24f,dp(activity,24)/24f);
                if(kind.equals("terminal")) {
                    canvas.drawRoundRect(1,4,23,20,2,2,paint);
                    canvas.drawLine(5,9,8,12,paint); canvas.drawLine(8,12,5,15,paint);
                    canvas.drawLine(11,15,16,15,paint);
                } else if(kind.equals("laptop")) {
                    canvas.drawRoundRect(4,3,20,17,1,1,paint);
                    canvas.drawLine(4,17,1,21,paint); canvas.drawLine(1,21,23,21,paint); canvas.drawLine(23,21,20,17,paint);
                } else if(kind.equals("desktop")) {
                    canvas.drawRoundRect(1,3,23,17,1.5f,1.5f,paint);
                    canvas.drawLine(12,17,12,22,paint); canvas.drawLine(7,22,17,22,paint);
                } else {
                    canvas.drawRoundRect(2,3,22,11,1.5f,1.5f,paint);
                    canvas.drawRoundRect(2,13,22,21,1.5f,1.5f,paint);
                    canvas.drawPoint(6,7,paint); canvas.drawPoint(6,17,paint);
                    canvas.drawLine(11,7,18,7,paint); canvas.drawLine(11,17,18,17,paint);
                }
                canvas.restore();
            }
        };
    }
    private static void row(String title,String subtitle,String iconKind,Runnable action) {
        LinearLayout row=new LinearLayout(owner); row.setGravity(android.view.Gravity.CENTER_VERTICAL);
        row.setPadding(dp(owner,18),0,dp(owner,18),0);
        row.setBackground(new android.graphics.drawable.RippleDrawable(
                android.content.res.ColorStateList.valueOf(BORDER),null,shape(owner,Color.WHITE,0,false)));
        row.addView(machineIcon(owner,iconKind),new LinearLayout.LayoutParams(dp(owner,48),-1));
        LinearLayout text=column(owner); text.setBackgroundColor(Color.TRANSPARENT);
        TextView name=label(owner,title); font(name,13,true); name.setSingleLine(); name.setEllipsize(android.text.TextUtils.TruncateAt.END); text.addView(name);
        gap(text,6); TextView detail=label(owner,subtitle); detail.setSingleLine(); detail.setEllipsize(android.text.TextUtils.TruncateAt.END); text.addView(detail);
        row.addView(text,new LinearLayout.LayoutParams(0,-2,1));
        View indicator=new View(owner) {
            protected void onDraw(android.graphics.Canvas canvas) {
                android.graphics.Paint paint=new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
                paint.setColor(MUTED);paint.setStyle(android.graphics.Paint.Style.STROKE);paint.setStrokeWidth(dp(getContext(),1.5f));
                float x=getWidth()/2f,y=getHeight()/2f;
                canvas.drawLine(x-dp(getContext(),2),y-dp(getContext(),5),x+dp(getContext(),2),y,paint);
                canvas.drawLine(x+dp(getContext(),2),y,x-dp(getContext(),2),y+dp(getContext(),5),paint);
            }
        };
        row.addView(indicator,new LinearLayout.LayoutParams(dp(owner,18),dp(owner,24)));
        row.setOnClickListener(v -> action.run()); row.setContentDescription(title+", "+subtitle);
        rows.addView(row,size(owner,76)); View line=new View(owner); line.setBackgroundColor(BORDER);
        LinearLayout.LayoutParams border=size(owner,0); border.height=Math.max(1,dp(owner,0.5f)); border.leftMargin=dp(owner,66); border.rightMargin=dp(owner,18); rows.addView(line,border);
    }
    private static boolean matches(JSONObject host,JSONObject machine) {
        String address=host.optString("address"); if (!address.contains("@")) return false;
        String endpoint=address.substring(address.indexOf('@')+1);
        if(endpoint.startsWith("[") && !endpoint.contains("]"))return false;
        String target=endpoint.startsWith("[") ? endpoint.substring(1,endpoint.indexOf(']')) : endpoint.split(":",2)[0];
        String hostname=machine.optString("hostname");
        return target.equalsIgnoreCase(machine.optString("address")) || target.equalsIgnoreCase(hostname) || target.equalsIgnoreCase(hostname.split("\\.",2)[0]);
    }
    private static void renderRows() {
        if (rows==null || owner==null) return;
        String message=model.optString("message"); boolean ready=model.optBoolean("tailnet_ready");
        if(refreshControl!=null){refreshControl.setEnabled(!model.optBoolean("refreshing"));refreshControl.setAlpha(model.optBoolean("refreshing")?0.4f:1f);}
        rows.removeAllViews(); String entered=search.getText().toString().trim(); String query=entered.toLowerCase(java.util.Locale.ROOT);
        JSONArray saved=model.optJSONArray("saved"); JSONObject roster=model.optJSONObject("roster");
        JSONArray machines=roster==null ? null : roster.optJSONArray("machines");
        int count=0;
        if(saved!=null) for(int i=0;i<saved.length();i++) {
            JSONObject host=saved.optJSONObject(i); if(host==null)continue;
            String name=host.optString("name"),address=host.optString("address");
            if(!(name+address).toLowerCase(java.util.Locale.ROOT).contains(query))continue;
            if(count++==0)section("SAVED CONNECTIONS");
            row(name,address,"terminal",()->connect(host));
        }
        int discovered=0;
        if(machines!=null)for(int i=0;i<machines.length();i++) {
            JSONObject machine=machines.optJSONObject(i); if(machine==null)continue;
            String name=machine.optString("name"),address=machine.optString("address");
            boolean duplicate=false; if(saved!=null)for(int j=0;j<saved.length();j++)if(saved.optJSONObject(j)!=null&&matches(saved.optJSONObject(j),machine))duplicate=true;
            if(duplicate || !(name+address+machine.optString("hostname")).toLowerCase(java.util.Locale.ROOT).contains(query))continue;
            if(discovered++==0)section("COMPUTERS");
            row(name,machine.optString("os")+" · "+(machine.optBoolean("online") ? "Online in Tailscale" : "Offline in Tailscale"),machine.optString("os").equalsIgnoreCase("macos") ? (name.toLowerCase(java.util.Locale.ROOT).contains("macbook") ? "laptop" : "desktop") : "server",()->connection(owner,machine));
        }
        if(model.optBoolean("refreshing"))status.setText("Refreshing Tailscale machines…");
        else if(!message.isEmpty())status.setText(message);
        else if(!ready)status.setText("Connect to Tailscale to see your machines.");
        else if(!query.isEmpty()) {
            int total=count+discovered;
            status.setText(total==0 ? "No results for “"+entered+"”.\nTry another name, hostname, or IP address."
                    : total+" "+(total==1?"result":"results")+" for “"+entered+"”\nChoose a computer to connect over SSH.");
        } else if(machines==null||machines.length()==0)status.setText("No computers are available in this tailnet yet.");
        else {
            long age=Math.max(0,System.currentTimeMillis()/1000-roster.optLong("updated_at"));
            String time=age<60?"just now":age<3600?(age/60)+"m ago":(age/3600)+"h ago";
            status.setText("Choose a computer to connect over SSH.\nTailscale machines · updated "+time);
        }
        if(ready){footer.removeAllViews();footer.setVisibility(View.GONE);}
        else {footer.setVisibility(View.VISIBLE);renderDeviceSettings(footer);}
        if(deviceSettings!=null&&deviceSettings.isShowing())renderDeviceSettings(settingsForm);
    }
    private static void showDeviceSettings(Activity activity) {
        if(deviceSettings!=null&&deviceSettings.isShowing())return;
        Dialog dialog=new Dialog(activity,android.R.style.Theme_DeviceDefault_NoActionBar);deviceSettings=dialog;
        LinearLayout page=column(activity);page.setFocusableInTouchMode(true);
        page.addView(navigation(activity,"Device settings",dialog::dismiss,null));
        ScrollView scroll=new ScrollView(activity);settingsForm=column(activity);scroll.addView(settingsForm);
        page.addView(scroll,new LinearLayout.LayoutParams(-1,0,1));
        dialog.setOnDismissListener(d->{if(deviceSettings==dialog){deviceSettings=null;settingsForm=null;}if(picker!=null&&picker.isShowing())renderRows();});
        renderDeviceSettings(settingsForm);present(dialog,page);
    }
    private static TextView settingsLabel(String text,int size) {
        TextView label=label(owner,text);font(label,size,false);label.setTextColor(MUTED);return label;
    }
    private static void renderDeviceSettings(LinearLayout form) {
        if(form==null||owner==null)return;
        EditText previous=form.findViewWithTag("tailscale-device-name");
        // Model refreshes must not replace the field while the user is typing.
        if(previous!=null&&previous.hasFocus())return;
        boolean ready=model.optBoolean("tailnet_ready"),pending=model.optString("message").equals("Waiting for Tailscale sign-in…");
        form.removeAllViews();form.setMinimumHeight(dp(owner,ready?360:304));form.setPadding(dp(owner,18),dp(owner,24),dp(owner,18),dp(owner,24));
        form.addView(settingsLabel(ready?"THIS DEVICE":"CONNECT THIS DEVICE",10));gap(form,10);
        form.addView(settingsLabel("How this device appears in Tailscale.",11));gap(form,24);
        form.addView(settingsLabel("DEVICE NAME",10));gap(form,8);
        EditText name=input(owner,"Name this Android in Tailscale",owner.getPreferences(Context.MODE_PRIVATE).getString("tailscale-device-name","iZed Android"));
        name.setTag("tailscale-device-name");name.setContentDescription("This device’s Tailscale name");font(name,12,false);form.addView(name,size(owner,44));gap(form,10);
        form.addView(settingsLabel("This names your Android device, not the computer you connect to.",11));gap(form,20);
        Button action=button(owner,ready?"Save device name":pending?"Waiting for Tailscale…":"Connect to Tailscale",()->{
            String value=name.getText().toString().trim();if(value.isEmpty()){name.setError("Enter a device name");return;}
            owner.getPreferences(Context.MODE_PRIVATE).edit().putString("tailscale-device-name",value).apply();
            ((android.view.inputmethod.InputMethodManager)owner.getSystemService(Context.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(name.getWindowToken(),0);
            name.clearFocus();nativeEvent(ready?"name":"login",value);
        });
        action.setGravity(android.view.Gravity.CENTER);action.setBackground(shape(owner,SURFACE,6,true));
        action.setEnabled(!model.optBoolean("refreshing")&&!pending);action.setAlpha(action.isEnabled()?1f:0.4f);form.addView(action,size(owner,44));
        if(ready){gap(form,24);Button logout=button(owner,"Sign out of Tailscale",()->new AlertDialog.Builder(owner)
                .setTitle("Sign out of Tailscale?").setMessage("iZed Terminal will need to sign in again to reach your tailnet.")
                .setNegativeButton("Cancel",null).setPositiveButton("Sign Out",(d,w)->event("logout")).show());
            font(logout,11,false);logout.setTextColor(0xffff453a);form.addView(logout,size(owner,44));}
    }
    private static void connection(Activity activity,JSONObject machine) {
        String name=machine.optString("name"),address=machine.optString("address");
        Dialog setup=new Dialog(activity,android.R.style.Theme_DeviceDefault_NoActionBar);
        LinearLayout page=column(activity); page.addView(navigation(activity,"Connect",setup::dismiss,null));
        ScrollView scroll=new ScrollView(activity); LinearLayout form=column(activity); form.setPadding(dp(activity,22),dp(activity,26),dp(activity,22),dp(activity,28)); scroll.addView(form); page.addView(scroll,new LinearLayout.LayoutParams(-1,0,1));
        TextView title=label(activity,name); font(title,18,false); form.addView(title); gap(form,12);
        form.addView(label(activity,machine.optString("os")+" · "+(machine.optString("hostname").isEmpty()?address:machine.optString("hostname")))); gap(form,30);
        TextView userLabel=label(activity,"SSH USERNAME"); font(userLabel,11,false); userLabel.setTextColor(MUTED); form.addView(userLabel); gap(form,12);
        EditText user=input(activity,"Your account on this machine",""); form.addView(user,size(activity,48)); gap(form,12);
        form.addView(label(activity,"We’ll remember this for next time.")); gap(form,12);
        EditText command=input(activity,"e.g. tmux new-session -A -s phone",""); command.setContentDescription("Command (optional)"); command.setVisibility(View.GONE);
        Button advanced=button(activity,"Launch command (optional)",()->command.setVisibility(command.getVisibility()==View.GONE?View.VISIBLE:View.GONE));font(advanced,11,false);advanced.setTextColor(MUTED);form.addView(advanced,size(activity,32));
        form.addView(command,size(activity,48));gap(form,12);
        Button open=button(activity,"Open terminal  →",()->{
            String username=user.getText().toString().trim();
            if(username.isEmpty()||username.contains("@")||username.matches(".*\\s.*")){user.setError("Enter your SSH username.");return;}
            try {JSONObject host=new JSONObject();host.put("name",name);host.put("address",username+"@"+(address.contains(":")?"["+address+"]":address));host.put("command",command.getText().toString());setup.dismiss();connect(host);}catch(Exception e){user.setError("Invalid SSH address");}
        });font(open,14,true);open.setGravity(android.view.Gravity.CENTER);open.setBackground(shape(activity,0xff1f1f1f,5,true));form.addView(open,size(activity,50));gap(form,12);
        Button key=button(activity,"Copy this phone’s public SSH key",()->{});
        key.setOnClickListener(v->{((ClipboardManager)activity.getSystemService(Context.CLIPBOARD_SERVICE)).setPrimaryClip(ClipData.newPlainText("SSH public key",model.optString("public_key"))); key.setText("Public key copied");}); font(key,10,false);key.setTextColor(MUTED);key.setGravity(android.view.Gravity.CENTER);form.addView(key,size(activity,44));present(setup,page);
    }
    private static void connect(JSONObject host) {
        if (picker != null) picker.dismiss();
        nativeEvent("add", host.toString());
    }
    private static void showTrust(Activity activity, JSONObject host) {
        if (trust != null && trust.isShowing()) trust.dismiss();
        boolean changed = host.optBoolean("changed");
        Dialog dialog = new Dialog(activity);
        dialog.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE);
        // Every exit resolves the pending verification exactly once, including Back,
        // an outside tap, replacement, and Activity destruction. Dismissal never trusts.
        java.util.concurrent.atomic.AtomicBoolean resolved = new java.util.concurrent.atomic.AtomicBoolean();
        java.util.function.Consumer<Boolean> resolve = accepted -> {
            if (resolved.compareAndSet(false, true)) nativeEvent("trust", accepted ? "true" : "false");
        };
        dialog.setOnCancelListener(d -> resolve.accept(false));
        WindowManager windowManager=activity.getSystemService(WindowManager.class);
        java.util.function.Consumer<Boolean> blurChanged=enabled->{
            android.view.Window window=dialog.getWindow();
            window.setBackgroundDrawable(shape(activity,enabled?0xcc222222:0xff222222,36,true));
            if(android.os.Build.VERSION.SDK_INT>=31)window.setBackgroundBlurRadius(enabled?dp(activity,24):0);
        };
        dialog.setOnDismissListener(d -> {
            if(android.os.Build.VERSION.SDK_INT>=31)windowManager.removeCrossWindowBlurEnabledListener(blurChanged);
            resolve.accept(false); if (trust == dialog) trust = null;
        });
        dialog.setCanceledOnTouchOutside(true);

        // Match the production iOS dark alert: left-aligned copy and stacked pill actions.
        LinearLayout panel = column(activity);
        panel.setPadding(dp(activity,16),dp(activity,24),dp(activity,16),dp(activity,16));
        panel.setBackgroundColor(Color.TRANSPARENT);
        LinearLayout copy = column(activity); copy.setBackgroundColor(Color.TRANSPARENT);
        copy.setPadding(dp(activity,14),0,dp(activity,14),0);
        TextView title = new TextView(activity);
        title.setText(changed ? "Host identity changed" : "Verify host identity");
        title.setTypeface(android.graphics.Typeface.create("sans-serif-medium",android.graphics.Typeface.NORMAL));
        title.setTextSize(17); title.setTextColor(Color.WHITE); title.setIncludeFontPadding(false);
        copy.addView(title); gap(copy,7);
        TextView message = new TextView(activity);
        message.setText(host.optString("address") + "\n\n" + host.optString("fingerprint")
                + "\n\nCompare this fingerprint with your host before trusting it.");
        message.setTypeface(android.graphics.Typeface.create("sans-serif",android.graphics.Typeface.NORMAL));
        message.setTextSize(16); message.setTextColor(0xffa0a0a0); message.setIncludeFontPadding(false);
        message.setLineSpacing(dp(activity,1),1); copy.addView(message);
        panel.addView(copy); gap(panel,20);
        Button accept = trustAction(activity,changed ? "Trust new identity" : "Trust and connect",
                changed ? 0xffff5a61 : 0xff0a84ff,()-> { resolve.accept(true); dialog.dismiss(); });
        panel.addView(accept,size(activity,48)); gap(panel,8);
        panel.addView(trustAction(activity,"Cancel",Color.WHITE,()-> { resolve.accept(false); dialog.dismiss(); }),size(activity,48));
        ScrollView scroll = new ScrollView(activity); scroll.setFillViewport(false); scroll.addView(panel);
        dialog.setContentView(scroll); trust = dialog;
        try {
            dialog.show();
            dialog.getWindow().setFormat(android.graphics.PixelFormat.TRANSLUCENT);
            blurChanged.accept(android.os.Build.VERSION.SDK_INT>=31 && windowManager.isCrossWindowBlurEnabled());
            if(android.os.Build.VERSION.SDK_INT>=31)windowManager.addCrossWindowBlurEnabledListener(blurChanged);
            dialog.getWindow().setLayout(Math.min(dp(activity,320),activity.getResources().getDisplayMetrics().widthPixels-dp(activity,48)),-2);
            dialog.getWindow().addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);
            WindowManager.LayoutParams params=dialog.getWindow().getAttributes(); params.dimAmount=0.6f;
            dialog.getWindow().setAttributes(params);
        } catch (RuntimeException error) {
            resolve.accept(false); dialog.dismiss(); throw error;
        }
    }
    private static Button trustAction(Activity activity,String text,int color,Runnable action) {
        Button button = button(activity,text,action);
        button.setTypeface(android.graphics.Typeface.create("sans-serif",android.graphics.Typeface.NORMAL));
        button.setTextSize(17); button.setTextColor(color); button.setGravity(android.view.Gravity.CENTER);
        button.setBackground(new android.graphics.drawable.RippleDrawable(
                android.content.res.ColorStateList.valueOf(0xff505050),shape(activity,0xff3b3b3b,25,false),null));
        return button;
    }
    private static void keyboard(Activity activity, boolean show) {
        android.view.inputmethod.InputMethodManager ime = (android.view.inputmethod.InputMethodManager)
                activity.getSystemService(Context.INPUT_METHOD_SERVICE);
        if (!show) { ime.hideSoftInputFromWindow(activity.getWindow().getDecorView().getWindowToken(),0); return; }
        if (inputView == null) {
            inputView = new View(activity) {
                @Override public boolean onCheckIsTextEditor() { return true; }
                @Override public android.view.inputmethod.InputConnection onCreateInputConnection(
                        android.view.inputmethod.EditorInfo info) {
                    info.inputType = android.text.InputType.TYPE_CLASS_TEXT
                            | android.text.InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
                            | android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
                    info.imeOptions = android.view.inputmethod.EditorInfo.IME_ACTION_NONE
                            | android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI;
                    return new android.view.inputmethod.BaseInputConnection(this,false) {
                        @Override public boolean commitText(CharSequence text,int cursor) {
                            nativeEvent("text",text.toString()); return true;
                        }
                        @Override public boolean deleteSurroundingText(int before,int after) {
                            for(int i=0;i<before;i++)nativeEvent("text","\b");
                            for(int i=0;i<after;i++)nativeEvent("text","\u007f");
                            return true;
                        }
                        @Override public boolean performEditorAction(int action) {
                            nativeEvent("text","\n"); return true;
                        }
                    };
                }
            };
            inputView.setFocusable(true); inputView.setFocusableInTouchMode(true);
            inputView.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
            activity.addContentView(inputView,new android.widget.FrameLayout.LayoutParams(1,1));
            inputView.getViewTreeObserver().addOnGlobalLayoutListener(() -> {
                View root=activity.getWindow().getDecorView();
                android.graphics.Rect visible=new android.graphics.Rect();
                root.getWindowVisibleDisplayFrame(visible);
                int height=Math.max(0,root.getHeight()-visible.bottom);
                if(height<dp(activity,100))height=0;
                if(height!=lastKeyboardHeight) {
                    lastKeyboardHeight=height;
                    nativeEvent("keyboard_height",Float.toString(height/activity.getResources().getDisplayMetrics().density));
                }
            });
        }
        inputView.requestFocus();
        inputView.post(() -> ime.showSoftInput(inputView,0));
    }
    private static final android.graphics.Rect terminalBounds = new android.graphics.Rect();
    private static void updateScreen(Activity activity, JSONObject value) {
        if (screen == null) {
            // NativeActivity owns the window surface. Expose its existing content
            // view rather than adding a Java overlay that is never laid out.
            ViewGroup content = activity.findViewById(android.R.id.content);
            screen = content.getChildCount() > 0 ? content.getChildAt(0) : content;
            screen.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);
            screen.setAccessibilityDelegate(new View.AccessibilityDelegate() {
                @Override public void onInitializeAccessibilityNodeInfo(View host,
                        android.view.accessibility.AccessibilityNodeInfo info) {
                    super.onInitializeAccessibilityNodeInfo(host, info);
                    info.setClassName("android.widget.TextView");
                    info.setBoundsInScreen(terminalBounds);
                    info.setVisibleToUser(true);
                }
            });
        }
        float scale = activity.getResources().getDisplayMetrics().density;
        int[] origin = new int[2]; activity.getWindow().getDecorView().getLocationOnScreen(origin);
        int x = origin[0] + (int)(value.optDouble("x") * scale);
        int y = origin[1] + (int)(value.optDouble("y") * scale);
        terminalBounds.set(x, y, x + Math.max(1,(int)(value.optDouble("width")*scale)),
                y + Math.max(1,(int)(value.optDouble("height")*scale)));
        screen.setContentDescription("ssh-terminal-screen\n" + value.optString("text"));
    }
    public static void resume() { event("refresh"); }
    public static void destroy() {
        if (trust != null && trust.isShowing()) trust.dismiss();
        if (deviceSettings != null) deviceSettings.dismiss();
        if (picker != null) picker.dismiss();
        lastKeyboardHeight = 0; inputView = null; footer = null; settingsForm = null; deviceSettings = null; refreshControl = null; picker = null; trust = null; rows = null; status = null; search = null; screen = null; owner = null;
    }
}
