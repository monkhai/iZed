use std::ffi::{c_char, CStr, CString};
use zeroize::Zeroizing;

unsafe extern "C" {
    fn terminal_load_key() -> *mut c_char;
    fn terminal_store_key(key: *const c_char) -> i32;
    fn terminal_free_key(key: *mut c_char);
}

pub(crate) fn private_key() -> anyhow::Result<Zeroizing<String>> {
    unsafe {
        let stored = terminal_load_key();
        if !stored.is_null() {
            let result = Zeroizing::new(CStr::from_ptr(stored).to_string_lossy().into_owned());
            terminal_free_key(stored);
            return Ok(result);
        }
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)?;
        let encoded = key.to_openssh(Default::default())?;
        let c_key = Zeroizing::new(CString::new(encoded.as_str())?.into_bytes_with_nul());
        let status = terminal_store_key(c_key.as_ptr().cast());
        anyhow::ensure!(
            status == 0,
            "Could not save the SSH key to Keychain ({status}). Unlock the phone and try again."
        );
        Ok(Zeroizing::new(encoded.to_string()))
    }
}
