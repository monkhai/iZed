// Root build.gradle.kts for the iZed Platform Demo project.
//
// This is a minimal Gradle project that packages the Rust native library
// (compiled separately via cargo-ndk) into an APK using NativeActivity.
//
// Build steps:
//   1. Compile the Rust library:
//      cargo ndk -t arm64-v8a -o app/android/gradle/app/src/main/jniLibs \
//          build --manifest-path app/Cargo.toml --lib --release
//
//   2. Build the APK:
//      ./gradlew assembleDebug
//
//   3. Install on device/emulator:
//      adb install app/build/outputs/apk/debug/app-debug.apk

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.1.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.22")
    }
}

tasks.register("clean", Delete::class) {
    delete(rootProject.layout.buildDirectory)
}

// ── Convenience task: build Rust + APK in one go ────────────────────────────

tasks.register<Exec>("buildRustRelease") {
    group = "rust"
    description = "Compile the Rust native library for arm64-v8a using cargo-ndk."
    workingDir = rootProject.projectDir.parentFile.parentFile.parentFile // repository root
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a",
        "-o", "app/android/gradle/app/src/main/jniLibs",
        "build", "--manifest-path", "app/Cargo.toml", "--lib", "--release"
    )
}

tasks.register<Exec>("buildRustDebug") {
    group = "rust"
    description = "Compile the Rust native library for arm64-v8a (debug) using cargo-ndk."
    workingDir = rootProject.projectDir.parentFile.parentFile.parentFile
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a",
        "-o", "app/android/gradle/app/src/main/jniLibs",
        "build", "--manifest-path", "app/Cargo.toml", "--lib"
    )
}

tasks.register("buildAll") {
    group = "rust"
    description = "Build Rust library (release) and then assemble the debug APK."
    dependsOn("buildRustRelease")
    finalizedBy(":app:assembleDebug")
}
