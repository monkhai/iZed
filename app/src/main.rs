//! Binary entry point for the iZed app host.
//!
//! On **iOS** this provides the standard `fn main()` entry point which
//! delegates to [`ized_app::ios_main`].
//!
//! On **Android** the `android-activity` crate invokes `android_main` directly
//! from the cdylib defined in `lib.rs` — so `main` is never called.  We still
//! provide a stub so that `cargo check` (without a target triple) succeeds.
//!
//! ## Building
//!
//! ```text
//! # iOS simulator
//! cargo build --target aarch64-apple-ios-sim -p ized-app --features ized
//!
//! # iOS device
//! cargo build --target aarch64-apple-ios -p ized-app --features ized
//!
//! # Android (uses lib.rs cdylib, not this binary)
//! cargo ndk -t arm64-v8a build -p ized-app
//! ```

#[cfg(target_os = "ios")]
fn main() {
    ized_app::ios_main();
}

#[cfg(target_os = "android")]
fn main() {
    // On Android the real entry point is `android_main` in lib.rs, which is
    // called by the `android-activity` crate from the cdylib.  This binary
    // target is unused but must compile.
    eprintln!("This binary is not used on Android. The app enters via android_main() in lib.rs.");
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn main() {
    // Allow `cargo check` / `cargo clippy` on the host (macOS / Linux) to
    // succeed without requiring a mobile target.
    eprintln!(
        "This app host is designed for iOS and Android. \
         Please build with --target aarch64-apple-ios-sim (iOS) or via cargo-ndk (Android)."
    );
}
