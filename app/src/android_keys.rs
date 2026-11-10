use anyhow::Context;
use ized_platform::android::jni as android;
use jni::objects::{JByteArray, JValue};
use std::sync::Mutex;
use zeroize::Zeroizing;

const HELPER: &str = "com.monkhai.ized.platform.TerminalKeys";
static SETUP: Mutex<()> = Mutex::new(());

/// Return the same exportable OpenSSH Ed25519 identity used by the shared SSH code.
pub(crate) fn private_key() -> anyhow::Result<Zeroizing<String>> {
    let _guard = SETUP
        .lock()
        .map_err(|_| anyhow::anyhow!("SSH key setup interrupted"))?;
    android::with_env(|env| {
        let result = (|| -> anyhow::Result<Zeroizing<String>> {
            let activity = android::activity(env).map_err(anyhow::Error::msg)?;
            let class = android::find_app_class(env, HELPER).map_err(anyhow::Error::msg)?;
            let stored = env
                .call_static_method(
                    &class,
                    jni::jni_str!("load"),
                    jni::jni_sig!("(Landroid/content/Context;)[B"),
                    &[JValue::Object(&activity)],
                )
                .context("Could not load the Android SSH key")?
                .l()?;
            if !stored.is_null() {
                // The Java method's declared result is byte[]. Clear its plaintext copy
                // after copying into zeroizing Rust storage, including on conversion errors.
                let bytes = unsafe { JByteArray::from_raw(env, stored.as_raw()) };
                let length = bytes.len(env)?;
                let copied = env.convert_byte_array(&bytes).map(Zeroizing::new);
                bytes.set_region(env, 0, &vec![0; length])?;
                let copied = copied?;
                let key = Zeroizing::new(std::str::from_utf8(&copied)?.to_owned());
                russh::keys::PrivateKey::from_openssh(key.as_str())
                    .context("Stored Android SSH key is invalid")?;
                return Ok(key);
            }

            let key =
                russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)?;
            let encoded = key.to_openssh(Default::default())?;
            let bytes = env.byte_array_from_slice(encoded.as_bytes())?;
            let saved = env.call_static_method(
                &class,
                jni::jni_str!("store"),
                jni::jni_sig!("(Landroid/content/Context;[B)V"),
                &[JValue::Object(&activity), JValue::Object(&bytes)],
            );
            // Java also clears this array in finally. Clear here for JNI failures
            // that prevented Java from entering the method.
            env.exception_clear();
            bytes.set_region(env, 0, &vec![0; encoded.len()])?;
            saved.context("Could not save the Android SSH key")?;
            Ok(Zeroizing::new(encoded.to_string()))
        })();
        env.exception_clear();
        result.map_err(|error| format!("{error:#}"))
    })
    .map_err(anyhow::Error::msg)
}
