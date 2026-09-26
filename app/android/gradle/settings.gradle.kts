// Settings for the iZed Platform Demo project.
//
// This is a minimal single-module Gradle project that packages the Rust
// native library (compiled via cargo-ndk) into an APK using NativeActivity.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "iZedPlatformDemo"
include(":app")
