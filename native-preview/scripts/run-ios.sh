#!/bin/zsh
set -euo pipefail
cd "${0:A:h:h}"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
if [[ $# -ne 1 ]]; then
 print -u2 'Usage: zsh scripts/run-ios.sh <booted-ipad-simulator-udid>'
 exit 1
fi
catalog_sim="$1"
for catalog_port in 43220 43221; do
 if lsof -tiTCP:"$catalog_port" -sTCP:LISTEN >/dev/null; then
  print -u2 "Catalog port $catalog_port is already in use. Reuse the existing catalog or stop its terminal first."
  exit 1
 fi
done
zsh scripts/build-ios.sh
xcrun simctl bootstatus "$catalog_sim" -b
xcrun simctl install "$catalog_sim" .build/ios/TerminalCatalogPreview.app
xcrun simctl launch --terminate-running-process "$catalog_sim" dev.ized.catalog.ios
catalog_hid="$(npx --yes --package=serve-sim@0.1.48 -c 'command -v serve-sim')"
catalog_bridge_pid=""
cleanup() {
 if [[ -n "$catalog_bridge_pid" ]]; then kill "$catalog_bridge_pid" 2>/dev/null || true; fi
 xcrun simctl terminate "$catalog_sim" dev.ized.catalog.ios >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM HUP
node scripts/input-bridge.mjs "$catalog_sim" "$catalog_hid" &
catalog_bridge_pid=$!
print 'Native host is running. In another terminal: zsh scripts/run.sh'
wait "$catalog_bridge_pid"
