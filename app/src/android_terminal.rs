//! Android native dialogs and callbacks for the shared production terminal.
use crate::{
    android_tailnet::{self, Command},
    terminal_app::{self, send_native_event, NativeEvent},
};
use ized_platform::android::jni as android;
use jni::objects::{JObject, JValue};
use std::ffi::{c_char, c_void, CStr};

const UI: &str = "com.monkhai.ized.platform.TerminalUi";

fn call(action: &str, payload: &str) -> Result<(), String> {
    android::with_env(|env| {
        let activity = android::activity(env)?;
        let class = android::find_app_class(env, UI)?;
        let action = env.new_string(action).map_err(|e| e.to_string())?;
        let payload = env.new_string(payload).map_err(|e| e.to_string())?;
        let result = env.call_static_method(
            &class,
            jni::jni_str!("update"),
            jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V"),
            &[
                JValue::Object(&activity),
                JValue::Object(&action),
                JValue::Object(&payload),
            ],
        );
        env.exception_clear();
        result
            .map(|_| ())
            .map_err(|_| "Could not open Android terminal controls".into())
    })
}
fn publish_error(error: String) {
    send_native_event(NativeEvent::TailscaleError(error));
}
pub(crate) fn open_login(url: &str) -> anyhow::Result<()> {
    call("login", url).map_err(anyhow::Error::msg)
}

#[no_mangle]
unsafe extern "C" fn terminal_machines(json: *const c_char, present: bool) {
    if let Err(error) = call(
        if present { "show" } else { "update" },
        &CStr::from_ptr(json).to_string_lossy(),
    ) {
        log::error!("{error}");
    }
}
#[no_mangle]
unsafe extern "C" fn terminal_show_trust(
    address: *const c_char,
    fingerprint: *const c_char,
    changed: bool,
) {
    let payload = serde_json::json!({"address": CStr::from_ptr(address).to_string_lossy(),
        "fingerprint": CStr::from_ptr(fingerprint).to_string_lossy(), "changed": changed});
    if call("trust", &payload.to_string()).is_err() {
        terminal_app::terminal_trust_host(false);
    }
}
#[no_mangle]
extern "C" fn terminal_start_tailscale() {
    android_tailnet::command(Command::Refresh);
}
#[no_mangle]
extern "C" fn terminal_refresh_tailscale() {
    android_tailnet::command(Command::Refresh);
}
#[no_mangle]
unsafe extern "C" fn terminal_accessibility(
    value: *const c_char,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let payload = serde_json::json!({"text": CStr::from_ptr(value).to_string_lossy(), "x":x, "y":y, "width":width, "height":height});
    // Prepaint's debug-build stack is too deep to enter ART safely. Coalesce
    // snapshots and deliver through the existing queue outside the render stack.
    static SCREEN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let Some(platform) = android::platform() else { return };
    let mut latest = SCREEN.lock().unwrap();
    let scheduled = latest.is_some();
    *latest = Some(payload.to_string());
    drop(latest);
    if !scheduled {
        platform.foreground_executor().dispatch_on_main_thread(|| {
            let snapshot = SCREEN.lock().unwrap().take();
            if let Some(snapshot) = snapshot {
                if let Err(error) = call("screen", &snapshot) {
                    log::error!("Could not update terminal accessibility: {error}");
                }
            }
        });
    }
}

#[no_mangle]
unsafe extern "C" fn Java_com_monkhai_ized_platform_TerminalUi_nativeEvent(
    _env: *mut c_void,
    _class: *mut c_void,
    action: *mut c_void,
    payload: *mut c_void,
) {
    let result = android::with_env(|env| {
        let action = android::get_string(env, &JObject::from_raw(env, action.cast()));
        let payload = android::get_string(env, &JObject::from_raw(env, payload.cast()));
        match action.as_str() {
            "keyboard_height" => {
                if let Ok(height) = payload.parse::<f32>() { ized_platform::set_keyboard_height(height); }
            }
            "text" => send_native_event(NativeEvent::Text(payload)),
            "refresh" => android_tailnet::command(Command::Refresh),
            "login" => android_tailnet::command(Command::Login(payload)),
            "name" => android_tailnet::command(Command::Name(payload)),
            "logout" => android_tailnet::command(Command::Logout),
            "trust" => terminal_app::terminal_trust_host(payload == "true"),
            "add" => {
                let host: crate::ssh::Host =
                    serde_json::from_str(&payload).map_err(|e| e.to_string())?;
                send_native_event(NativeEvent::Add(host));
            }
            "error" => publish_error(payload),
            _ => {}
        }
        Ok(())
    });
    if let Err(error) = result {
        publish_error(error);
    }
}

pub(crate) fn data_dir() -> std::path::PathBuf {
    let path = android::with_env(|env| {
        let activity = android::activity(env)?;
        let file = env
            .call_method(
                &activity,
                jni::jni_str!("getNoBackupFilesDir"),
                jni::jni_sig!("()Ljava/io/File;"),
                &[],
            )
            .and_then(|v| v.l())
            .map_err(|e| e.to_string())?;
        let path = env
            .call_method(
                &file,
                jni::jni_str!("getAbsolutePath"),
                jni::jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .and_then(|v| v.l())
            .map_err(|e| e.to_string())?;
        Ok(android::get_string(env, &path))
    })
    .expect("Android private data directory");
    path.into()
}

pub(crate) fn network_interfaces() -> anyhow::Result<String> {
    android::with_env(|env| {
        let class = android::find_app_class(env, "com.monkhai.ized.platform.TerminalNetwork")?;
        let result = env.call_static_method(&class, jni::jni_str!("interfaces"),
            jni::jni_sig!("()Ljava/lang/String;"), &[])
            .and_then(|value| value.l()).map_err(|e| e.to_string());
        env.exception_clear();
        Ok(android::get_string(env, &result?))
    }).map_err(anyhow::Error::msg)
}

pub(crate) fn keyboard(show: bool) {
    if let Err(error) = call("keyboard", if show { "show" } else { "hide" }) {
        publish_error(error);
    }
}
