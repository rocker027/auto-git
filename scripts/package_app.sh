#!/usr/bin/env bash
# Build release binary and package macOS .app bundle with Codex-designed icon.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="${HOME}/.rustup/toolchains/stable-aarch64-apple-darwin/bin:${PATH}"

APP_NAME="Note Sync"
BUNDLE_ID="app.notesync.note-sync"
DIST="${ROOT}/dist"
APP="${DIST}/${APP_NAME}.app"
ICON_SRC="${ROOT}/assets/icon/icon-1024.png"
ICONSET="${DIST}/AppIcon.iconset"
ICNS="${DIST}/AppIcon.icns"

if [[ ! -f "${ICON_SRC}" ]]; then
  echo "missing icon: ${ICON_SRC}" >&2
  exit 1
fi

echo "==> cargo build --release"
(
  cd "${ROOT}"
  cargo build --release
)

BIN="${ROOT}/target/release/note_sync"
if [[ ! -x "${BIN}" ]]; then
  echo "missing binary: ${BIN}" >&2
  exit 1
fi

echo "==> prepare iconset"
rm -rf "${ICONSET}" "${ICNS}" "${APP}"
mkdir -p "${ICONSET}" "${DIST}"

# Generate required iconutil sizes from 1024 master.
declare -a SIZES=(16 32 128 256 512)
for size in "${SIZES[@]}"; do
  sips -z "${size}" "${size}" "${ICON_SRC}" --out "${ICONSET}/icon_${size}x${size}.png" >/dev/null
done
# @2x variants
sips -z 32 32 "${ICON_SRC}" --out "${ICONSET}/icon_16x16@2x.png" >/dev/null
sips -z 64 64 "${ICON_SRC}" --out "${ICONSET}/icon_32x32@2x.png" >/dev/null
sips -z 256 256 "${ICON_SRC}" --out "${ICONSET}/icon_128x128@2x.png" >/dev/null
sips -z 512 512 "${ICON_SRC}" --out "${ICONSET}/icon_256x256@2x.png" >/dev/null
sips -z 1024 1024 "${ICON_SRC}" --out "${ICONSET}/icon_512x512@2x.png" >/dev/null

iconutil -c icns "${ICONSET}" -o "${ICNS}"

echo "==> assemble ${APP}"
mkdir -p "${APP}/Contents/MacOS" "${APP}/Contents/Resources"
cp "${BIN}" "${APP}/Contents/MacOS/note_sync"
chmod +x "${APP}/Contents/MacOS/note_sync"
cp "${ICNS}" "${APP}/Contents/Resources/AppIcon.icns"

cat > "${APP}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleExecutable</key>
  <string>note_sync</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

# Ad-hoc sign so Gatekeeper is less noisy for local runs.
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep -s - "${APP}" 2>/dev/null || true
fi

echo "==> done"
echo "${APP}"
ls -lh "${APP}/Contents/MacOS/note_sync" "${APP}/Contents/Resources/AppIcon.icns"
