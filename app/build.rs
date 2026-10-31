fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let directory = std::env::var("IZED_TAILSCALE_LIB_DIR").unwrap_or_else(|_| {
            format!(
                "{}/android/gradle/app/src/main/jniLibs/arm64-v8a",
                std::env::var("CARGO_MANIFEST_DIR").unwrap()
            )
        });
        println!("cargo:rustc-link-search=native={directory}");
        println!("cargo:rerun-if-env-changed=IZED_TAILSCALE_LIB_DIR");
    }
}
