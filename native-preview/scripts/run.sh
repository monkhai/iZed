#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"

python3 scripts/generate-catalog.py
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
mkdir -p .build/cache .build/config .build/security .build/ModuleCache
CLANG_MODULE_CACHE_PATH="$PWD/.build/ModuleCache" \
SWIFTPM_MODULECACHE_OVERRIDE="$PWD/.build/ModuleCache" \
swift build --disable-sandbox --cache-path .build/cache \
    --config-path .build/config --security-path .build/security
app_dir="$PWD/.build/TerminalCatalog.app"
mkdir -p "$app_dir/Contents/MacOS"
cp .build/debug/Rndr "$app_dir/Contents/MacOS/TerminalCatalog.next"
mv "$app_dir/Contents/MacOS/TerminalCatalog.next" "$app_dir/Contents/MacOS/TerminalCatalog"
cat > "$app_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>TerminalCatalog</string>
<key>CFBundleIdentifier</key><string>dev.ized.catalog.mac</string>
<key>CFBundleName</key><string>Terminal Catalog</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSAppTransportSecurity</key><dict>
<key>NSAllowsLocalNetworking</key><true/>
</dict>
</dict></plist>
PLIST
if [[ "${1:-}" != "--build-only" ]]; then
    open "$app_dir"
fi
