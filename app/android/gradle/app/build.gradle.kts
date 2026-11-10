import groovy.json.JsonSlurper

// App module build.gradle.kts for the iZed Platform Demo.
//
// This module packages the pre-compiled Rust native library into an APK
// that uses Android's NativeActivity to host the GPUI application.
//
// The Rust library must be compiled separately and placed into
// app/src/main/jniLibs/<abi>/ before building the APK.
//
// Quick start:
//   cd <repo-root>
//   cargo ndk -t arm64-v8a -o app/android/gradle/app/src/main/jniLibs \
//       build --manifest-path app/Cargo.toml --lib --release
//   cd app/android/gradle
//   ./gradlew assembleDebug

plugins {
    id("com.android.application")
}

android {
    namespace = "com.monkhai.ized"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.monkhai.ized"
        minSdk = 26          // Vulkan 1.0 is mandatory from API 26+
        targetSdk = 34
        versionCode = 1
        versionName = "1.0.0"
        testInstrumentationRunner = "com.monkhai.ized.platform.CompositionProbe"

        // Tell NativeActivity which .so to load.
        // This must match the cdylib / example output name.
        ndk {
            abiFilters += listOf("arm64-v8a")
        }

        // Forward the library name to the manifest via a placeholder.
        manifestPlaceholders["nativeLibraryName"] = "ized_app"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isDebuggable = true
            isJniDebuggable = true
        }
    }

    // We do NOT use CMake / ndk-build — the native library is compiled
    // externally via cargo-ndk and placed directly into jniLibs.
    //
    // Disable the built-in native build system so Gradle doesn't look for
    // a CMakeLists.txt or Android.mk.
    externalNativeBuild {
        // Intentionally left empty.
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    // Tell Gradle where the pre-built .so files live.
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
            assets.srcDir("../../../fonts")
        }
    }

    packaging {
        // Prevent stripping of the Rust library — cargo already strips in
        // release mode and stripping again can break backtraces.
        jniLibs {
            // cargo-ndk also copies dependency cdylibs; this crate is linked
            // into ized_app and is neither loaded nor dynamically needed.
            excludes += "**/libized_platform.so"
            keepDebugSymbols += listOf(
                "*/arm64-v8a/libized_app.so",
                "*/armeabi-v7a/libized_app.so",
                "*/x86_64/libized_app.so",
                "*/x86/libized_app.so"
            )
        }
    }

    // Lint configuration — relaxed for an example project.
    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}

dependencies {
    // AndroidX core for NotificationCompat (used by GpuiNotifications)
    implementation("androidx.core:core:1.12.0")
    // AndroidX SplashScreen compat (used by GpuiActivity to hold splash until native init)
    implementation("androidx.core:core-splashscreen:1.0.1")
    // AndroidX Biometric for BiometricPrompt (used by GpuiAuthActivity)
    implementation("androidx.biometric:biometric:1.1.0")
    // AndroidX Media for MediaSessionCompat (used by GpuiMediaSession for system controls)
    implementation("androidx.media:media:1.7.1")
}

// The full IDE needs the same pinned macOS server archives as iOS. Beam does not.
if (providers.gradleProperty("izedIde").orNull == "true") {
    // Resolve the JVM verifier from Cargo's pinned dependency, never an unrelated Maven version.
    val metadata = providers.exec {
        workingDir(file("../../.."))
        commandLine("cargo", "metadata", "--manifest-path", "Cargo.toml", "--format-version", "1",
            "--locked", "--offline", "--features", "ized", "--filter-platform", "aarch64-linux-android")
    }.standardOutput.asText.get()
    val packages = (JsonSlurper().parseText(metadata) as Map<*, *>)["packages"] as List<*>
    val verifier = packages.map { it as Map<*, *> }.single { it["name"] == "rustls-platform-verifier-android" }
    val verifierDirectory = file(verifier["manifest_path"] as String).parentFile
    val verifierAar = fileTree(verifierDirectory.resolve("maven")) { include("**/*.aar") }.singleFile
    dependencies.add("implementation", files(verifierAar))

    // Development may target only the Mac being tested; distribution defaults to both.
    val serverArchitectures = providers.gradleProperty("izedServerArchitectures")
        .orElse("arm64,x86_64").get().split(",").map { it.trim() }.distinct()
    check(serverArchitectures.isNotEmpty() && serverArchitectures.all { it in listOf("arm64", "x86_64") }) {
        "izedServerArchitectures must contain arm64 and/or x86_64"
    }
    val serverArchives = serverArchitectures.map { "macos-$it.gz" }
    val remoteServers = tasks.register<Sync>("prepareRemoteServerAssets") {
        from("../../../ios/RemoteServers") {
            include(serverArchives)
            // Android asset packaging expands .gz inputs and strips the suffix.
            // Keep the exact archive bytes required by the shared SSH uploader.
            rename { "$it.bin" }
        }
        into(layout.buildDirectory.dir("ideAssets/RemoteServers"))
        doFirst {
            for (name in serverArchives) {
                check(file("../../../ios/RemoteServers/$name").isFile) { "Missing matching remote server: $name" }
            }
        }
    }
    android.sourceSets.getByName("main").assets.srcDir(layout.buildDirectory.dir("ideAssets").get().asFile)
    tasks.named("preBuild").configure { dependsOn(remoteServers) }
}
