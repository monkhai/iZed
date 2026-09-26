//! iZed's iPad host for Zed's remote workspace.
//!
//! The iOS app registers [`ized`] as its GPUI root view. The inherited
//! Android demo remains available as a platform example through [`screens`].
//! Build instructions for a physical iPad are in the repository README.

// Link ized-platform so its symbols (jni helpers, platform, etc.) are available.
extern crate ized_platform;

pub mod demos;
pub mod screens;

#[cfg(all(target_os = "ios", feature = "ized"))]
mod ized;

#[cfg(any(target_os = "ios", target_os = "android"))]
use gpui::{prelude::*, App, WindowOptions};

#[cfg(target_os = "android")]
use gpui::Application;

#[cfg(any(target_os = "ios", target_os = "android"))]
use screens::Router;

// ═══════════════════════════════════════════════════════════════════════════
// Android entry point
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "android")]
use ized_platform::android::jni;

/// Called by the `android-activity` crate on a dedicated native thread.
/// Does NOT return until the app is ready to exit.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    // Logger first — so everything after this is visible in logcat.
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("ized-app"),
    );

    // Panic hook — routes panics to logcat instead of silently aborting.
    jni::install_panic_hook();

    log::info!("android_main: entered");

    // Initialise the global AndroidApp + AndroidPlatform.
    let _platform = jni::init_platform(&app);
    log::info!("android_main: platform initialised");

    // Get a SharedPlatform (Rc-compatible wrapper around the global
    // Arc<AndroidPlatform>) so we can hand it to GPUI.
    let shared = match jni::shared_platform() {
        Some(s) => s,
        None => {
            log::error!("android_main: shared_platform() returned None — aborting");
            return;
        }
    };

    log::info!("android_main: creating GPUI Application");

    // `Application::with_platform(...).run(...)` calls `Platform::run` which,
    // on Android, **blocks** by driving the native event loop.  The user's
    // `|cx| { ... }` closure is deferred: it runs inside `run_event_loop`
    // once `MainEvent::InitWindow` has delivered a native surface and an
    // `AndroidWindow` exists.
    //
    // Because `Platform::run` blocks, the `Application` stays alive on this
    // stack frame for the entire duration of the event loop.  This means the
    // `Rc<RefCell<AppContext>>` (which GPUI callbacks hold via `Weak`) remains
    // valid — solving the lifetime mismatch that previously caused
    // `default_prevented=false` on touch events and potential crashes.
    Application::with_platform(shared.into_rc()).run(|cx: &mut App| {
        log::info!("Application::run callback — opening window with Router");
        open_main_window(cx);
    });

    // `Application::run` returns here only after the event loop exits
    // (i.e. the activity was destroyed or quit() was called).
    log::info!("android_main: Application.run returned — activity will finish");
}

// ═══════════════════════════════════════════════════════════════════════════
// iOS entry point
// ═══════════════════════════════════════════════════════════════════════════

/// Register the iZed root view with the GPUI iOS platform.
///
/// This is called from `main.m` **before** `gpui_ios_run_app()` so that
/// when the GPUI run loop starts it knows which view to create.
///
/// The symbol lives in the iZed app's static library, which is force-loaded
/// alongside `libized_platform.a` by the Xcode linker.
/// Minimal logger that routes Rust `log` crate messages through NSLog.
#[cfg(target_os = "ios")]
struct NsLogLogger;

#[cfg(target_os = "ios")]
impl log::Log for NsLogLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let msg = format!(
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
            nslog(&msg);
        }
    }
    fn flush(&self) {}
}

/// Call NSLog from Rust via raw FFI.
#[cfg(target_os = "ios")]
fn nslog(msg: &str) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        extern "C" {
            fn NSLog(fmt: *mut AnyObject, ...);
        }
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let ns_msg: *mut AnyObject = msg_send![class!(NSString), alloc];
        let ns_msg: *mut AnyObject = msg_send![ns_msg, initWithUTF8String: c_msg.as_ptr()];
        let c_fmt = std::ffi::CString::new("%@").unwrap_or_default();
        let ns_fmt: *mut AnyObject = msg_send![class!(NSString), alloc];
        let ns_fmt: *mut AnyObject = msg_send![ns_fmt, initWithUTF8String: c_fmt.as_ptr()];
        NSLog(ns_fmt, ns_msg);
    }
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_register_app() {
    #[cfg(feature = "ized")]
    ized_platform::ios::ffi::set_asset_source(assets::Assets);

    // Set up Rust logging → NSLog so log::info! etc. appear in devicectl --console.
    let _ = log::set_logger(&NsLogLogger).map(|()| log::set_max_level(log::LevelFilter::Info));

    // Panic hook → NSLog so panics are visible.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("GPUI PANIC: {info}");
        nslog(&msg);
    }));

    ized_platform::ios::ffi::set_app_callback(Box::new(|cx: &mut App| {
        #[cfg(feature = "ized")]
        ized::open(cx);
        #[cfg(not(feature = "ized"))]
        open_main_window(cx);
    }));
}

/// Convenience entry point for the binary target (`main.rs`).
#[cfg(target_os = "ios")]
pub fn ios_main() {
    gpui_ios_register_app();
    ized_platform::ios::ffi::run_app();
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared window creation
// ═══════════════════════════════════════════════════════════════════════════

/// Open the main application window with the shared `Router` view.
///
/// This is called from both the Android and iOS entry points.  On both
/// platforms, windows are fullscreen so `window_bounds` is `None`.
///
/// If the app was launched via a deeplink (e.g. `ized://video_player`),
/// the router starts on the corresponding screen.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn open_main_window(cx: &mut App) {
    // Set up HTTP client so gpui::img() can fetch remote images (e.g. picsum.photos).
    // We build the reqwest client ourselves so we can skip TLS cert verification.
    // On iOS, rustls-native-certs fails to load system root certs, causing all
    // HTTPS requests to fail with "UnknownIssuer". Since this is a demo app,
    // disabling cert verification is acceptable.
    log::info!("Setting up HTTP client for image loading...");
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to create reqwest client");
    let http_client: reqwest_client::ReqwestClient = client.into();
    cx.set_http_client(std::sync::Arc::new(http_client));
    log::info!("HTTP client configured successfully");

    // Check if the app was launched via a deeplink and determine the initial screen.
    let initial_screen = match ized_platform::packages::deeplink::get_initial_link() {
        Ok(Some(url)) => {
            log::info!("Deeplink: launched with URL: {url}");
            screens::Screen::from_deeplink_url(&url).unwrap_or_default()
        }
        Ok(None) => {
            log::info!("Deeplink: no initial link");
            screens::Screen::default()
        }
        Err(e) => {
            log::warn!("Deeplink: error getting initial link: {e}");
            screens::Screen::default()
        }
    };
    log::info!("Initial screen: {:?}", initial_screen);

    match cx.open_window(
        WindowOptions {
            window_bounds: None,
            ..Default::default()
        },
        |_, cx| cx.new(|_| Router::with_initial_screen(initial_screen)),
    ) {
        Ok(_handle) => {
            #[cfg(target_os = "android")]
            log::info!("cx.open_window succeeded — Router is live");
        }
        Err(_e) => {
            #[cfg(target_os = "android")]
            log::error!("cx.open_window failed: {_e:#}");

            #[cfg(target_os = "ios")]
            eprintln!("cx.open_window failed: {_e:#}");
        }
    }

    cx.activate(true);
}
