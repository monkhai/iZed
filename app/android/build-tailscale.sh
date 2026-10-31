#!/usr/bin/env bash
set -euo pipefail

# Build the same pinned tsnet source used by iOS, for the app's arm64 Android ABI.
app_dir="$(cd "$(dirname "$0")/.." && pwd)"
sdk_dir="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
ndk_dir="${ANDROID_NDK_HOME:-$sdk_dir/ndk/26.1.10909125}"
case "$(uname -s)" in
  Darwin) host_tag=darwin-x86_64 ;;
  Linux) host_tag=linux-x86_64 ;;
  *) echo "Unsupported Android build host" >&2; exit 1 ;;
esac
compiler="$ndk_dir/toolchains/llvm/prebuilt/$host_tag/bin/aarch64-linux-android26-clang"
test -x "$compiler" || { echo "Set ANDROID_NDK_HOME to an installed NDK" >&2; exit 1; }
output="$app_dir/target/android-tailscale"
mkdir -p "$output" "$app_dir/android/gradle/app/src/main/jniLibs/arm64-v8a"
# Stage the pinned C wrapper plus our Android-only LocalAPI entry points without
# modifying the shared submodule or its iOS build.
source_dir="$(mktemp -d "$output/source.XXXXXX")"
trap 'rm -rf "$source_dir"' EXIT
vendor_dir="$app_dir/../terminal-app/vendor/libtailscale"
cp "$vendor_dir/tailscale.go" "$vendor_dir/tailscale.c" "$vendor_dir/tailscale.h" \
   "$vendor_dir/go.mod" "$vendor_dir/go.sum" "$source_dir/"
cp "$app_dir/android/tailscale/android.go" "$source_dir/"
cd "$source_dir"
CGO_ENABLED=1 GOOS=android GOARCH=arm64 CC="$compiler" GOMAXPROCS=2 \
  go build -p 2 -mod=readonly -buildmode=c-shared -o "$output/libtailscale.so" .
cp "$output/libtailscale.so" "$app_dir/android/gradle/app/src/main/jniLibs/arm64-v8a/libtailscale.so"
