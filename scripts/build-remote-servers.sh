#!/usr/bin/env bash
set -euo pipefail

zed_source="${1:?Pass the Zed source checkout used by the iPad Cargo dependency}"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
output_dir="$repo_root/app/ios/RemoteServers"
native_target="$(rustc -vV | awk '/^host:/ { print $2 }')"
target_dir="${CARGO_TARGET_DIR:-$zed_source/target}"
mkdir -p "$output_dir"

server_features="debug-embed"
case "${2:---all}" in
    --all) targets=(aarch64-apple-darwin x86_64-apple-darwin) ;;
    --native-only) targets=("$native_target"); server_features+=",gpui_platform/runtime_shaders" ;;
    *) echo "Usage: $0 ZED_SOURCE [--all|--native-only]" >&2; exit 2 ;;
esac

for target in "${targets[@]}"; do
    if [[ "$target" == "$native_target" ]]; then
        cargo build --manifest-path "$zed_source/Cargo.toml" -p remote_server --features "$server_features" --bin remote_server
        binary="$target_dir/debug/remote_server"
    else
        rustup target add "$target"
        cargo build --manifest-path "$zed_source/Cargo.toml" --target "$target" -p remote_server --features "$server_features" --bin remote_server
        binary="$target_dir/$target/debug/remote_server"
    fi
    arch="${target%%-*}"
    if [[ "$arch" == "aarch64" ]]; then arch="arm64"; fi
    archive="$output_dir/macos-$arch.gz"
    gzip -c -9 "$binary" > "$archive.tmp"
    mv "$archive.tmp" "$archive"
done
