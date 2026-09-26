#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
patch="$repo_root/patches/zed-ios.patch"
lsp_patch="$repo_root/patches/lsp-ios.patch"
terminal_patch="$repo_root/patches/terminal-ios.patch"
debugger_patch="$repo_root/patches/debugger-ios.patch"
ai_patch="$repo_root/patches/ai-ios.patch"
chatgpt_patch="$repo_root/patches/chatgpt-ios.patch"
status_bar_patch="$repo_root/patches/status-bar-ios.patch"
performance_patch="$repo_root/patches/performance-ios.patch"
agent_command_patch="$repo_root/patches/agent-command-ios.patch"
pane_hover_patch="$repo_root/patches/pane-hover-ios.patch"
trash_patch="$repo_root/patches/trash-ios.patch"
zed_patches=("$patch" "$lsp_patch" "$terminal_patch" "$debugger_patch" "$ai_patch" "$chatgpt_patch" "$status_bar_patch" "$performance_patch" "$agent_command_patch" "$pane_hover_patch")
expected_revision="5688167d224b5eca54875d49afb8bfd73a07915a"

source_paths="$(
    cd "$repo_root/app"
    cargo metadata --format-version 1 --features ized --filter-platform aarch64-apple-ios |
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

applied_count=-1
for ((count=${#zed_patches[@]}; count>=0; count--)); do
    if python3 "$repo_root/scripts/check-zed-patch-state.py" "$zed_source" "${zed_patches[@]:0:count}" 2>/dev/null; then
        applied_count="$count"
        break
    fi
done
if [[ "$applied_count" -eq "${#zed_patches[@]}" ]]; then
    echo "All iZed Zed patches are already applied."
elif [[ "$applied_count" -ge 0 ]]; then
    if [[ "$applied_count" -eq 0 ]] &&
        ! git -C "$zed_source" diff --quiet HEAD -- . ':!Cargo.lock'; then
        echo "Zed source has changes outside the iZed patch stack: $zed_source" >&2
        exit 1
    fi
    for ((index=applied_count; index<${#zed_patches[@]}; index++)); do
        zed_patch="${zed_patches[index]}"
        git -C "$zed_source" apply --check "$zed_patch"
        git -C "$zed_source" apply "$zed_patch"
        echo "Applied $(basename "$zed_patch")."
    done
else
    echo "Could not verify the applied Zed patch stack." >&2
    exit 1
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
