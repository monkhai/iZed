#!/usr/bin/env bash
set -euo pipefail

zed_source="${1:?Pass the Zed source checkout used by the iPad Cargo dependency}"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
output_dir="$repo_root/app/ios/RemoteServers"
native_target="$(rustc -vV | awk '/^host:/ { print $2 }')"
mkdir -p "$output_dir"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if [[ "$target" == "$native_target" ]]; then
        cargo build --manifest-path "$zed_source/Cargo.toml" -p remote_server --features debug-embed --bin remote_server
        binary="$zed_source/target/debug/remote_server"
    else
        rustup target add "$target"
        cargo build --manifest-path "$zed_source/Cargo.toml" --target "$target" -p remote_server --features debug-embed --bin remote_server
        binary="$zed_source/target/$target/debug/remote_server"
    fi
    arch="${target%%-*}"
    if [[ "$arch" == "aarch64" ]]; then arch="arm64"; fi
    archive="$output_dir/macos-$arch.gz"
    gzip -c -9 "$binary" > "$archive.tmp"
    mv "$archive.tmp" "$archive"
done
