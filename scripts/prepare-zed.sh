#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
patch="$repo_root/patches/zed-ios.patch"
trash_patch="$repo_root/patches/trash-ios.patch"
expected_revision="5688167d224b5eca54875d49afb8bfd73a07915a"

source_paths="$(
    cd "$repo_root/example"
    cargo metadata --format-version 1 --features editor-spike --filter-platform aarch64-apple-ios |
        python3 -c 'import json, pathlib, sys
packages = json.load(sys.stdin)["packages"]
for name, parent_count in (("ui", 2), ("trash", 0)):
    package = next(package for package in packages if package["name"] == name and package.get("source", "").startswith("git+"))
    print(pathlib.Path(package["manifest_path"]).parents[parent_count])'
)"
zed_source="$(printf '%s\n' "$source_paths" | sed -n '1p')"
trash_source="$(printf '%s\n' "$source_paths" | sed -n '2p')"

if [[ "$(git -C "$zed_source" rev-parse HEAD)" != "$expected_revision" ]]; then
    echo "The Zed checkout does not match the revision used by iZed: $zed_source" >&2
    exit 1
fi

if git -C "$zed_source" apply --reverse --check "$patch" 2>/dev/null; then
    echo "iZed's Zed changes are already applied."
else
    git -C "$zed_source" apply --check "$patch"
    git -C "$zed_source" apply "$patch"
    echo "Applied iZed's Zed changes."
fi

if git -C "$trash_source" apply --reverse --check "$trash_patch" 2>/dev/null; then
    echo "iZed's iOS Trash changes are already applied."
else
    git -C "$trash_source" apply --check "$trash_patch"
    git -C "$trash_source" apply "$trash_patch"
    echo "Applied iZed's iOS Trash changes."
fi

echo "Zed source: $zed_source"
if [[ "${1:-}" == "--patch-only" ]]; then
    exit 0
fi
echo "Building macOS remote servers for the iPad bundle…"
"$repo_root/scripts/build-remote-servers.sh" "$zed_source"
