//! GPUI service credentials encrypted by the same Android Keystore mechanism as SSH.
use super::jni as android;
use anyhow::{Context, Result};
use jni::objects::{JByteArray, JValue};
const HELPER: &str = "com.monkhai.ized.platform.PlatformCredentials";

pub fn write(service: &str, username: &str, password: &[u8]) -> Result<()> {
    let length = u32::try_from(username.len()).context("Credential username is too long")?;
    let mut record = length.to_be_bytes().to_vec();
    record.extend_from_slice(username.as_bytes());
    record.extend_from_slice(password);
    let result = android::with_env(|env| {
        let result = (|| {
            let activity = android::activity(env)?;
            let class = android::find_app_class(env, HELPER)?;
            let service = env.new_string(service).map_err(|e| e.to_string())?;
            let bytes = env
                .byte_array_from_slice(&record)
                .map_err(|e| e.to_string())?;
            let saved = env.call_static_method(
                &class,
                jni::jni_str!("store"),
                jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;[B)V"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&service),
                    JValue::Object(&bytes),
                ],
            );
            env.exception_clear();
            bytes
                .set_region(env, 0, &vec![0; record.len()])
                .map_err(|e| e.to_string())?;
            saved
                .map(|_| ())
                .map_err(|_| "Could not persist Android credential".to_string())
        })();
        env.exception_clear();
        result
    });
    record.fill(0);
    result.map_err(anyhow::Error::msg)
}

pub fn read(service: &str) -> Result<Option<(String, Vec<u8>)>> {
    android::with_env(|env| {
        let result = (|| -> Result<Option<(String, Vec<u8>)>> {
            let activity = android::activity(env).map_err(anyhow::Error::msg)?;
            let class = android::find_app_class(env, HELPER).map_err(anyhow::Error::msg)?;
            let service = env.new_string(service)?;
            let value = env
                .call_static_method(
                    &class,
                    jni::jni_str!("load"),
                    jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;)[B"),
                    &[JValue::Object(&activity), JValue::Object(&service)],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            let bytes = unsafe { JByteArray::from_raw(env, value.as_raw()) };
            let length = bytes.len(env)?;
            let copied = env.convert_byte_array(&bytes);
            bytes.set_region(env, 0, &vec![0; length])?;
            let mut record = copied?;
            let decoded = (|| {
                anyhow::ensure!(record.len() >= 4, "Invalid credential record");
                let length = u32::from_be_bytes(record[..4].try_into()?) as usize;
                let end = 4usize
                    .checked_add(length)
                    .context("Invalid credential length")?;
                anyhow::ensure!(end <= record.len(), "Invalid credential record");
                Ok(Some((
                    std::str::from_utf8(&record[4..end])?.to_owned(),
                    record[end..].to_vec(),
                )))
            })();
            record.fill(0);
            decoded
        })();
        env.exception_clear();
        result.map_err(|_| "Could not read Android credential".to_string())
    })
    .map_err(anyhow::Error::msg)
}

pub fn delete(service: &str) -> Result<()> {
    android::with_env(|env| {
        let result = (|| {
            let activity = android::activity(env)?;
            let class = android::find_app_class(env, HELPER)?;
            let service = env.new_string(service).map_err(|e| e.to_string())?;
            env.call_static_method(
                &class,
                jni::jni_str!("delete"),
                jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
                &[JValue::Object(&activity), JValue::Object(&service)],
            )
            .map(|_| ())
            .map_err(|_| "Could not delete Android credential".to_string())
        })();
        env.exception_clear();
        result
    })
    .map_err(anyhow::Error::msg)
}
