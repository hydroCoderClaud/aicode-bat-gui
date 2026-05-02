#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="AICode BAT GUI"
APP_DIR="$ROOT_DIR/dist/${APP_NAME}.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
BINARY_NAME="aicode-bat-gui"

mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cd "$ROOT_DIR"
cargo build --release

cat > "$CONTENTS_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>AICode BAT GUI</string>
    <key>CFBundleExecutable</key>
    <string>aicode-bat-gui</string>
    <key>CFBundleIdentifier</key>
    <string>com.hydrocoderclaud.aicode-bat-gui.desktop</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>AICode BAT GUI</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>LSUIElement</key>
    <false/>
    <key>LSBackgroundOnly</key>
    <false/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

cp "$ROOT_DIR/target/release/$BINARY_NAME" "$MACOS_DIR/$BINARY_NAME"
chmod 755 "$MACOS_DIR/$BINARY_NAME"

echo "Built app bundle at:"
echo "$APP_DIR"
