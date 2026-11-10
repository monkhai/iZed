//! Asynchronous Android confirmation dialogs. No dialog callback enters GPUI.
use super::jni::{self as android, JniExt};
use futures::channel::oneshot;
use gpui::PromptButton;
use jni::objects::{JObject, JValue};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::{OnceLock, atomic::{AtomicI64, Ordering}}};

static NEXT_ID: AtomicI64 = AtomicI64::new(1);
type Pending = HashMap<i64, (usize, oneshot::Sender<usize>)>;
static PENDING: OnceLock<Mutex<Pending>> = OnceLock::new();
fn pending() -> &'static Mutex<Pending> { PENDING.get_or_init(Default::default) }

pub(super) fn show(message: &str, detail: Option<&str>, answers: &[PromptButton]) -> Option<oneshot::Receiver<usize>> {
    if answers.is_empty() { return None; }
    let platform = android::platform()?;
    let message = message.to_owned();
    let detail = detail.unwrap_or("").to_owned();
    let labels: Vec<String> = answers.iter().map(|answer| answer.label().to_string()).collect();
    let roles: String = answers.iter().map(|answer| if answer.is_cancel() { 'c' } else { 'o' }).collect();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    pending().lock().insert(id, (labels.len(), tx));
    // prompt() can be reached within GPUI updates. Defer JNI to the shallow
    // native dispatcher, then let Java post View work onto its UI thread.
    platform.foreground_executor().dispatch_on_main_thread(move || {
        let result = android::with_env(|env| {
            let result = (|| {
                let class = android::find_app_class(env, "com.monkhai.ized.platform.GpuiPrompt")?;
                let activity = android::activity(env)?;
                let message = env.new_string(&message).e()?;
                let detail = env.new_string(&detail).e()?;
                let roles = env.new_string(&roles).e()?;
                let labels_array = env.new_object_array(labels.len() as i32, jni::jni_str!("java/lang/String"), JObject::null()).e()?;
                for (index, label) in labels.iter().enumerate() {
                    let label = env.new_string(label).e()?;
                    labels_array.set_element(env, index, &label).e()?;
                }
                env.call_static_method(&class, jni::jni_str!("show"),
                    jni::jni_sig!("(Landroid/app/Activity;JLjava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)V"),
                    &[JValue::Object(&activity), JValue::Long(id), JValue::Object(&message), JValue::Object(&detail),
                      JValue::Object(&labels_array), JValue::Object(&roles)]).e()?;
                Ok(())
            })();
            if result.is_err() { env.exception_clear(); }
            result
        });
        if result.is_err() {
            pending().lock().remove(&id);
            log::warn!("Could not show Android confirmation dialog");
        }
    });
    Some(rx)
}

/// A negative/out-of-range result drops the sender: dismissal never approves an action.
///
/// # Safety
/// Called by the JVM with the primitive request identifier and answer index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_monkhai_ized_platform_GpuiPrompt_nativeComplete(
    _env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, id: i64, answer: i32,
) {
    let pending = pending().lock().remove(&id);
    if let Some((count, sender)) = pending {
        if answer >= 0 && (answer as usize) < count { let _ = sender.send(answer as usize); }
    }
}
