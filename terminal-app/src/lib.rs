#[cfg(target_os = "ios")]
mod app;
mod ssh;
mod tailnet;

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn gpui_ios_register_app() {
    ized_platform::ios::ffi::set_asset_source(assets::Assets);
    ized_platform::ios::ffi::set_app_callback(Box::new(app::open));
}
