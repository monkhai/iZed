#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
python3 scripts/generate-catalog.py
# The feature is opt-in and the generated app has a separate bundle/container.
(cd ../terminal-app && cargo build --lib --locked --offline --target aarch64-apple-ios-sim --features native-catalog)
catalog_build="$PWD/.build/ios"
catalog_app="$catalog_build/TerminalCatalogPreview.app"
mkdir -p "$catalog_app" "$catalog_build/ModuleCache"
catalog_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
# Access-control-only relaxation lets fixtures use file-private production types.
# The UI implementation is rebuilt from the current source on every build.
python3 - <<'PY'
from pathlib import Path
root=Path.cwd();source=root.parent/'terminal-app/ios/MachinePicker.swift'
s=source.read_text().replace('private ','fileprivate ')
fixture=root/'iOS/Fixtures/TerminalFixtures.swift'
(root/'.build/ios/MachinePickerCatalog.swift').write_text(
 f'#sourceLocation(file: "{source}", line: 1)\n'+s+
 f'\n#sourceLocation(file: "{fixture}", line: 1)\n'+fixture.read_text())
PY
for unit in SurfaceProducer ServiceStubs; do
 xcrun --sdk iphonesimulator clang -isysroot "$catalog_sdk" -target arm64-apple-ios18.1-simulator -fobjc-arc \
  -c "iOS/Rendering/$unit.m" -o "$catalog_build/$unit.o"
done
xcrun --sdk iphonesimulator swiftc -sdk "$catalog_sdk" -target arm64-apple-ios18.1-simulator \
 -swift-version 5 -parse-as-library -import-objc-header iOS/Rendering/CatalogBridge.h \
 -module-cache-path "$catalog_build/ModuleCache" \
 iOS/Host/*.swift "$catalog_build/MachinePickerCatalog.swift" "$catalog_build/SurfaceProducer.o" "$catalog_build/ServiceStubs.o" \
 -Xlinker -force_load -Xlinker "$PWD/../terminal-app/target/aarch64-apple-ios-sim/debug/libized_terminal.a" \
 -lc++ -lz -liconv -framework UIKit -framework Network -framework Metal -framework MetalKit \
 -framework QuartzCore -framework CoreGraphics -framework CoreText -framework Security \
 -framework SystemConfiguration -framework AudioToolbox -framework AVFoundation \
 -framework CoreMedia -framework VideoToolbox -framework IOSurface \
 -o "$catalog_app/TerminalCatalogPreview"
cp ../app/fonts/JetBrainsMono-{Regular,Medium}.ttf "$catalog_app/"
cat > "$catalog_app/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>UIAppFonts</key><array><string>JetBrainsMono-Regular.ttf</string><string>JetBrainsMono-Medium.ttf</string></array>
<key>CFBundleExecutable</key><string>TerminalCatalogPreview</string>
<key>CFBundleIdentifier</key><string>dev.ized.catalog.ios</string>
<key>CFBundleName</key><string>Terminal Catalog</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1</string>
<key>MinimumOSVersion</key><string>18.1</string>
<key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
<key>UILaunchScreen</key><dict/>
<key>UIApplicationSceneManifest</key><dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
</dict></plist>
PLIST
codesign --force --sign - "$catalog_app"
print "Built $catalog_app"
