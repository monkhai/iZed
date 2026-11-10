//! Android-only boundaries for the shared iZed workspace.
use crate::{android_keys, android_tailnet, ssh};
use std::sync::{LazyLock, Mutex};
static PROXY: Mutex<Option<ssh::TailscaleProxy>> = Mutex::new(None);

static NETWORK: LazyLock<Mutex<serde_json::Value>> = LazyLock::new(|| Mutex::new(
    serde_json::json!({"tailnet_ready": false, "message": "Connecting to Tailscale…"})
));

pub(crate) fn record_network_error(error: String) {
    *PROXY.lock().unwrap() = None;
    *NETWORK.lock().unwrap() = serde_json::json!({"tailnet_ready": false, "message": error});
}

pub(crate) fn network_event(event: crate::terminal_app::NativeEvent) {
    match event {
        crate::terminal_app::NativeEvent::TailscaleUpdate(_, proxy) => {
            *PROXY.lock().unwrap() = Some(proxy);
            *NETWORK.lock().unwrap() = serde_json::json!({"tailnet_ready": true, "message": ""});
        }
        crate::terminal_app::NativeEvent::TailscaleError(error) => record_network_error(error),
        _ => return,
    }
    // Reuse Beam's device settings, without queuing events for its unopened terminal view.
    let model = NETWORK.lock().unwrap().to_string();
    if crate::android_terminal::call("network_state", &model).is_err() {
        log::warn!("Could not refresh Android network settings");
    }
}

pub(crate) fn network_settings() {
    let model = NETWORK.lock().unwrap().to_string();
    if let Some(platform) = ized_platform::android::jni::platform() {
        platform.foreground_executor().dispatch_on_main_thread(move || {
            if crate::android_terminal::call("network_settings", &model).is_err() {
                log::warn!("Could not show Android network settings");
            }
        });
    }
}

pub(crate) fn initialize() -> anyhow::Result<()> {
    initialize_certificate_verifier()?;
    remote::set_android_ssh_hooks(remote::AndroidSshHooks {
        directory: crate::android_terminal::data_dir().join(".ssh"),
        private_key: || {
            let encoded = android_keys::private_key()?;
            Ok(russh::keys::PrivateKey::from_openssh(encoded.as_str())?)
        },
        connect: |host, port| {
            Box::pin(async move {
                let proxy = PROXY.lock().unwrap().clone()
                .ok_or_else(|| anyhow::anyhow!("Tailscale is not connected yet. Wait for your saved device connection and retry."))?;
                ssh::tailscale_stream(&proxy, &host, port).await
            })
        },
    })?;
    android_tailnet::command(android_tailnet::Command::Refresh);
    Ok(())
}

/// Runs on the existing connection worker, never on the rendering path.
pub(crate) fn server_archives() -> anyhow::Result<std::path::PathBuf> {
    use ized_platform::android::jni as android;
    use jni::objects::JValue;
    android::with_env(|env| {
        let result = (|| {
            let activity = android::activity(env)?;
            let class = android::find_app_class(env, "com.monkhai.ized.platform.IdeAssets")?;
            let path = env
                .call_static_method(
                    &class,
                    jni::jni_str!("remoteServers"),
                    jni::jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
                    &[JValue::Object(&activity)],
                )
                .and_then(|value| value.l())
                .map_err(|_| {
                    "Matching remote server archives are unavailable in this build".to_string()
                })?;
            Ok(std::path::PathBuf::from(android::get_string(env, &path)))
        })();
        env.exception_clear();
        result
    })
    .map_err(anyhow::Error::msg)
}

/// Supply Android's trust manager runtime before the shared HTTP client is used.
fn initialize_certificate_verifier() -> anyhow::Result<()> {
    use ized_platform::android::jni as android;
    android::with_env(|env| {
        let activity = android::activity(env)?;
        // Both JNI wrappers refer to the same attached thread and live Activity.
        // The verifier retains global references; the borrowed wrappers stay local.
        let result = unsafe {
            let mut legacy_env = jni_021::JNIEnv::from_raw(env.get_raw().cast())
                .map_err(|_| "Could not initialize Android certificate verifier".to_string())?;
            let context = jni_021::objects::JObject::from_raw(activity.as_raw().cast());
            rustls_platform_verifier::android::init_with_env(&mut legacy_env, context)
                .map_err(|_| "Could not initialize Android certificate verifier".to_string())
        };
        env.exception_clear();
        result
    }).map_err(anyhow::Error::msg)
}
